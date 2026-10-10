#[path="../game/src/hud_state.rs"]mod hud;
extern crate self as psx_gpu;
extern crate self as psx_vram;
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
 pub struct QuadTextured;
 impl QuadTextured{pub fn with_material(_:[(i16,i16);4],_:[(u8,u8);4],_:TextureMaterial)->Self{Self}}
}
pub mod ot {
 use super::prim::Sprite;
 pub struct OrderingTable<const N:usize>{pub sprites:Vec<Sprite>}
 impl<const N:usize> OrderingTable<N>{pub fn add(&mut self,_:usize,p:&mut Sprite,_:usize){self.sprites.push(*p);}}
}
mod display{pub unsafe fn ot_add<const N:usize>(ot:&mut crate::ot::OrderingTable<N>,z:usize,p:&mut crate::prim::Sprite,w:usize){ot.add(z,p,w);}}
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
 lifeblood::upload(BOOT);assert!(!UPLOADS.lock().unwrap().is_empty());
 let mut ot=ot::OrderingTable::<1>{sprites:Vec::new()};lifeblood::append_hud(&mut ot,5,0);assert!(ot.sprites.is_empty());
 lifeblood::append_hud(&mut ot,5,2);assert_eq!(ot.sprites.iter().map(|s|s.x).collect::<Vec<_>>(),vec![152,136]);
 let art=lifeblood::LIFE_FRAMES[lifeblood::BLUE_HUD].parts[0];
 for sprite in &ot.sprites {assert_eq!((sprite.y,sprite.uv,sprite.clut),(14,(art.u,art.v),art.clut));assert!(sprite.w<=14&&sprite.h<=14);}
 ot.sprites.clear();lifeblood::append_hud(&mut ot,9,22);assert_eq!(ot.sprites.len(),22);
 for(i,sprite) in ot.sprites.iter().enumerate(){let(x,y)=hud::mask_position(9+21-i as u16);assert_eq!((sprite.x,sprite.y),(x,y));assert!(x+i16::from(art.w)<=320&&y+i16::from(art.h)<hud::geo_y(31));}
 let mut life=lifeblood::World::new();let origin=lifeblood::LIFE_SPEC.origin;
 assert_eq!(lifeblood::draw(&life,0,(origin[0],origin[1])),5);
 let b=lifeblood::LIFE_SPEC.bounds;let poly=[[b[0],b[1]],[b[2],b[1]],[b[2],b[3]],[b[0],b[3]]];
 assert!(life.strike(0,&poly).opened);assert_eq!(life.bugs.iter().flatten().count(),2);
 assert!(lifeblood::draw(&life,0,(origin[0],origin[1]))>=4);
 println!("actual Lifeblood uploads, source blue HUD and cocoon/bug draw paths passed");
}
