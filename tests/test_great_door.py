import copy,json,os,subprocess,sys,tempfile,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from great_door import bind,generate,horizontal_entry_ray

def entry():
 return dict(spawn=[.5,44.400625],settle_ticks=10,fade_delay_ticks=7,fade_ticks=30,lead_ticks=12,walk_ticks=28,speed=8.3)

def record():
 return dict(stage_hits=[4,8,13],cooldown_ticks=9,transition_ticks=2,entry_delay_ticks=150,
             target_scene='Town',position=[190.48,68.42,.61],scale=59172,
             bounds=[189.66,61.72,190.63,76.09],frames=[10,11,12],edges=[],collider_source='level6:8218')

class GreatDoorTests(unittest.TestCase):
 def test_horizontal_gate_ground_rays_are_mirrored_and_ignore_vertical_offset(self):
  self.assertEqual(horizontal_entry_ray([1.5,49],{'x':0,'y':99},'left1'),(.5,[2.5,49]))
  self.assertEqual(horizontal_entry_ray([190.5,68],{'x':0,'y':99},'right1'),(191.5,[189.5,68]))
  self.assertEqual(horizontal_entry_ray([20,5],{'x':2,'y':-4},'right2'),(23,[21,5]))
  with self.assertRaises(ValueError):horizontal_entry_ray([20,5],{'x':0,'y':0},'top1')
 def test_generated_return_entry_has_its_own_source_spawn_and_waits(self):
  metadata={'scenes':[{'scene_id':0,'scene_name':'Tutorial_01'},{'scene_id':1,'scene_name':'Town'}],'regions':[{'great_door':record()}]}
  back=entry();back['spawn']=[193.5,63.400625];back['walk_ticks']=28
  generated=generate(metadata,entry(),back)
  self.assertIn('pub const RETURN_ENTRY:Entry=Entry{spawn:[12681216,4155023]',generated)

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
   (p/'data/great_door.rs').write_text(generate(metadata,entry(),entry()))
   harness='''mod world {pub struct State;impl State {pub fn append_script_edges(&mut self,_:&[u16]){}}}
mod render {pub fn texture(_:usize,_:[(i16,i16);4],_:(u8,u8,u8)){}}
#[path="'''+str(ROOT/'game/src/great_door.rs')+'''"]mod great_door;
fn main() {
 use great_door::*;let mut w=World::new();
 let b=BOUNDS;let polygon=[[b[0],b[1]],[b[2],b[1]],[b[2],b[3]],[b[0],b[3]]];
 assert!(!w.strike(1,&polygon).hit);assert!(!w.strike(0,&[[0,0],[1,0],[0,1]]).hit);
 for hit in 1..=13 {
  let s=w.strike(0,&polygon);assert!(s.hit);assert_eq!(s.opened,hit==13);
  if hit==13 {break;}
  for _ in 0..8 {assert!(!w.strike(0,&polygon).hit);assert!(!w.tick(0));}
  assert!(!w.strike(0,&polygon).hit);assert!(!w.tick(0));
  unsafe {assert_eq!(HK_GREAT_DOOR_HITS,hit);assert_eq!(HK_GREAT_DOOR_STAGE,if hit>=8{2}else if hit>=4{1}else{0});}
 }
 assert!(w.opened());assert!(w.pending());assert_eq!(w.frame(0,(POSITION[0],POSITION[1])),None);
 assert!(w.blackout());assert!(!w.entering());assert!(!w.tick(0));
 assert!(w.pending());assert!(w.tick(0));assert!(w.pending());assert!(!w.tick(0));
 assert!(w.blackout());w.begin_entry(0);assert!(!w.entering());
 for _ in 0..400 {assert!(!w.tick(TARGET_SCENE));assert!(w.pending());assert!(w.blackout());}
 w.begin_entry(TARGET_SCENE);assert!(w.entering());assert!(w.pending());
 for _ in 0..200 {assert!(!w.tick(0));} // Origin ticks cannot consume destination entry.
 unsafe {assert_eq!(HK_GREAT_DOOR_ENTRY_WAIT,200);}
 let mut player=hk_sim::Player::spawn(ENTRY.spawn[0],ENTRY.spawn[1]);
 let mut walks=0;let mut previous_shade=255;
 for elapsed in 1..200 {
  assert!(!w.tick(TARGET_SCENE));assert!(w.pending());
  assert!(w.shade()<=previous_shade);previous_shade=w.shade();
  if elapsed<=17 {assert_eq!(w.shade(),255);}
  if elapsed>=47 {assert_eq!(w.shade(),0);}
  if w.forced_direction()!=0 {walks+=1;}
  assert!(w.apply_entry(&mut player));assert_eq!(player.y,ENTRY.spawn[1]);
  assert_eq!(player.x,ENTRY.spawn[0]+walks*(ENTRY.speed/60));
 }
 assert_eq!(walks,28);assert_eq!(player.animation,1);
 assert!(!w.tick(TARGET_SCENE));assert!(!w.pending());assert!(!w.apply_entry(&mut player));
 w.begin_entry(TARGET_SCENE);assert!(!w.pending()); // One entry per reset.
 assert!(!w.strike(0,&polygon).hit);unsafe {assert_eq!(HK_GREAT_DOOR_TRANSITIONS,1);}
 // Reverse traversal has a source right-side walk, no Great Door150-tick delay.
 w.begin_gate_entry(0,-1);let total=RETURN_ENTRY.settle+RETURN_ENTRY.lead+RETURN_ENTRY.walk;
 let mut player=hk_sim::Player::spawn(RETURN_ENTRY.spawn[0],RETURN_ENTRY.spawn[1]);
 for _ in 0..400 {assert!(!w.tick(TARGET_SCENE));}
 unsafe {assert_eq!(HK_GREAT_DOOR_ENTRY_WAIT,total as u32);}
 let mut walks=0;let mut previous_shade=255;
 for _ in 1..total {
  assert!(!w.tick(0));assert!(w.entering());assert!(w.shade()<=previous_shade);previous_shade=w.shade();
  if w.forced_direction()!=0 {walks+=1;assert_eq!(w.forced_direction(),-1);}
  assert!(w.apply_entry(&mut player));assert_eq!(player.facing,-1);assert_eq!(player.y,RETURN_ENTRY.spawn[1]);
  assert_eq!(player.x,RETURN_ENTRY.spawn[0]-walks*(RETURN_ENTRY.speed/60));
 }
 assert_eq!(walks,RETURN_ENTRY.walk as i32);assert_eq!(player.animation,1);
 assert!(!w.tick(0));assert!(!w.pending());assert!(!w.apply_entry(&mut player));
 // Later forward entries are repeatable, but do not repeat the initial delay.
 w.begin_gate_entry(TARGET_SCENE,1);unsafe {assert_eq!(HK_GREAT_DOOR_ENTRY_WAIT,(ENTRY.settle+ENTRY.lead+ENTRY.walk)as u32);}
 for _ in 0..ENTRY.settle+ENTRY.lead+ENTRY.walk {assert!(!w.tick(TARGET_SCENE));}
 assert!(!w.pending());w.begin_gate_entry(0,-1);assert!(w.pending());

 w.reset();assert!(!w.opened());assert_eq!(w.frame(0,(POSITION[0],POSITION[1])),Some(10));assert_eq!(w.frame(1,(POSITION[0],POSITION[1])),None);
 assert_eq!(w.frame(0,(0,0)),None);assert!(core::mem::size_of::<World>()<=16);
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
