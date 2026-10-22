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
            art=struct.pack('<8sHHI',b'HKMENU03',320,240,pixels)+bytes(512+pixels)+bytes([6]*95+[0])
            # The disc chunk carries the glyph sheet right after the title art
            # (host/build_guest.py boot_art), then the title's ornaments (3,576 bytes) and the area cards' glyph strip (7,680); MENU_BYTES is the title art alone.
            (root/'data/menu.hk').write_bytes(art+bytes(12832+3576+7680))
            # The art is read from the disc now; the guest links only its size,
            # checksum and glyph advances, which host/cook_menu.py generates.
            (root/'data/menu.rs').write_text(f'pub const MENU_BYTES:usize={len(art)};\npub const MENU_CHECKSUM:u32=0;\n'
                                              f'pub const MENU_METRICS:[u8;96]={[6]*95+[0]};\n')
            # The title's ornaments are cooked by hk-cook dialogue-art; the module links their offsets and sizes only.
            (root/'data/menu_ornaments.rs').write_text('pub const TITLE_FLEUR_TOP:(usize,u8,u8)=(0,192,25);\npub const TITLE_FLEUR_BOTTOM:(usize,u8,u8)=(2400,128,14);\n'
                                                       'pub const TITLE_POINTER:(usize,u8,u8)=(3296,20,14);\npub const TITLE_POINTER_MIRRORED:(usize,u8,u8)=(3436,20,14);\n'
                                                       'pub const TITLE_ORNAMENT_BYTES:usize=3576;\n')
            binary=root/'menu-tests';env=dict(os.environ,CARGO_MANIFEST_DIR=str(root/'game'))
            compiled=subprocess.run(['rustc','--edition=2021','-Awarnings','--test',str(ROOT/'tests/menu_runtime.rs'),'--extern','hk_sim='+str(library),'-L','dependency='+str(library.parent/'deps'),'-o',str(binary)],env=env,capture_output=True,text=True)
            self.assertEqual(compiled.returncode,0,compiled.stdout+compiled.stderr)
            run=subprocess.run([str(binary)],capture_output=True,text=True)
            self.assertEqual(run.returncode,0,run.stdout+run.stderr)
