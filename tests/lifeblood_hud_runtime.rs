#[path="../game/src/hud_state.rs"]mod hud;
extern crate self as psx_gpu;
extern crate self as psx_vram;
extern crate self as psx_rt;
pub mod interrupts{pub static NOW:std::sync::atomic::AtomicU32=std::sync::atomic::AtomicU32::new(0);pub fn vblank_count()->u32{NOW.load(std::sync::atomic::Ordering::Relaxed)}}
use std::sync::Mutex;
pub const KNIGHT_SCALE:i32=60693;
pub mod material {
 #[derive(Clone,Copy,Debug)]pub enum BlendMode{Average}
 #[derive(Clone,Copy,Debug)]pub struct TextureMaterial{pub clut:u16,pub tpage:u16}
 impl TextureMaterial {pub fn blended(clut:u16,tpage:u16,_:(u8,u8,u8),_:BlendMode)->Self{Self{clut,tpage}}}
}
pub mod prim {
 use super::material::TextureMaterial;
 #[derive(Clone,Copy,Debug)]pub struct Sprite{pub x:i16,pub y:i16,pub w:u16,pub h:u16,pub uv:(u8,u8),pub clut:u16}
 impl Sprite {
  pub const WORDS:usize=4;
  pub const fn new(x:i16,y:i16,w:u16,h:u16,uv:(u8,u8),clut:u16,_:u8,_:u8,_:u8)->Self{Self{x,y,w,h,uv,clut}}
  pub fn with_material(x:i16,y:i16,w:u16,h:u16,uv:(u8,u8),mat:TextureMaterial)->Self{Self{x,y,w,h,uv,clut:mat.clut}}
 }
 pub trait Quadlike{fn as_quad(&self)->QuadTextured;}
 impl Quadlike for QuadTextured{fn as_quad(&self)->QuadTextured{*self}}
 impl Quadlike for Sprite{fn as_quad(&self)->QuadTextured{let(x1,y1)=(self.x+self.w as i16,self.y+self.h as i16);QuadTextured{v:[(self.x,self.y),(x1,self.y),(self.x,y1),(x1,y1)],uv:[self.uv;4],clut:self.clut}}}
 #[derive(Clone,Copy,Debug)]pub struct QuadTextured{pub v:[(i16,i16);4],pub uv:[(u8,u8);4],pub clut:u16}
 impl QuadTextured{
  pub const WORDS:usize=9;
  pub const fn new(v:[(i16,i16);4],uv:[(u8,u8);4],clut:u16,_:u16,_:(u8,u8,u8))->Self{Self{v,uv,clut}}
  pub fn with_material(v:[(i16,i16);4],uv:[(u8,u8);4],mat:TextureMaterial)->Self{Self{v,uv,clut:mat.clut}}
  pub fn x(&self)->i16{self.v.iter().map(|p|p.0).min().unwrap()}
  pub fn y(&self)->i16{self.v.iter().map(|p|p.1).min().unwrap()}
  pub fn w(&self)->i16{self.v.iter().map(|p|p.0).max().unwrap()-self.x()}
  pub fn h(&self)->i16{self.v.iter().map(|p|p.1).max().unwrap()-self.y()}
 }
}
pub mod ot {
 use super::prim::{QuadTextured,Quadlike};
 pub struct OrderingTable<const N:usize>{pub sprites:Vec<QuadTextured>}
 impl<const N:usize> OrderingTable<N>{pub fn add<P:Quadlike>(&mut self,_:usize,p:&mut P,_:usize){self.sprites.push(p.as_quad());}}
}
mod display{pub unsafe fn ot_add<const N:usize,P:crate::prim::Quadlike>(ot:&mut crate::ot::OrderingTable<N>,z:usize,p:&mut P,w:usize){ot.add(z,p,w);}}
pub struct VramRect{pub x:u16,pub y:u16,pub w:u16,pub h:u16}
impl VramRect{pub fn new(x:u16,y:u16,w:u16,h:u16)->Self{Self{x,y,w,h}}}
static UPLOADS:Mutex<Vec<(u16,u16,u16,u16)>>=Mutex::new(Vec::new());
pub fn upload_bytes(rect:VramRect,bytes:&[u8]){assert_eq!(bytes.len(),rect.w as usize*rect.h as usize*2);UPLOADS.lock().unwrap().push((rect.x,rect.y,rect.w,rect.h));}
mod input{pub fn checkpoint(){}}
mod world{
 pub fn scene_of(_:usize)->usize {0}
 pub struct State{pub edges:Vec<u16>}
 impl State{pub fn set_lifeblood_edges(&mut self,edges:&[u16]){self.edges=edges.to_vec();}}
}
mod render {
 pub fn set_visible(_:usize,_:bool){}
 pub fn resident_quad(_:&crate::prim::QuadTextured,points:[(i16,i16);4]){assert!(points.iter().all(|p|p.0.abs()<1000&&p.1.abs()<1000));}
}
#[path="../game/src/lifeblood.rs"]mod lifeblood;
/// The boot art chunk carries the cocoon art at `LIFE_ART`; this harness
/// hands the real bank over as a chunk of its own.
mod boot_art{pub const LIFE_ART:(usize,usize)=(0,include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/lifeblood.hk")).len());}
static BOOT:&[u8]=include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/lifeblood.hk"));
fn main(){
 use std::sync::atomic::Ordering::Relaxed;
 lifeblood::upload(BOOT);assert!(!UPLOADS.lock().unwrap().is_empty());
 let mut ot=ot::OrderingTable::<1>{sprites:Vec::new()};lifeblood::append_hud(&mut ot,5,0);assert!(ot.sprites.is_empty());
 // A new mask grows in (Blue Appear, 27 vblanks): small on its first frame, whole once it has finished.
 lifeblood::append_hud(&mut ot,5,2);
 let art=lifeblood::LIFE_FRAMES[lifeblood::BLUE_HUD].parts[0];
 assert!(ot.sprites.iter().all(|q|q.w()<i16::from(art.w)/2&&q.h()<i16::from(art.h)/2),"the appearing mask starts small");
 interrupts::NOW.store(1000,Relaxed);
 ot.sprites.clear();lifeblood::append_hud(&mut ot,5,2);assert_eq!(ot.sprites.iter().map(|s|s.x()).collect::<Vec<_>>(),vec![152,136]);
 for sprite in &ot.sprites {assert_eq!((sprite.y(),sprite.uv[0],sprite.clut),(14,(art.u,art.v),art.clut));assert!(sprite.w()<=14&&sprite.h()<=14);assert_eq!((sprite.w(),sprite.h()),(i16::from(art.w),i16::from(art.h)));}
 // A lost mask swells and goes dark (Blue Break, 15 vblanks) where it was, then is gone.
 ot.sprites.clear();lifeblood::append_hud(&mut ot,5,1);assert_eq!(ot.sprites.len(),2,"one mask and the breaking one");
 interrupts::NOW.store(1010,Relaxed);
 ot.sprites.clear();lifeblood::append_hud(&mut ot,5,1);assert_eq!(ot.sprites.len(),2);
 assert!(ot.sprites.iter().any(|q|q.w()>i16::from(art.w)),"the breaking mask is bigger than a whole one");
 interrupts::NOW.store(1020,Relaxed);
 ot.sprites.clear();lifeblood::append_hud(&mut ot,5,1);assert_eq!(ot.sprites.len(),1,"the break is over");
 interrupts::NOW.store(2000,Relaxed);
 ot.sprites.clear();lifeblood::append_hud(&mut ot,9,22);
 interrupts::NOW.store(3000,Relaxed);
 ot.sprites.clear();lifeblood::append_hud(&mut ot,9,22);assert_eq!(ot.sprites.len(),22);
 for(i,sprite) in ot.sprites.iter().enumerate(){let(x,y)=hud::mask_position(9+21-i as u16);assert_eq!((sprite.x(),sprite.y()),(x,y));assert!(x+i16::from(art.w)<=320&&y+i16::from(art.h)<hud::geo_y(31));}
 let mut life=lifeblood::World::new();let origin=lifeblood::LIFE_SPEC.origin;
 assert_eq!(lifeblood::draw(&life,0,(origin[0],origin[1])),5);
 let b=lifeblood::LIFE_SPEC.bounds;let poly=[[b[0],b[1]],[b[2],b[1]],[b[2],b[3]],[b[0],b[3]]];
 assert!(life.strike(0,&poly).opened);assert_eq!(life.bugs.iter().flatten().count(),2);
 assert!(lifeblood::draw(&life,0,(origin[0],origin[1]))>=4);
 println!("actual Lifeblood uploads, source blue HUD and cocoon/bug draw paths passed");
}
