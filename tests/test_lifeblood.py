import os,json,subprocess,sys,tempfile,unittest
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from lifeblood import dense_positions,pack_art,LIFE_RECTS

class LifebloodTests(unittest.TestCase):
 def test_dense_packing_is_deterministic_complete_disjoint_and_bounded(self):
  items=[('pal',16,1,16)]+[(i,3+i%6,3+i%11,1)for i in range(18)]
  a=dense_positions(items,LIFE_RECTS);self.assertEqual(a,dense_positions(items,LIFE_RECTS));self.assertEqual(len(a),len(items));used=set()
  for key,(x,y,w,h)in a.items():
   self.assertTrue(352<=x<x+w<=384 and 96<=y<y+h<=176)
   cells={(xx,yy)for xx in range(x,x+w)for yy in range(y,y+h)};self.assertFalse(cells&used);used|=cells
  with self.assertRaises(ValueError):dense_positions([('huge',33,81,1)],LIFE_RECTS)
 def test_full_texture_split_retains_every_row_and_texel(self):
  images=[Image.new('RGBA',(11,37),(0,180,255,255)),Image.new('RGBA',(9,13),(80,200,255,255))]
  blob,uploads,textures,parts,frames,mapping,_=pack_art(images)
  for im,refs in zip(images,frames):
   self.assertEqual(sum(parts[r]['height']for r in refs),im.height)
   self.assertEqual([parts[r]['y']for r in refs],list(range(0,im.height,16))if len(refs)>1 else[0])
   for ref in refs:
    part=parts[ref];texture=textures[mapping[ref]]
    entry=next(u for u in uploads if (u['x']-320)*4==texture['u']and u['y']==texture['v'])
    actual=blob[entry['offset']:entry['offset']+len(part['pixels'])];self.assertEqual(actual,part['pixels'])
    for y in range(part['height']):
     for x in range(im.width):self.assertNotEqual((actual[y*((im.width+3)//4*2)+x//2]>>((x&1)*4))&15,0)
 def test_actual_cooked_native_hud_and_draw_paths(self):
  # Retail-dependent artifact checks are part of the BYO-assets test workflow.
  if not(ROOT/'data/lifeblood.rs').is_file():self.skipTest('cook source Lifeblood bank first')
  for crate in ['hk-sim','hk-cache']:
   run=subprocess.run(['cargo','build','--quiet','--manifest-path',str(ROOT/f'shared/{crate}/Cargo.toml')],capture_output=True,text=True)
   self.assertEqual(run.returncode,0,run.stdout+run.stderr)
  with tempfile.TemporaryDirectory(prefix='hk-life-hud-')as temp:
   args=['rustc','--edition=2021','-Awarnings',str(ROOT/'tests/lifeblood_hud_runtime.rs'),'-o',temp+'/run']
   for crate in ['hk-sim','hk-cache']:
    deps=ROOT/f'shared/{crate}/target/debug/deps';lib=max(deps.glob('lib'+crate.replace('-','_')+'-*.rlib'),key=lambda p:p.stat().st_mtime)
    args+=['--extern',crate.replace('-','_')+'='+str(lib),'-L','dependency='+str(deps)]
   # game/src/lifeblood.rs calls psx-math directly; hk-sim's build leaves it in its deps.
   deps=ROOT/'shared/hk-sim/target/debug/deps';lib=max(deps.glob('libpsx_math-*.rlib'),key=lambda p:p.stat().st_mtime)
   args+=['--extern','psx_math='+str(lib)]
   result=subprocess.run(args,env=dict(os.environ,CARGO_MANIFEST_DIR=str(ROOT/'game')),capture_output=True,text=True)
   self.assertEqual(result.returncode,0,result.stdout+result.stderr)
   result=subprocess.run([temp+'/run'],capture_output=True,text=True);self.assertEqual(result.returncode,0,result.stdout+result.stderr)
if __name__=='__main__':unittest.main()
