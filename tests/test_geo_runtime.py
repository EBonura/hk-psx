"""Compile the actual Geo runtime against native shared physics; no retail assets."""
import json
import os
from pathlib import Path
import subprocess
import unittest
ROOT=Path(__file__).resolve().parents[1]
class GeoRuntime(unittest.TestCase):
    def test_native_runtime(self):
        output=ROOT/'.hkpsx/geo-runtime-tests';output.mkdir(parents=True,exist_ok=True)
        env=dict(os.environ,CARGO_TARGET_DIR=str(output/'cargo'))
        built=subprocess.run(['cargo','build','--locked','--offline','--manifest-path',str(ROOT/'shared/hk-sim/Cargo.toml'),'--message-format=json'],cwd=ROOT,env=env,capture_output=True,text=True)
        self.assertEqual(built.returncode,0,built.stdout+built.stderr)
        libs={}
        for line in built.stdout.splitlines():
            record=json.loads(line)
            if record.get('reason')=='compiler-artifact' and record['target']['name']in('hk_sim','hk_format','psx_math'):
                libs[record['target']['name']]=next(Path(p)for p in record['filenames']if p.endswith('.rlib'))
        # game/src/geo.rs calls psx-math directly, as the guest does.
        self.assertEqual(set(libs),{'hk_sim','hk_format','psx_math'})
        cmd=['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/geo_runtime.rs')]
        for name,path in libs.items():cmd+=['--extern',f'{name}={path}']
        cmd+=['-L',f'dependency={output/"cargo/debug/deps"}','-o',str(output/'geo-tests')]
        compiled=subprocess.run(cmd,cwd=ROOT,capture_output=True,text=True)
        self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
        tested=subprocess.run([str(output/'geo-tests'),'--test-threads=1'],cwd=ROOT,capture_output=True,text=True)
        self.assertEqual(tested.returncode,0,tested.stdout+tested.stderr)
if __name__=='__main__':unittest.main()
