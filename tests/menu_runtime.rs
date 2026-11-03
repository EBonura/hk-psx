//! Native hardware-call contract for the actual title implementation.
extern crate self as psx_gpu;
extern crate self as psx_pad;
extern crate self as psx_vram;
extern crate self as psx_rt;
extern crate self as psx_io;
// `psx_io::gpu::write_command`: the texture cache clear after a style's upload.
pub mod gpu{pub fn write_command(_:u32){}}
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Mutex;
static LOCK:Mutex<()>=Mutex::new(());
#[derive(Default)]struct Trace{samples:VecDeque<u16>,polls:usize,pending:bool,armed:bool,signalled:bool,uploads:Vec<(VramRect,usize)>,volumes:Vec<(u8,u8)>,sprites:usize,music_begins:usize,unlocks:Vec<u8>,style_reads:usize,quads:usize}
thread_local!{static TRACE:RefCell<Trace>=RefCell::new(Trace::default());}
pub mod framebuf{
 pub struct FrameBuffer;
 impl FrameBuffer{pub fn begin_deferred_swap(&mut self)->u32{0}pub fn apply_draw_target(&mut self){}}
}
pub mod material{
 pub struct TextureMaterial;
 #[derive(Clone,Copy)]pub enum BlendMode{Opaque,Add,Subtract}
 impl TextureMaterial{pub fn opaque(_:u16,_:u16,tint:(u8,u8,u8))->Self{assert!(tint.0<=128&&tint.1<=128&&tint.2<=128);Self}
  pub fn blended(_:u16,_:u16,tint:(u8,u8,u8),_:BlendMode)->Self{assert!(tint.0<=128&&tint.1<=128&&tint.2<=128);Self}}
}
#[derive(Clone,Copy,Debug)]pub struct VramRect{pub x:u16,pub y:u16,pub w:u16,pub h:u16}
impl VramRect{pub fn new(x:u16,y:u16,w:u16,h:u16)->Self{assert!(x+w<=1024&&y+h<=512);Self{x,y,w,h}}}
pub struct Clut;impl Clut{pub fn new(x:u16,y:u16)->Self{assert!(x%16==0&&y<512);Self}pub fn uv_word(&self)->u16{0}}
pub enum TextureDepth{Bit4,Bit8}pub struct TexturePage;impl TexturePage{pub fn new(x:u16,y:u16,_:TextureDepth)->Self{assert!(x%64==0&&y%256==0);Self}pub fn uv_word(&self,_:u16)->u16{0}}
pub fn upload_bytes(rect:VramRect,data:&[u8]){assert_eq!(rect.w as usize*rect.h as usize*2,data.len());TRACE.with(|t|t.borrow_mut().uploads.push((rect,data.len())));}
pub fn draw_sprite_material(x:i16,y:i16,w:u16,h:u16,_:(u8,u8),_:material::TextureMaterial){assert!(x>=0&&y>=0&&x as u16+w<=320&&y as u16+h<=240);TRACE.with(|t|t.borrow_mut().sprites+=1);}
pub fn draw_quad_textured_material(_:[(i16,i16);4],_:[(u8,u8);4],_:material::TextureMaterial){TRACE.with(|t|t.borrow_mut().quads+=1);}
pub fn draw_rect_flat(x:i16,y:i16,w:u16,h:u16,_:u8,_:u8,_:u8){assert!(x>=0&&y>=0&&x as u16+w<=320&&y as u16+h<=240);}
pub mod button{pub const START:u16=1<<3;pub const CROSS:u16=1<<14;}
pub struct Buttons(u16);impl Buttons{pub fn bits(&self)->u16{self.0}pub fn is_held(&self,b:u16)->bool{self.0&b!=0}}
pub struct Pad{pub buttons:Buttons}
pub fn poll_port1()->Pad{TRACE.with(|t|{let mut t=t.borrow_mut();t.polls+=1;Pad{buttons:Buttons(t.samples.pop_front().expect("menu polled past bounded test input"))}})}
// The title polls through the program-wide pad reader (game/src/input.rs); the trace is its hardware.
mod input{pub fn poll_bits()->u16{super::poll_port1().buttons.bits()}}
// GPUSTAT bit 24: psx-rt applies a queued flip only once a GP0(1Fh) sent
// after the last arm has raised it, so every present must arm, then signal.
pub fn arm_draw_done(){TRACE.with(|t|{let mut t=t.borrow_mut();t.armed=true;t.signalled=false;});}
pub fn signal_draw_done(){TRACE.with(|t|{let mut t=t.borrow_mut();assert!(t.armed,"GP0(1Fh) without an arm");t.armed=false;t.signalled=true;});}
mod presentation{pub const FLIP_TIMEOUT_VBLANKS:u32=30;pub fn force_flip(){panic!("the menu's flip timed out");}}
pub mod interrupts{
 pub fn queue_display_control_at_vblank(_:u32){super::TRACE.with(|t|{let mut t=t.borrow_mut();assert!(t.signalled,"flip queued with no GP0(1Fh) after the arm: it would never apply");t.signalled=false;t.pending=true;});}
 pub fn is_display_control_queued()->bool{super::TRACE.with(|t|t.borrow().pending)}
 pub fn vblank_count()->u32{super::TRACE.with(|t|t.borrow().polls as u32)}
 pub fn wait_vblank(){super::TRACE.with(|t|t.borrow_mut().pending=false);}
}
pub mod audio{pub fn set_volume(v:u8){super::TRACE.with(|t|t.borrow_mut().volumes.push((0,v)));}
 // The menu's cursor and option clips (MenuAudioController); no SPU here.
 pub fn ui_select(){}pub fn ui_slider(){}
 // Confirm/cancel and start, from the world bank.
 pub fn ui_confirm(){}pub fn ui_start(){}}
