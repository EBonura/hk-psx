"""Host import integrity and coverage semantics using synthetic data only."""
import gzip
import json
from pathlib import Path
import sys
import tempfile
import unittest
from types import SimpleNamespace
sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'host'))
from world_import import (atomic_json, catalog, sha, source_json, summarize,
                          can_retry, failed_worker_row, read_checkpoint, valid_checkpoint,
                          next_attempt_number, stop_process, verify_inputs,
                          worker_outcome, write_gzip)


class ImportTests(unittest.TestCase):
    def test_catalog_retains_non_gameplay_scenes_and_windows_paths(self):
        build = SimpleNamespace(type=SimpleNamespace(name='BuildSettings'))
        source = SimpleNamespace(file=lambda _: SimpleNamespace(objects={1:build}),
            read=lambda _: {'scenes':['Assets\\Scenes\\Menu.unity', 'Assets/Scenes/Room.unity']})
        self.assertEqual(catalog(source), [
            {'index':0,'file':'level0','path':'Assets\\Scenes\\Menu.unity','scene_name':'Menu'},
            {'index':1,'file':'level1','path':'Assets/Scenes/Room.unity','scene_name':'Room'}])

    def test_partial_and_subset_never_claim_resolved_world(self):
        rows = [{'index':0, 'status':'imported', 'component_types':{'Enemy':2}, 'counts':{'sprites':3}},
                {'index':1, 'status':'partial', 'component_types':{'Enemy':1}}]
        summary = summarize(rows, 3)
        self.assertFalse(summary['all_scenes_processed'])
        self.assertFalse(summary['all_geometry_resolved'])
        self.assertEqual(summary['systems']['Enemy']['instances'], 3)
        self.assertEqual(summary['systems']['Enemy']['gameplay_support'], 'not_evaluated')
        summary = summarize(rows, 2)
        self.assertTrue(summary['all_scenes_processed'])
        self.assertFalse(summary['all_geometry_resolved'])
        self.assertEqual(summary['ps1_packing'], 'not_run')
        self.assertEqual(summary['gameplay_validation'], 'not_run')

    def test_checkpoints_reject_modified_output_and_changed_code(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            item = write_gzip(folder/'geometry.json.gz', {'value':1})
            row = {'fingerprint':'abc', 'status':'partial','outputs':{'geometry':item}}
            path = folder/'result.json'
            atomic_json(path,row)
            self.assertEqual(valid_checkpoint(path,'abc'),row)
            self.assertIsNone(valid_checkpoint(path,'def'))
            (folder/item['path']).write_bytes(b'broken')
            self.assertIsNone(valid_checkpoint(path,'abc'))

    def test_failed_checkpoint_is_retried(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'result.json'
            atomic_json(path, {'fingerprint':'abc','status':'failed','outputs':{}})
            self.assertIsNone(valid_checkpoint(path,'abc'))
            self.assertEqual(read_checkpoint(path,'abc',allow_failed=True)['status'], 'failed')

    def test_checkpoint_can_bind_one_worker_attempt(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'result.json'
            atomic_json(path, {'fingerprint':'abc','status':'partial','outputs':{},
                               'worker_attempt':'abc-level2-2'})
            self.assertIsNone(read_checkpoint(path,'abc',attempt_id='abc-level2-1'))
            self.assertIsNotNone(read_checkpoint(path,'abc',attempt_id='abc-level2-2'))

    def test_worker_outcomes_do_not_turn_interruption_into_scene_failure(self):
        checkpoint = {'status':'partial'}
        self.assertEqual(worker_outcome(0, checkpoint), 'completed')
        self.assertEqual(worker_outcome(0, None), 'missing_result')
        self.assertEqual(worker_outcome(-15, None), 'signal:15')
        self.assertEqual(worker_outcome(9, None), 'exit:9')
        self.assertEqual(worker_outcome(-15, None, stopped=True), 'interrupted')
        self.assertEqual(worker_outcome(-15, None, timed_out=True), 'timeout')

    def test_failed_worker_records_exact_exit_without_traceback_guess(self):
        row = failed_worker_row({'index':3,'file':'level3','scene_name':'Room'},
                                'fingerprint','attempt-2','signal:9',-9)
        self.assertEqual(row['status'], 'failed')
        self.assertEqual(row['failure'], {'stage':'isolated_worker','outcome':'signal:9',
                                          'exit_code':-9,'signal':9})

    def test_stop_process_escalates_an_unresponsive_owned_worker(self):
        class Worker:
            exitcode = None
            terminate_calls = 0
            kill_calls = 0
            def is_alive(self):
                return self.kill_calls == 0
            def terminate(self):
                self.terminate_calls += 1
            def kill(self):
                self.kill_calls += 1
                self.exitcode = -9
            def join(self, timeout=None):
                self.timeout = timeout
        worker = Worker()
        self.assertEqual(stop_process(worker, grace_seconds=.01), -9)
        self.assertEqual((worker.terminate_calls, worker.kill_calls), (1, 1))

    def test_resume_attempt_number_preserves_prior_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder/'attempt-1.json').write_text('{}')
            (folder/'attempt-3.json').write_text('{}')
            (folder/'attempt-note.json').write_text('{}')
            self.assertEqual(next_attempt_number(folder), 4)

    def test_retry_budget_uses_run_try_not_persistent_attempt_number(self):
        # A scene may already have attempt-37.json from older fingerprints; its
        # first failure in this run still receives the configured retry.
        self.assertTrue(can_retry(1, 1))
        self.assertFalse(can_retry(2, 1))
        self.assertFalse(can_retry(1, 0))

    def test_input_content_hash_detects_same_length_change(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); path = root/'level0'; path.write_bytes(b'abc')
            recorded = {'level0':{'bytes':3,'sha256':sha(path)}}
            verify_inputs(root, recorded)
            path.write_bytes(b'def')
            with self.assertRaisesRegex(ValueError, 'fingerprint changed'):
                verify_inputs(root, recorded)

    def test_serialized_binary_and_nonfinite_values_are_lossless_tagged_json(self):
        value = source_json({'bytes':b'\x00\xff', 'range':[float('inf'), float('-inf'), 2.0]})
        self.assertEqual(value['bytes'], {'$source_bytes_base64':'AP8='})
        self.assertEqual(value['range'], [{'$source_float':'inf'}, {'$source_float':'-inf'}, 2.0])
        json.dumps(value, allow_nan=False)

    def test_gzip_output_is_reproducible(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'geometry.json.gz'
            first = write_gzip(path, {'points':[[1,2]],'binary':b'abc'})
            second = write_gzip(path, {'points':[[1,2]],'binary':b'abc'})
            self.assertEqual(first, second)
            self.assertEqual(json.loads(gzip.decompress(path.read_bytes()))['points'], [[1,2]])


if __name__ == '__main__':
    unittest.main()
