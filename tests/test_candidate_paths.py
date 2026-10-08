"""One playable disc path; diagnostic maps/reports may remain internal."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
import paths
spec=importlib.util.spec_from_file_location('candidate_build_tool',ROOT/'host/build_report.py')
build_tool=importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_tool)


class CandidatePaths(unittest.TestCase):
    def test_one_playable_destination_for_every_legacy_variant(self):
        expected={'exe':ROOT/'dist/hk-psx.exe','bin':paths.DISC_LIBRARY/'hk-psx.bin',
                  'cue':paths.DISC_LIBRARY/'hk-psx.cue'}
        for candidate in [False,True]:
            for telemetry in [False,True]:
                self.assertEqual(paths.artifacts(telemetry,candidate),expected)

    def test_internal_maps_remain_distinct_without_extra_discs(self):
        maps=set()
        for candidate in [False,True]:
            for telemetry in [False,True]:
                link=paths.link_map_path(telemetry,candidate)
                self.assertNotIn(link,maps);maps.add(link)
                self.assertTrue(link.is_relative_to(ROOT/'build'))
                self.assertEqual(paths.artifacts(telemetry,candidate)['cue'].parent,paths.DISC_LIBRARY)

    def test_candidate_evidence_never_changes_stable_reports(self):
        # JSON only: no EXEs, BINs or CUEs are created by this test.
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'.hkpsx').mkdir()
            names=['build.json','build-normal.json','build-telemetry.json']
            for name in names:(root/'.hkpsx'/name).write_text('stable sentinel')
            with patch.object(paths,'ROOT',root),patch.object(build_tool,'ROOT',root):
                for telemetry in [False,True]:
                    report={'candidate':True,'telemetry':telemetry}
                    build_tool.write_build_report(report,telemetry,candidate=True,latest=True)
                    self.assertEqual(json.loads(paths.build_report_path(telemetry,True).read_text()),report)
                for name in names:self.assertEqual((root/'.hkpsx'/name).read_text(),'stable sentinel')

    def test_stable_latest_is_only_written_for_requested_final_variant(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'.hkpsx').mkdir()
            latest=root/'.hkpsx/build.json';latest.write_text('old')
            with patch.object(paths,'ROOT',root),patch.object(build_tool,'ROOT',root):
                build_tool.write_build_report({'telemetry':True},True,latest=False)
                self.assertEqual(latest.read_text(),'old')
                build_tool.write_build_report({'telemetry':False},False,latest=True)
                self.assertEqual(json.loads(latest.read_text()),{'telemetry':False})

    def test_a_pgo_build_leaves_the_ordinary_report_alone(self):
        # docs/BUDGET.md pins build-normal.json, and a pgo image is a different
        # size, so writing over it failed the next build's test step.
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'.hkpsx').mkdir()
            ordinary=root/'.hkpsx/build-normal.json';ordinary.write_text('ordinary')
            with patch.object(paths,'ROOT',root),patch.object(build_tool,'ROOT',root):
                build_tool.write_build_report({'build_kind':'pgo'},False,latest=True,pgo=True)
            self.assertEqual(ordinary.read_text(),'ordinary')
            self.assertEqual(json.loads((root/'.hkpsx/build-pgo-normal.json').read_text()),{'build_kind':'pgo'})
            self.assertEqual(json.loads((root/'.hkpsx/build.json').read_text()),{'build_kind':'pgo'})


if __name__=='__main__':unittest.main()
