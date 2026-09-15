import copy,json,os,subprocess,sys,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from great_door import bind,generate

def record():
 return dict(stage_hits=[4,8,13],cooldown_ticks=9,transition_ticks=152,
             target_scene='Town',position=[190.48,68.42,.61],scale=59172,
             bounds=[189.66,61.72,190.63,76.09],frames=[10,11,12],edges=[],collider_source='level6:8218')

class GreatDoorTests(unittest.TestCase):
 def test_binding_preserves_frame_geometry_and_local_edge_indices(self):
  r=record();before=copy.deepcopy(r)
  b=bind(r,{'edge_sources':['a','level6:8218','b','level6:8218']},100)
  self.assertEqual(b['frames'],[110,111,112]);self.assertEqual(b['edges'],[1,3]);self.assertEqual(r,before)
  with self.assertRaises(ValueError):bind(r,{'edge_sources':['level6:8218']*5},0)
 def test_native_stages_cooldown_and_transition_once(self):
  result=subprocess.run(['cargo','build','--quiet','--locked','--manifest-path',str(ROOT/'shared/hk-sim/Cargo.toml')],capture_output=True,text=True)
  self.assertEqual(result.returncode,0,result.stderr)
  with tempfile.TemporaryDirectory(prefix='hk-great-door-')as tmp:
   p=Path(tmp);(p/'data').mkdir();(p/'game').mkdir()
   metadata={'scenes':[{'scene_id':0,'scene_name':'Tutorial_01'},{'scene_id':1,'scene_name':'Town'}],'regions':[{'great_door':record()},{'great_door':None}]}
   (p/'data/great_door.rs').write_text(generate(metadata))
   harness='''mod world {pub struct State;impl State {pub fn append_script_edges(&mut self,_:&[u16]){}}}
mod render {pub fn texture(_:usize,_:[(i16,i16);4],_:(u8,u8,u8)){}}
#[path="'''+str(ROOT/'game/src/great_door.rs')+'''"]mod great_door;
fn main() {
 use great_door::*;let mut w=World::new();
 let b=BOUNDS;let polygon=[[b[0],b[1]],[b[2],b[1]],[b[2],b[3]],[b[0],b[3]]];
 assert!(!w.strike(1,&polygon).hit);assert!(!w.strike(0,&[[0,0],[1,0],[0,1]]).hit);
 for hit in 1..=13 {
  let s=w.strike(0,&polygon);assert!(s.hit);assert_eq!(s.opened,hit==13);
  for _ in 0..8 {assert!(!w.strike(0,&polygon).hit);assert!(!w.tick(0));}
  assert!(!w.strike(0,&polygon).hit);assert!(!w.tick(0));
  unsafe {assert_eq!(HK_GREAT_DOOR_HITS,hit);assert_eq!(HK_GREAT_DOOR_STAGE,if hit>=8{2}else if hit>=4{1}else{0});}
 }
 assert!(w.opened());assert!(w.pending());assert_eq!(w.frame(0,(POSITION[0],POSITION[1])),None);
 for _ in 0..142 {assert!(!w.tick(0));}
 assert!(w.pending());assert!(w.tick(0));assert!(!w.pending());assert!(!w.tick(0));
 assert!(!w.strike(0,&polygon).hit);unsafe {assert_eq!(HK_GREAT_DOOR_TRANSITIONS,1);}
 w.reset();assert!(!w.opened());assert_eq!(w.frame(0,(POSITION[0],POSITION[1])),Some(10));assert_eq!(w.frame(1,(POSITION[0],POSITION[1])),None);
 assert_eq!(w.frame(0,(0,0)),None);assert!(core::mem::size_of::<World>()<=8);
}'''
   (p/'main.rs').write_text(harness)
   deps=ROOT/'shared/hk-sim/target/debug/deps';args=['rustc','--edition=2021','-Awarnings',str(p/'main.rs'),'-o',str(p/'run')]
   for name in ('hk_sim','hk_format'):
    lib=max(deps.glob('lib'+name+'-*.rlib'),key=lambda f:f.stat().st_mtime);args+=['--extern',name+'='+str(lib)]
   args+=['-L','dependency='+str(deps)]
   result=subprocess.run(args,env=dict(os.environ,CARGO_MANIFEST_DIR=str(p/'game')),capture_output=True,text=True)
   self.assertEqual(result.returncode,0,result.stderr)
   result=subprocess.run([str(p/'run')],capture_output=True,text=True);self.assertEqual(result.returncode,0,result.stderr)

if __name__=='__main__':unittest.main()
