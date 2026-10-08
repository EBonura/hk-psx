"""Compile the real prop runtime against the cooked art and hk_sim."""
import json,os,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class PropsRuntimeTests(unittest.TestCase):
    def test_goam_cycle_stalactite_fall_and_grub_jar(self):
        build=subprocess.run(['cargo','build','--locked','--manifest-path',str(ROOT/'shared/hk-sim/Cargo.toml'),'--message-format=json'],check=True,capture_output=True,text=True)
        artifacts=[json.loads(line) for line in build.stdout.splitlines()]
        libraries=[Path(f) for a in artifacts if a.get('reason')=='compiler-artifact' and a['target']['name']=='hk_sim' for f in a['filenames'] if f.endswith('.rlib')]
        self.assertEqual(len(libraries),1)
        library=libraries[0]
        with tempfile.TemporaryDirectory(prefix='hk-props-test-') as temp:
            binary=Path(temp)/'props-tests'
            env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'game'))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/props_runtime.rs'),
                '--extern','hk_sim='+str(library),'-L','dependency='+str(library.parent/'deps'),'-o',str(binary)],
                env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
