"""Compile the real Hollow Shade runtime against the cooked art and hk_sim."""
from hk_externs import externs
import json,os,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class ShadeRuntimeTests(unittest.TestCase):
    def test_record_spawn_rule_and_cooked_frame_table(self):
        build=subprocess.run(['cargo','build','--locked','--manifest-path',str(ROOT/'shared/hk-sim/Cargo.toml'),'--message-format=json'],check=True,capture_output=True,text=True)
        artifacts=[json.loads(line) for line in build.stdout.splitlines()]
        libraries=[Path(f) for a in artifacts if a.get('reason')=='compiler-artifact' and a['target']['name']=='hk_sim' for f in a['filenames'] if f.endswith('.rlib')]
        self.assertEqual(len(libraries),1)
        library=libraries[0]
        with tempfile.TemporaryDirectory(prefix='hk-shade-test-') as temp:
            binary=Path(temp)/'shade-tests'
            env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'game'))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/shade_runtime.rs'),
                *externs(),'-o',str(binary)],
                env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
    def test_look_pose_runtime_runs_against_the_cooked_ability_art(self):
        # The same harness as above: shade.rs and ability_art.rs share the animation key space,
        # and look_pose.rs (the Knight's LookUp / LookDown clips) reads both.
        build=subprocess.run(['cargo','build','--locked','--manifest-path',str(ROOT/'shared/hk-sim/Cargo.toml')],check=True,capture_output=True,text=True)
        with tempfile.TemporaryDirectory(prefix='hk-look-pose-test-') as temp:
            binary=Path(temp)/'look-pose-tests'
            env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'game'))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/look_pose_runtime.rs'),
                *externs(),'-o',str(binary)],env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
