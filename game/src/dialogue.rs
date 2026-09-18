//! Original tablet text/Perpetua font; bounded PS1 panel adaptation.
use psx_gpu::{ot::OrderingTable,prim::RectFlat};
use psx_vram::{upload_bytes,Clut,TexDepth,Tpage,VramRect};
#[path="read_state.rs"]mod read_state;
use read_state::{ReadPoint,State};
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/read_points.rs"));
static FONT:&[u8]=include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/read_font.hk"));
static mut STATE:State=State::new();
#[no_mangle]pub static mut HK_READ_SOURCE:u32=0;
#[no_mangle]pub static mut HK_READ_PAGE:u32=0;
#[no_mangle]pub static mut HK_READ_OPENED:u32=0;
#[no_mangle]pub static mut HK_READ_CLOSED:u32=0;
const CAP:usize=416;
#[repr(C,align(4))]struct Glyph{tag:u32,mode:u32,color:u32,xy:u32,uv:u32,wh:u32}
const EMPTY:Glyph=Glyph{tag:0,mode:0,color:0,xy:0,uv:0,wh:0};
static mut GLYPHS:[Glyph;CAP]=[const{EMPTY};CAP];
static mut BOXES:[RectFlat;2]=[const{RectFlat::new(0,0,0,0,0,0,0)};2];
static mut USED:usize=0;
static mut BOX_COUNT:usize=0;
pub fn upload(){
    assert_eq!(FONT.len(),7712);
    upload_bytes(VramRect::new(320,481,16,1),&FONT[..32]);
    for page in 0..5{upload_bytes(VramRect::new(page*64,500,64,12),&FONT[32+page as usize*1536..32+(page as usize+1)*1536]);}
}
pub fn open()->bool{unsafe{STATE.open()}}
pub fn consumes_actions()->bool{unsafe{STATE.consumes_actions()}}
pub fn cancel(){unsafe{if STATE.open(){HK_READ_CLOSED+=1;}STATE.cancel();HK_READ_SOURCE=0;}}
pub fn reset(){unsafe{STATE=State::new();HK_READ_SOURCE=0;HK_READ_PAGE=0;}}
pub fn tick(scene:usize,player:&hk_sim::Player,params:hk_sim::Params,eligible:bool,bits:u16){
    let body=[player.x-params.half_width,player.y+params.bottom,player.x+params.half_width,player.y+params.top];
    unsafe{let e=STATE.step(POINTS,scene,body,eligible,bits);HK_READ_OPENED+=u32::from(e.opened);HK_READ_CLOSED+=u32::from(e.closed);
      HK_READ_SOURCE=STATE.active.map_or(0,|i|POINTS[i].source_id);HK_READ_PAGE=STATE.page as u32;}
}
fn width(text:&str)->i16{text.bytes().map(|b|ADVANCES[(b-32) as usize] as i16).sum()}
fn text(x:i16,y:i16,text:&str){
    let mut x=x;
    for b in text.bytes(){
        assert!((32..=126).contains(&b));let i=(b-32) as usize;
        if b!=b' ' {
            unsafe{assert!(USED<CAP);let p=&mut GLYPHS[USED];USED+=1;
                p.mode=0xe100_0000|Tpage::new((i/21*64) as u16,256,TexDepth::Bit4).uv_tpage_word(0) as u32;
                p.color=0x6480_8080;p.xy=x as u16 as u32|((y as u16 as u32)<<16);
                p.uv=(i%21*12) as u32|(244<<8)|((Clut::new(320,481).uv_clut_word() as u32)<<16);
                p.wh=12|(12<<16);
            }
        }
        x+=ADVANCES[i] as i16;
    }
}
fn number(x:i16,y:i16,value:u32) {
    let mut digits=[b'0';10];let mut start=9;let mut value=value;
    digits[start]+= (value%10)as u8;value/=10;
    while value!=0 {start-=1;digits[start]+=(value%10)as u8;value/=10;}
    text(x,y,core::str::from_utf8(&digits[start..]).unwrap());
}
#[inline(never)] // UI composition must not expand main beyond MIPS branch reach.
pub fn prepare(camera:(i32,i32),geo:u32,pause:Option<(&crate::pause::State,&crate::menu::Settings)>,total_masks:u16,save_prompt:Option<u8>){
    unsafe{USED=0;BOX_COUNT=0;
      let geo_y=crate::hud::geo_y(total_masks);text(18,geo_y,"Geo");number(43,geo_y,geo);
      if let Some((p,settings))=pause {
        BOXES[0]=RectFlat::new(38,61,244,162,150,150,150);BOXES[1]=RectFlat::new(40,63,240,158,0,0,0);BOX_COUNT=2;
        if p.cheats {
            BOXES[0]=RectFlat::new(14,28,292,204,150,150,150);BOXES[1]=RectFlat::new(16,30,288,200,0,0,0);
            text(160-width("Cheats")/2,38,"Cheats");
            for(i,line) in crate::menu::state::CHEAT_ITEMS.iter().chain(["Restore health + SOUL","Add 5 Lifeblood masks","Turn all cheats off","Back"].iter()).enumerate(){
                let y=50+i as i16*13;text(46,y,line);if p.row==i{text(29,y,">");}
                if i<crate::menu::state::CHEAT_ITEMS.len(){text(255,y,if settings.cheats.enabled(i){"ON"}else{"OFF"});}
            }
            text(160-width("Charms are not implemented yet")/2,200,"Charms are not implemented yet");
            text(160-width("X: apply    Left/Right: off/on    O: back")/2,214,"X: apply    Left/Right: off/on    O: back");
        }else if p.controls {
            text(160-width("Controls")/2,69,"Controls");
            for (i,line)in ["D-pad: Move / aim","X: Jump    Square: Nail","O: Hold to Focus / heal","Up: Inspect    Start: Pause","Air Down + Square: Pogo","X / O: Back"].iter().enumerate() {
                text(53,89+i as i16*17,line);
            }
        }else {
            text(160-width("Paused")/2,69,"Paused");
            for(i,line)in ["Resume","Sound effects","Ambience","Music","Controls","Cheats"].iter().enumerate() {
                let y=92+i as i16*18;text(73,y,line);if p.row==i {text(56,y,">");}
            }
            number(222,110,u32::from(settings.sfx));number(222,128,u32::from(settings.ambience));number(222,146,u32::from(settings.music));
            text(239,110,"/10");text(239,128,"/10");text(239,146,"/10");
            text(58,207,"Left / Right: Volume   O: Resume");
        }
      }else if let Some(row)=save_prompt {
        // Seated at a bench. The source saves silently; this port asks first.
        BOXES[0]=RectFlat::new(92,84,136,72,150,150,150);BOXES[1]=RectFlat::new(94,86,132,68,0,0,0);BOX_COUNT=2;
        text(160-width("Save game?")/2,96,"Save game?");
        for (i,label) in ["Yes","No"].iter().enumerate() {
            let y=116+i as i16*18;text(146,y,label);
            if row as usize==i {text(130,y,">");}
        }
      }else if let Some(id)=STATE.active{
        BOXES[0]=RectFlat::new(10,46,300,152,150,150,150);BOXES[1]=RectFlat::new(12,48,296,148,0,0,0);BOX_COUNT=2;
        for (i,line) in POINTS[id].pages[STATE.page].iter().enumerate(){text(24,59+i as i16*14,line);}
        let footer=if STATE.page+1<POINTS[id].pages.len(){"X: Next    O: Close"}else{"X / O: Close"};
        text(160-width(footer)/2,178,footer);
      }else if let Some(id)=STATE.prompt{
        let p=POINTS[id].marker;
        let x=160+((((p[0]-camera.0)>>8) as i64*crate::KNIGHT_SCALE as i64)>>20) as i32;
        let y=120-((((p[1]-camera.1)>>8) as i64*crate::KNIGHT_SCALE as i64)>>20) as i32;
        let label=POINTS[id].label;let w=width(label);let x=(x-w as i32/2).clamp(4,316-w as i32) as i16;let y=y.clamp(32,220) as i16;
        BOXES[0]=RectFlat::new(x-3,y-2,w as u16+6,16,0,0,0);BOX_COUNT=1;text(x,y,label);
      }
    }
}
/// Called before scenery/HUD insertions: prepend ordering puts the panel last.
pub fn append(ot:&mut OrderingTable<1>){unsafe{
    for i in (0..USED).rev(){ot.add(0,&mut GLYPHS[i],5);}
    for i in (0..BOX_COUNT).rev(){ot.add(0,&mut BOXES[i],3);}
}}