// display.rs drives the GPU (brightness quad, GP1 display range); here it is a
// record of what the title asked for.
pub mod display{
 use std::cell::RefCell;
 thread_local!{pub static STATE:RefCell<(i8,(i8,i8),Vec<u8>)>=RefCell::new((0,(0,0),Vec::new()));}
 pub fn brightness()->i8{STATE.with(|s|s.borrow().0)}
 pub fn screen()->(i8,i8){STATE.with(|s|s.borrow().1)}
 pub fn set_brightness(v:i8){STATE.with(|s|s.borrow_mut().0=v)}
 pub fn set_screen(x:i8,y:i8){STATE.with(|s|s.borrow_mut().1=(x,y))}
 pub fn draw_direct(gain:u8){STATE.with(|s|s.borrow_mut().2.push(gain))}
}
pub mod ambience{pub fn set_volume(v:u8){super::TRACE.with(|t|t.borrow_mut().volumes.push((1,v)));}}
pub mod music{pub fn begin(){super::TRACE.with(|t|t.borrow_mut().music_begins+=1);}pub fn suspend()->bool{true}pub fn tick(){}pub fn stop()->bool{true}pub fn set_fade(_:u8){}pub fn set_volume(v:u8){super::TRACE.with(|t|t.borrow_mut().volumes.push((2,v)));}}
// cheats composes the equipped charms into the live parameters and reconciles
// the all-charms grant. Neither reaches the title screen, and the real module
// links the cooked catalogue this harness has no manifest directory for, so the
// contract is stubbed here; tests/charms_runtime.rs exercises the real one.
mod charms{
 pub fn vitals(base:hk_sim::VitalParams)->hk_sim::VitalParams{base}
 pub fn grant_all(_:bool){}
 pub fn on_damage(_:&mut hk_sim::Vitals,_:hk_sim::VitalParams){}
 pub fn damage_taken(d:u16)->u16{d}
}
// And the shop's half of PlayerData sits under the charms in the same stack,
// for the same reason: the real module links the cooked stock. Its own
// composition is exercised by tests/shop_runtime.rs.
mod shop{
 pub fn vitals(base:hk_sim::VitalParams)->hk_sim::VitalParams{base}
}
#[path="../game/src/cheats.rs"]mod cheats;
// menu_state indexes the save screen by the profile count.
mod save{pub const PROFILES:usize=4;pub fn unlocked_styles()->u8{super::TRACE.with(|t|t.borrow().unlocks.last().copied().unwrap_or(0))}pub fn unlock_styles(mask:u8){super::TRACE.with(|t|t.borrow_mut().unlocks.push(mask));}}
const EMPTY:[&str;4]=["Empty","Empty","Empty","Empty"];
/// The title art the disc would hand `run` and `restore` (tests/test_menu.py writes it).
static ART:&[u8]=include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/menu.hk"));
// The boot art chunk's layout the title reads: its ornaments follow the title art (89,712 bytes) and the glyph sheet (7,712).
mod boot_art{pub const TITLE_ORNAMENTS:(usize,usize)=(89712+12832,10768);pub const CARD_FONT:(usize,usize)=(89712+12832+10768,7680);
 // The default style's extras follow the card glyphs (512 bytes), then the logos; every other style is read from the disc (the stub below).
 pub const STYLE_ART:[(u32,u32,u32,u32);10]=[(100000,100000+89712,512,0),(100000,100000+89712,512,0),(100000,100000+89712,512,0),(100000,100000+89712,512,0),(100000,100000+89712,512,0),(100000,100000+89712,512,0),(100000,100000+89712,512,0),(100000,100000+89712,512,0),(100000,100000+89712,512,0),(0,89712+12832+10768+7680,512,0)];
 pub const LOGO_ART:(usize,usize)=(89712+12832+10768+7680+512,64);}
