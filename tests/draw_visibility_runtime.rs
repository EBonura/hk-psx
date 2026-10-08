#![allow(dead_code,static_mut_refs)]
const MAX_DRAWS:usize=96;
// The renderer's per-draw state: the cooked record (read in place in the game)
// plus the mutable flags and colours draw_visibility keeps.
#[derive(Clone,Copy)]struct Draw {texture:u16,tint:[u8;3]}
impl Draw {fn texture(&self)->usize{self.texture as usize}fn tint(&self)->(u8,u8,u8){(self.tint[0],self.tint[1],self.tint[2])}}
const BLACK_AVERAGE:u8=4;
#[path="../game/src/scenery_color.rs"]mod scenery_color;
static mut DRAW_COUNT:usize=MAX_DRAWS;
static mut RECORDS:[Draw;MAX_DRAWS]=[Draw{texture:0,tint:[0;3]};MAX_DRAWS];
static mut DRAW_FLAGS:[u8;MAX_DRAWS]=[64;MAX_DRAWS];
static mut DRAW_COLORS:[u32;MAX_DRAWS]=[0;MAX_DRAWS];
unsafe fn draw_record(i:usize)->&'static Draw {unsafe {&RECORDS[i]}}
static mut VISIBLE:[u32;MAX_DRAWS/32]=[0;MAX_DRAWS/32];
static mut GAINS:[u8;MAX_DRAWS]=[0;MAX_DRAWS];
static mut OPACITIES:[u8;MAX_DRAWS]=[0;MAX_DRAWS];
static mut BLACK_MASKS:[u32;2]=[u32::MAX;2];
static mut TILE_ELIGIBILITY_DIRTY:bool=false;
static mut TILE_GENERATION:u32=u32::MAX-4;
struct Prepared {invalidations:u32}
impl Prepared {fn invalidate(&mut self){self.invalidations=self.invalidations.wrapping_add(1);}}
static mut TILE_PREPARED:Prepared=Prepared{invalidations:0};
fn certified(draw:usize)->bool{draw%3!=0}
fn tile_eligibility_changed(draw:usize){unsafe{if certified(draw){TILE_ELIGIBILITY_DIRTY=true;TILE_GENERATION=TILE_GENERATION.wrapping_add(1);TILE_PREPARED.invalidate();}}}
fn vignette_recolor(_draw:usize){}
#[path="../game/src/draw_visibility.rs"]mod live;
struct Original {visible:[bool;MAX_DRAWS],gain:[u8;MAX_DRAWS],opacity:[u8;MAX_DRAWS],dirty:bool,generation:u32,invalidations:u32}
impl Original {
 fn new()->Self{Self{visible:[true;MAX_DRAWS],gain:[128;MAX_DRAWS],opacity:[128;MAX_DRAWS],dirty:false,generation:u32::MAX-4,invalidations:0}}
 fn changed(&mut self,i:usize){if certified(i){self.dirty=true;self.generation=self.generation.wrapping_add(1);self.invalidations=self.invalidations.wrapping_add(1);}}
 fn set(&mut self,i:usize,kind:u8,v:u8){match kind{0=>{if self.visible[i]!=(v!=0){self.changed(i);}self.visible[i]=v!=0},1=>{let v=v.min(128);if(self.gain[i]==0)!=(v==0){self.changed(i);}self.gain[i]=v},_=>{let v=v.min(128);if(self.opacity[i]==128)!=(v==128){self.changed(i);}self.opacity[i]=v}}}
 fn reset(&mut self){if self.dirty{self.generation=self.generation.wrapping_add(1);self.invalidations=self.invalidations.wrapping_add(1);self.dirty=false;}self.visible=[true;MAX_DRAWS];self.gain=[128;MAX_DRAWS];self.opacity=[128;MAX_DRAWS];}
 fn check(&self){unsafe{
  let mut words=[0u32;64];if let Some(count)=live::sparse_snapshot(&mut words){
   assert!(live::matches_snapshot(&words[..count]));
   let mut visible=[true;MAX_DRAWS];let mut gain=[128;MAX_DRAWS];let mut opacity=[128;MAX_DRAWS];
   let mut seen=[false;MAX_DRAWS];
   for &word in &words[..count]{let draw=(word>>17)as usize;assert!(!seen[draw]);seen[draw]=true;
    visible[draw]=word&(1<<16)!=0;gain[draw]=((word>>8)&255)as u8;opacity[draw]=(word&255)as u8;}
   assert_eq!(visible,self.visible);assert_eq!(gain,self.gain);assert_eq!(opacity,self.opacity);
   if count!=0{words[0]^=1;assert!(!live::matches_snapshot(&words[..count]));}
  }else{assert!(!live::matches_snapshot(&words));}

  for i in 0..DRAW_COUNT {assert_eq!(VISIBLE[i/32]&(1<<(i%32))!=0,self.visible[i]);assert_eq!(GAINS[i],self.gain[i]);assert_eq!(OPACITIES[i],self.opacity[i]);assert_eq!(DRAW_FLAGS[i]&64!=0,self.visible[i]&&self.gain[i]!=0&&self.opacity[i]!=0,"draw{i}");assert_eq!(DRAW_FLAGS[i]&63,(i%64)as u8);
   let g=self.gain[i]as u16;let t=RECORDS[i].tint();
   let (r,b,c)=((t.0 as u16*g/128)as u8,(t.1 as u16*g/128)as u8,(t.2 as u16*g/128)as u8);
   let mut expected=r as u32|((b as u32)<<8)|((c as u32)<<16);
   if DRAW_FLAGS[i]&BLACK_AVERAGE!=0 {expected=(expected&!255)|127;}
   if self.opacity[i]<128 {let a=self.opacity[i]as u32;expected=a|(a<<8)|(a<<16);}
   assert_eq!(DRAW_COLORS[i],expected,"draw{i} cached color");}
  assert_eq!(TILE_GENERATION,self.generation);assert_eq!(TILE_ELIGIBILITY_DIRTY,self.dirty);assert_eq!(TILE_PREPARED.invalidations,self.invalidations);
 }}
}
fn set(old:&mut Original,i:usize,kind:u8,v:u8){old.set(i,kind,v);match kind{0=>live::set_visible(i,v!=0),1=>live::set_gain(i,v),_=>live::set_opacity(i,v)}old.check();}
fn reset(old:&mut Original){old.reset();live::reset_visibility();old.check();unsafe{for f in &DRAW_FLAGS[..DRAW_COUNT]{assert_eq!(f&128,0);}}}
#[test]
fn exhaustive_setter_order_sparse_reset_and_region_rebinding_match_original(){unsafe{
 live::begin_region();for i in 0..MAX_DRAWS {DRAW_FLAGS[i]=64|(i%64)as u8;
 RECORDS[i].tint=[(i*71)as u8,(i*93)as u8,(i*37)as u8];
 DRAW_COLORS[i]=scenery_color::command(0,RECORDS[i].tint(),128,128|if DRAW_FLAGS[i]&4!=0 {256}else{0});}let mut old=Original::new();old.check();
 for i in[0,31,32,63]{for visible in[0,1]{for gain in[0,1,127,128,255]{for opacity in[0,1,127,128,255]{for order in[[0,1,2],[0,2,1],[1,0,2],[1,2,0],[2,0,1],[2,1,0]]{
  reset(&mut old);let values=[visible,gain,opacity];for kind in order{set(&mut old,i,kind,values[kind as usize]);}for kind in order{set(&mut old,i,kind,values[kind as usize]);}
 }}}}}
 let mut r=0x24681357u32;
 for n in 0..50000 {r=r.wrapping_mul(1664525).wrapping_add(1013904223);let i=(r as usize>>8)%MAX_DRAWS;let kind=(r%3)as u8;let v=if n%9==0{0}else{(r>>16)as u8};set(&mut old,i,kind,v);if n%73==0{reset(&mut old);}}
 // Fill every list slot, repeatedly mutate each entry, then restore defaults.
 reset(&mut old);for i in 0..MAX_DRAWS{set(&mut old,i,0,0);set(&mut old,i,1,0);set(&mut old,i,2,0);}reset(&mut old);
 // A new region can shrink then grow its local index space; stale flags and
 // touched indices from the previous room must never survive rebinding.
 for count in[3,64,1,33,64]{set(&mut old,DRAW_COUNT-1,0,0);DRAW_COUNT=count;TILE_ELIGIBILITY_DIRTY=false;old.dirty=false;live::begin_region();old.visible=[true;MAX_DRAWS];old.gain=[128;MAX_DRAWS];old.opacity=[128;MAX_DRAWS];for i in 0..count{DRAW_FLAGS[i]=64|(i%64)as u8;
 RECORDS[i].tint=[(i*71)as u8,(i*93)as u8,(i*37)as u8];
 DRAW_COLORS[i]=scenery_color::command(0,RECORDS[i].tint(),128,128|if DRAW_FLAGS[i]&4!=0 {256}else{0});}reset(&mut old);}
}}
