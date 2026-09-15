"""Compile the real title module against bounded native GPU/pad traces."""
import json,os,struct,subprocess,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
class MenuRuntimeTests(unittest.TestCase):
    def test_real_menu_navigation_settings_start_and_retry_contract(self):
        build=subprocess.run(['cargo','build','--locked','--manifest-path',str(ROOT/'shared/hk-sim/Cargo.toml'),'--message-format=json'],check=True,capture_output=True,text=True)
        artifacts=[json.loads(line) for line in build.stdout.splitlines()]
        libraries=[Path(f) for a in artifacts if a.get('reason')=='compiler-artifact' and a['target']['name']=='hk_sim' for f in a['filenames'] if f.endswith('.rlib')]
        self.assertEqual(len(libraries),1)
        library=libraries[0]
        with tempfile.TemporaryDirectory(prefix='hk-menu-test-') as temp:
            root=Path(temp);(root/'game').mkdir();(root/'data').mkdir()
            pixels=256*240+64*240+2*192*32
            (root/'data/menu.hk').write_bytes(struct.pack('<8sHHI',b'HKMENU03',320,240,pixels)+bytes(512+pixels)+bytes([6]*95+[0]))
            (root/'data/read_font.hk').write_bytes(bytes(7712))
            binary=root/'menu-tests';env=dict(os.environ,CARGO_MANIFEST_DIR=str(root/'game'))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/menu_runtime.rs'),'--extern','hk_sim='+str(library),'-L','dependency='+str(library.parent/'deps'),'-o',str(binary)],env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
