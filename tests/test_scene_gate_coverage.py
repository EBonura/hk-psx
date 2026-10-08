import copy
import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from validate_scene_gates import verify_coverage


class CoverageEvidence(unittest.TestCase):
    def test_exact_owner_bytes_and_rejection_of_stale_evidence(self):
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'proof.hk';raw=b'coverage-proof!!';path.write_bytes(raw)
            owner=dict(scene_id=7,scene_index=1,raw_path=str(path),raw_len=len(raw),
                       raw_sha256=hashlib.sha256(raw).hexdigest())
            build={'scene_coverage':{'bundles':[owner],'arena_bytes':32}}
            state={'HK_COVERAGE_SCENE':2,'HK_COVERAGE_BYTES':len(raw),
                   'HK_COVERAGE_LOADS':3,'HK_SCENE_LOADS':3}
            scene={'scene_id':7}
            symbols='80000100 80000100 20 1 hk_psx::disc::COVERAGE_BUFFER\n'
            ram=bytes(256)+raw+bytes(32-len(raw))
            self.assertEqual(verify_coverage(build,state,scene,symbols,ram),len(raw))
            for key,value in [('HK_COVERAGE_SCENE',1),('HK_COVERAGE_BYTES',1),('HK_COVERAGE_LOADS',2)]:
                bad=dict(state);bad[key]=value
                with self.subTest(key=key),self.assertRaises(ValueError):
                    verify_coverage(build,bad,scene,symbols,ram)
            for bad_ram in (ram[:256],ram[:256]+bytes(32)):
                with self.assertRaisesRegex(ValueError,'Resident coverage'):
                    verify_coverage(build,state,scene,symbols,bad_ram)
            with self.assertRaisesRegex(ValueError,'allocation'):
                verify_coverage(build,state,scene,symbols.replace('20 1','10 1'),ram)
            duplicate=copy.deepcopy(build);duplicate['scene_coverage']['bundles'].append(owner)
            with self.assertRaisesRegex(ValueError,'owner'):
                verify_coverage(duplicate,state,scene,symbols,ram)
            path.write_bytes(bytes(len(raw)))
            with self.assertRaisesRegex(ValueError,'source changed'):
                verify_coverage(build,state,scene,symbols,ram)

    def test_historical_build_has_no_auxiliary_claim(self):
        self.assertIsNone(verify_coverage({}, {}, {}, '', b''))


if __name__=='__main__':unittest.main()
