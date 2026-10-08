#[path="../../../game/src/alpha_scissor_cache.rs"]mod oracle;
#[path="../../../game/src/core_map_cache.rs"]mod cache;
use cache::Cache;
#[test]
fn cached_full_local_cores_match_oracle_across_shapes_reflections_and_translations(){let mut c=Cache::new();let mut seed=123u32;let mut next=||{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed};let mut checks=0;
for i in 0..100000 {
 let w=1+(next()%256)as u16;let h=1+(next()%256)as u16;let dx=1+(next()%1023)as i32;let dy=1+(next()%511)as i32;
 let cx=(next()%w as u32)as u8;let cy=(next()%h as u32)as u8;let cw=(1+next()%((w-cx as u16).min(255))as u32)as u8;let ch=(1+next()%((h-cy as u16).min(255))as u32)as u8;
 let core=[cx,cy,cw,ch];
 for reflect in 0..4 {let sx=if reflect&1!=0{-dx}else{dx};let sy=if reflect&2!=0{-dy}else{dy};
  for translation in 0..3 {let x=if translation==0{-1100+(next()%1600)as i32}else{150-translation*13};let y=if translation==0{-600+(next()%1000)as i32}else{120-translation*19};let v=[(x as i16,y as i16),((x+sx)as i16,y as i16),(x as i16,(y+sy)as i16),((x+sx)as i16,(y+sy)as i16)];assert_eq!(c.map(v,w,h,core),oracle::texel_rect_to_screen(v,w,h,core),"case{i} reflection{reflect} shift{translation}");checks+=1;}
 }
}
// Metadata and geometry rejection must never be bypassed by a cached hit.
let v=[(0,0),(100,0),(0,100),(100,100)];for(w,h,core)in[(0,48,[0,0,4,4]),(257,48,[0,0,4,4]),(48,0,[0,0,4,4]),(48,257,[0,0,4,4]),(48,48,[0,0,0,4]),(48,48,[47,47,2,2])] {assert_eq!(c.map(v,w,h,core),oracle::texel_rect_to_screen(v,w,h,core));}
for v in [[(0,0),(100,1),(0,100),(100,100)],[(0,0),(0,0),(0,100),(0,100)],[(0,0),(1024,0),(0,100),(1024,100)],[(0,0),(100,0),(0,512),(100,512)]]{assert_eq!(c.map(v,48,48,[0,0,20,20]),oracle::texel_rect_to_screen(v,48,48,[0,0,20,20]));}
assert_eq!(checks,1200000);assert_eq!(core::mem::size_of::<Cache>(),1280);}
#[test]
fn complete_keys_survive_texture_bank_changes_without_a_lease_reset(){
    let mut c=Cache::new();let v=[(-220,-120),(500,-120),(-220,360),(500,360)];
    let cases=[(48,48,[4,11,23,32]),(32,24,[4,11,23,12]),(48,48,[4,11,24,32]),(48,48,[5,10,23,32]),(256,256,[0,0,255,255])];
    for _ in 0..32 {for(w,h,core)in cases {assert_eq!(c.map(v,w,h,core),oracle::texel_rect_to_screen(v,w,h,core));}}
}
#[test]
fn alpha_secondary_layout_keeps_every_copy_four_byte_aligned_without_growth(){
    assert_eq!(core::mem::size_of::<oracle::Entry>(),40);assert_eq!(core::mem::align_of::<oracle::Entry>(),4);
    assert_eq!(core::mem::size_of::<oracle::Secondary>(),5184);assert_eq!(core::mem::align_of::<oracle::Secondary>(),4);
    let entries=[oracle::Entry::EMPTY;4];for e in &entries {assert_eq!(e as *const _ as usize&3,0);}
}
