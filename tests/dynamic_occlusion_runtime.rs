#[path="../game/src/scenery_occlusion.rs"]mod occlusion;
#[path="../game/src/occlusion_scratch.rs"]mod occlusion_scratch;
use occlusion_scratch::{dynamic_hidden,Occluder};
fn quad(r:[i16;4])->[(i16,i16);4] {[(r[0],r[1]),(r[2],r[1]),(r[0],r[3]),(r[2],r[3])]}
fn front(rect:[i16;4])->Occluder {Occluder{draw:0,rect,front:true}}
#[test]fn exact_edges_and_one_pixel_escape() {
 let cover=front([10,20,30,40]);assert!(dynamic_hidden(quad(cover.rect),&[cover]));
 for r in [[9,20,30,40],[10,19,30,40],[10,20,31,40],[10,20,30,41]] {assert!(!dynamic_hidden(quad(r),&[cover]));}
}
#[test]fn reflections_and_rotations_use_every_vertex() {
 let cover=front([10,20,30,40]);let v=[(20,20),(30,30),(10,30),(20,40)];
 for permutation in [[0,1,2,3],[1,0,3,2],[2,3,0,1],[3,2,1,0]] {assert!(dynamic_hidden(permutation.map(|i|v[i]),&[cover]));}
 let mut escaped=v;escaped[3].1+=1;assert!(!dynamic_hidden(escaped,&[cover]));
}
#[test]fn clips_only_to_actual_viewport() {
 let cover=front([0,0,320,240]);assert!(dynamic_hidden(quad([-20,-30,340,260]),&[cover]));
 assert!(!dynamic_hidden(quad([-20,-30,340,260]),&[front([1,0,320,240])]));
}
#[test]fn degenerate_and_offscreen_fall_back() {
 let cover=front([0,0,320,240]);
 for r in [[10,10,10,20],[10,10,20,10],[-20,10,0,20],[320,10,330,20],[10,-20,20,0],[10,240,20,250]] {assert!(!dynamic_hidden(quad(r),&[cover]));}
}
#[test]fn signed_eleven_bit_boundaries_and_wrapping_fall_back() {
 let cover=front([0,0,320,240]);assert!(dynamic_hidden(quad([-1024,-1024,1023,1023]),&[cover]));
 for r in [[-1025,0,20,20],[0,-1025,20,20],[0,0,1024,20],[0,0,20,1024],[i16::MIN,0,20,20],[0,0,i16::MAX,20]] {assert!(!dynamic_hidden(quad(r),&[cover]));}
}
#[test]fn back_masks_never_hide_dynamic_layer() {
 let mut cover=front([0,0,320,240]);cover.front=false;cover.draw=u16::MAX;
 assert!(!dynamic_hidden(quad([10,10,20,20]),&[cover]));cover.front=true;cover.draw=0;assert!(dynamic_hidden(quad([10,10,20,20]),&[cover]));
}
#[test]fn adjacent_rectangles_do_not_trigger_union_work() {
 assert!(!dynamic_hidden(quad([0,0,20,20]),&[front([0,0,10,20]),front([10,0,20,20])]));
 assert!(!dynamic_hidden(quad([0,0,20,20]),&[]));
}