// The disc stub: a style's background and an empty extras blob, as `Cache::style_art` hands them.
mod disc{pub fn style_art(style:usize)->Option<&'static [u8]>{assert!(style<9);super::TRACE.with(|t|t.borrow_mut().style_reads+=1);let mut v=super::ART[..89712].to_vec();v.extend_from_slice(b"HKX1\0\0\0\0");v.resize(89712+512,0);Some(Box::leak(v.into_boxed_slice()))}}
#[path="../game/src/menu.rs"]mod menu;
/// Every test starts on the default style, as a fresh boot does (the style is a static of the title).
fn fresh(){unsafe{menu::STYLE=menu::state::DEFAULT_STYLE;}}
fn feed(bits:impl IntoIterator<Item=u16>){TRACE.with(|t|*t.borrow_mut()=Trace{samples:bits.into_iter().collect(),..Trace::default()});}
#[test]fn start_opens_the_save_screen_and_the_first_slot_keeps_the_defaults(){
 let _guard=LOCK.lock().unwrap();fresh();
 // As in the source, Start Game leads to the save screen rather than straight
 // into play; confirming slot 1 is what begins.
 for b in [button::START,button::CROSS]{
  let mut samples=vec![0;8];samples.push(b);samples.push(0);samples.push(button::CROSS);samples.extend([0;40]);feed(samples);
  let (s,p)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(s,menu::Settings::new());assert_eq!(p,0);
  TRACE.with(|t|{let t=t.borrow();assert!(t.volumes.is_empty());assert_eq!(t.uploads.len(),23);});
 }
}
#[test]fn options_call_real_volume_hooks_and_controls_return_without_auto_start(){
 let _guard=LOCK.lock().unwrap();fresh();use menu::state::*;
 let mut samples=Vec::new();for b in [DOWN,CROSS,LEFT,DOWN,LEFT,DOWN,LEFT,CIRCLE,DOWN,CROSS,CIRCLE,DOWN,DOWN,START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (s,_)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(s,menu::Settings{sfx:9,ambience:9,music:9,..menu::Settings::new()});
 TRACE.with(|t|{let t=t.borrow();assert_eq!(t.volumes,[(0,9),(1,9),(2,9)]);assert!(t.sprites>100);});
}
#[test]fn retry_requires_release_and_patch_uploads_keep_their_exact_ranges(){
 let _guard=LOCK.lock().unwrap();fresh();feed([button::START,button::START,0,button::CROSS]);menu::restore(Some(ART));menu::retry(&mut framebuf::FrameBuffer);
 TRACE.with(|t|{let t=t.borrow();assert_eq!(t.polls,4);// `restore` uploads the glyphs ahead of the art now; find the patches by place.
  let patch=t.uploads.iter().find(|v|v.0.x==544).expect("the status patches");assert_eq!(patch.0.h,64);assert_eq!(patch.1,12288);
  assert_eq!(t.uploads.iter().map(|v|v.1).sum::<usize>(),89600+12832+4976+7680+32);// art, glyphs, the title ornaments, the card glyphs, then the style's logo
 });
 feed([0]);menu::loading(&mut framebuf::FrameBuffer);TRACE.with(|t|assert_eq!(t.borrow().polls,1));
}

#[test]fn cheats_render_within_screen_and_carry_settings_to_gameplay(){
 let _guard=LOCK.lock().unwrap();fresh();use menu::state::*;
 let mut samples=Vec::new();for b in [UP,CROSS,CROSS,DOWN,RIGHT,DOWN,RIGHT,DOWN,RIGHT,CIRCLE,DOWN,START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (s,_)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(s.cheats.bits(),15);
 TRACE.with(|t|assert!(t.borrow().sprites>100));
}
#[test]fn the_save_screen_returns_the_confirmed_profile_and_can_be_left(){
 let _guard=LOCK.lock().unwrap();fresh();use menu::state::*;
 // Start Game, down twice to slot 3, confirm.
 let mut samples=Vec::new();for b in [START,DOWN,DOWN,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (_,p)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(p,2);
 // Circle leaves the save screen for the title, so the next Start still works.
 let mut samples=Vec::new();for b in [START,DOWN,CIRCLE,START,DOWN,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (_,p)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(p,1);
 // A slot's line is whatever the caller surveyed, including a card fault.
 let mut samples=Vec::new();for b in [START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let lines=["Geo 42","Empty","Damaged","Empty"];
 let (_,p)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&lines,"Memory card full");assert_eq!(p,0);
}
#[test]fn brightness_and_screen_position_reach_the_display_and_carry_to_gameplay(){
 let _guard=LOCK.lock().unwrap();fresh();use menu::state::*;
 display::STATE.with(|s|*s.borrow_mut()=(0,(0,0),Vec::new()));
 // Options, down to Brightness (+2), Screen X (-3), Screen Y (+1), back, then start slot 1.
 let mut samples=Vec::new();for b in [DOWN,CROSS,DOWN,DOWN,DOWN,RIGHT,RIGHT,DOWN,LEFT,LEFT,LEFT,DOWN,RIGHT,CIRCLE,UP,START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (s,_)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");
 assert_eq!((s.brightness,s.screen_x,s.screen_y),(2,-3,1));assert_eq!(s,menu::Settings{brightness:2,screen_x:-3,screen_y:1,..menu::Settings::new()});
 display::STATE.with(|d|{let d=d.borrow();assert_eq!((d.0,d.1),(2,(-3,1)));assert!(!d.2.is_empty()&&d.2.iter().all(|&g|g<=128));});
 // The next visit to the title starts from what is set now.
 let mut samples=Vec::new();for b in [START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (s,_)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!((s.brightness,s.screen_x,s.screen_y),(2,-3,1));
}

#[test]fn the_options_page_changes_the_title_style_with_a_fade_a_disc_read_and_a_new_upload(){
 let _guard=LOCK.lock().unwrap();fresh();use menu::state::*;
 // Options, down to Menu style, Right: from Modern the list wraps to Classic, which is read from the disc with
 // the title's song stopped and started again. The old style fades out over a few frames (the samples drop
 // meanwhile), then its art is uploaded; then Back, Start, slot 1.
 let mut samples=Vec::new();for b in [DOWN,CROSS]{samples.extend([b,0]);}
 for _ in 0..STYLE_ROW{samples.extend([DOWN,0]);}
 samples.extend([RIGHT,0]);samples.extend([0;24]);
 for b in [DOWN,CROSS,UP,START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (s,p)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(p,0);assert_eq!(s,menu::Settings{style:0,..menu::Settings::new()});
 // The title came up with Modern's art and the change uploaded Classic's palette.
 TRACE.with(|t|{let t=t.borrow();assert_eq!(t.uploads.iter().filter(|v|v.0.x==768&&v.0.y==480).count(),2);assert_eq!(t.style_reads,1);assert_eq!(t.music_begins,2);});
 // The next visit to the title keeps the style, which is read again.
 let mut samples=Vec::new();for b in [START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (s,_)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(s.style,0);
}
#[test]fn the_selector_skips_the_styles_whose_unlocks_are_not_set_and_the_cheats_page_sets_them_all(){
 let _guard=LOCK.lock().unwrap();fresh();use menu::state::*;
 // The unlock rules of the source: Infected, Shade, Steel and Eternal wait for their keys.
 let mut s=Settings::new();s.style=3;
 assert!(s.adjust(STYLE_ROW,1));assert_eq!(STYLE_NAMES[usize::from(s.style)],"Voidheart");
 assert!(s.adjust(STYLE_ROW,-1));assert_eq!(STYLE_NAMES[usize::from(s.style)],"GG");
 s.unlocked=4;// steelSoulMenu alone
 assert!(s.adjust(STYLE_ROW,1));assert_eq!(STYLE_NAMES[usize::from(s.style)],"Steel");
 assert!(s.adjust(STYLE_ROW,1));assert_eq!(STYLE_NAMES[usize::from(s.style)],"Voidheart");
 s.unlocked=ALL_STYLE_KEYS;s.style=3;
 for want in ["Infected","Shade","Steel","Eternal","Voidheart","Modern","Classic"]{assert!(s.adjust(STYLE_ROW,1));assert_eq!(STYLE_NAMES[usize::from(s.style)],want);}
 // Through the title: Cheats (Up from the first row wraps to it), down to the unlock row, Cross; Back; Options,
 // down to Menu style; Left steps back from Modern over Voidheart to Eternal, now unlocked.
 TRACE.with(|t|t.borrow_mut().unlocks.clear());
 let mut samples=Vec::new();for b in [UP,CROSS]{samples.extend([b,0]);}
 for _ in 0..UNLOCK_ROW{samples.extend([DOWN,0]);}
 for b in [CROSS,DOWN,CROSS]{samples.extend([b,0]);}
 for b in [DOWN,DOWN,CROSS]{samples.extend([b,0]);}
 for _ in 0..STYLE_ROW{samples.extend([DOWN,0]);}
 for _ in 0..2{samples.extend([LEFT,0]);samples.extend([0;24]);}
 for b in [DOWN,CROSS,UP,START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let (s,_)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");
 TRACE.with(|t|assert_eq!(t.borrow().unlocks,[ALL_STYLE_KEYS],"the card is written once"));
 assert_eq!((s.unlocked,s.style),(ALL_STYLE_KEYS,7),"Eternal, two steps from Modern");
 // And a card that already holds keys hands them to the selector at the title.
 TRACE.with(|t|{*t.borrow_mut()=Trace::default();t.borrow_mut().unlocks.push(1);});
 let mut samples=Vec::new();for b in [START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 TRACE.with(|t|t.borrow_mut().unlocks.push(1));
 let (s,_)=menu::run(&mut framebuf::FrameBuffer,Some(ART),&EMPTY,"");assert_eq!(s.unlocked,1);
}
#[test]fn a_style_blob_of_particles_and_layers_draws_one_quad_each_per_frame_they_are_on_screen(){
 let _guard=LOCK.lock().unwrap();fresh();use menu::state::*;
 // One upload, one emitter of two slots (one track per variant, all alike) and one layer of one sample.
 let mut blob=b"HKX1".to_vec();blob.extend([1,1,1,0]);
 let payload_at=8+12+(28+2*65)+(32+4);
 blob.extend(768u16.to_le_bytes());blob.extend(0u16.to_le_bytes());blob.extend(64u16.to_le_bytes());blob.extend(1u16.to_le_bytes());blob.extend((payload_at as u32).to_le_bytes());
 blob.extend([0,0,0,16,16,1,0,2]);blob.extend([255;9]);blob.extend([255;9]);blob.extend([1,0]);
 for _ in 0..2{blob.push(60);for _ in 0..4{for v in [1600i16,1200,0,0,0,0]{blob.extend(v.to_le_bytes());}blob.extend([4,128,128,128]);}}
 blob.extend([0,0,0,0,16,16,1,0]);for (x,y) in [(0i16,0i16),(100,0),(0,100),(100,100)]{blob.extend(x.to_le_bytes());blob.extend(y.to_le_bytes());}
 blob.extend([128,128,128,0]);blob.extend(1u16.to_le_bytes());blob.extend([1,0]);blob.extend([0,128,0,0]);
 blob.extend((0..64u8).flat_map(|_|[0x11u8,0x11]));
 assert_eq!(blob.len(),payload_at+128);
 let at=89712+12832+10768+7680;let mut chunk=ART.to_vec();chunk[at..at+blob.len()].copy_from_slice(&blob);
 // Straight to the save screen: the two slots draw while a page shows, and the layer too.
 let mut samples=Vec::new();for b in [START,CROSS]{samples.extend([b,0]);}samples.extend([0;40]);feed(samples);
 let _=menu::run(&mut framebuf::FrameBuffer,Some(&chunk),&EMPTY,"");
 TRACE.with(|t|{let t=t.borrow();
  // The pieces went up as the one upload the table lists, ahead of the first frame.
  assert!(t.uploads.iter().any(|v|v.0.x==768&&v.0.y==0&&v.0.w==64&&v.0.h==1&&v.1==128),"{:?}",t.uploads.iter().map(|v|(v.0.x,v.0.y,v.1)).collect::<Vec<_>>());
  // Two slots at (400,300)/4 = (100,75) and a layer, each frame the extras are drawn.
  assert!(t.quads>=3*3,"{} quads",t.quads);});
}
