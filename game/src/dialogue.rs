//! Original tablet and NPC text/Perpetua font; bounded PS1 panel adaptation.
use psx_gpu::{ot::OrderingTable,prim::RectFlat};
use psx_vram::{upload_bytes,Clut,TexDepth,Tpage,VramRect};
#[path="read_state.rs"]mod read_state;
use read_state::{ReadPoint,State};
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/read_points.rs"));
/// One cooked NPC conversation: the entries its `Conversation Control` speaks
/// on a fresh save, flattened into panel pages in the order its states run
/// them, plus the page whose own state writes the NPC's met PlayerData bool.
pub struct NpcLines{pub scene:usize,pub source_id:u32,pub label:&'static str,pub marker:[i32;2],
    pub pages:&'static [&'static [&'static str]],pub met_page:usize}
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/npc_lines.rs"));
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
/// The buttons a panel owns while it is up; the tablet reader's own set.
const NPC_ACTIONS:u16=read_state::UP|read_state::DOWN|read_state::CROSS|read_state::CIRCLE|0x8000;
/// An open NPC conversation. The tablet's `State` owns a list of world
/// triggers; an NPC's trigger is a resident world-bank record instead, so the
/// caller passes the NPC already in range and this keeps only the panel.
struct Convo{active:Option<usize>,prompt:Option<usize>,page:usize,previous:u16,release:bool}
impl Convo{const fn new()->Self{Self{active:None,prompt:None,page:0,previous:0,release:false}}}
static mut CONVO:Convo=Convo::new();
#[no_mangle]pub static mut HK_NPC_SOURCE:u32=0;
#[no_mangle]pub static mut HK_NPC_PAGE:u32=0;
#[no_mangle]pub static mut HK_NPC_OPENED:u32=0;
/// Source PlayerData `metElderbug`. One cooked NPC writes a met bool today, so
/// it is a session flag here; a second would need a real PlayerData record.
#[no_mangle]pub static mut HK_MET_ELDERBUG:u32=0;
pub fn open()->bool{unsafe{STATE.open()||CONVO.active.is_some()}}
pub fn consumes_actions()->bool{unsafe{STATE.consumes_actions()||CONVO.active.is_some()||CONVO.release}}
pub fn cancel(){unsafe{if STATE.open(){HK_READ_CLOSED+=1;}STATE.cancel();HK_READ_SOURCE=0;convo_cancel();}}
pub fn reset(){unsafe{STATE=State::new();HK_READ_SOURCE=0;HK_READ_PAGE=0;
  CONVO=Convo::new();HK_NPC_SOURCE=0;HK_NPC_PAGE=0;HK_MET_ELDERBUG=0;}}
pub fn tick(scene:usize,player:&hk_sim::Player,params:hk_sim::Params,eligible:bool,bits:u16){
    let body=[player.x-params.half_width,player.y+params.bottom,player.x+params.half_width,player.y+params.top];
    // One panel at a time: a tablet cannot open behind an open conversation.
    let eligible=eligible&&unsafe{CONVO.active.is_none()};
    unsafe{let e=STATE.step(POINTS,scene,body,eligible,bits);HK_READ_OPENED+=u32::from(e.opened);HK_READ_CLOSED+=u32::from(e.closed);
      HK_READ_SOURCE=STATE.active.map_or(0,|i|POINTS[i].source_id);HK_READ_PAGE=STATE.page as u32;}
}
/// The NPC whose conversation is open, by its cooked source object id.
pub fn npc_speaking()->Option<u32>{unsafe{CONVO.active.map(|i|NPC_LINES[i].source_id)}}
fn convo_cancel(){unsafe{CONVO.active=None;CONVO.prompt=None;CONVO.page=0;
  CONVO.release=CONVO.previous&NPC_ACTIONS!=0;HK_NPC_SOURCE=0;HK_NPC_PAGE=0;}}
/// One tick of the NPC panel. `in_range` is the cooked NPC whose talk trigger
/// the hero body is inside; UP there is npc_control's own CONVO START. Paging
/// and closing follow the tablet panel exactly, including holding the closing
/// press until it is released so it cannot also swing the nail.
pub fn npc_tick(scene:usize,in_range:Option<u32>,eligible:bool,bits:u16){unsafe{
    let eligible=eligible&&!STATE.open();
    let pressed=bits & !CONVO.previous;CONVO.previous=bits;
    if bits&NPC_ACTIONS==0 {CONVO.release=false;}
    if let Some(id)=CONVO.active {
        if NPC_LINES[id].scene!=scene || pressed&read_state::CIRCLE!=0 {convo_cancel();}
        else if pressed&read_state::CROSS!=0 {
            if CONVO.page+1<NPC_LINES[id].pages.len(){CONVO.page+=1;}else{convo_cancel();}
        }
    } else {
        CONVO.prompt=if eligible && !CONVO.release {
            in_range.and_then(|source|NPC_LINES.iter().position(|n|n.scene==scene&&n.source_id==source))
        }else{None};
        // npc_control listens for UP alone, unlike the tablet reader.
        if pressed&read_state::UP!=0 {
            if let Some(id)=CONVO.prompt {CONVO.active=Some(id);CONVO.page=0;HK_NPC_OPENED+=1;}
        }
    }
    if let Some(id)=CONVO.active {
        // Source `Intro Main` writes metElderbug as PlayMaker enters it, which
        // is before the DialogueBox call beside the write has spoken.
        if CONVO.page>=NPC_LINES[id].met_page {HK_MET_ELDERBUG=1;}
        HK_NPC_SOURCE=NPC_LINES[id].source_id;HK_NPC_PAGE=CONVO.page as u32;
    }
}}
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
                let y=44+i as i16*11;text(46,y,line);if p.row==i{text(29,y,">");}
                if i<crate::menu::state::CHEAT_ITEMS.len(){text(255,y,if settings.cheats.enabled(i){"ON"}else{"OFF"});}
            }
            text(160-width("X: apply    Left/Right: off/on    O: back")/2,214,"X: apply    Left/Right: off/on    O: back");
        }else if p.controls {
            text(160-width("Controls")/2,69,"Controls");
            for (i,line)in ["D-pad: Move / aim","X: Jump    Square: Nail","O: Hold to Focus / heal","Up: Inspect    Start: Pause","Air Down + Square: Pogo","L1: Dash    Hold R1: Crystal Heart","Hold Triangle: Dream Nail","X / O: Back"].iter().enumerate() {
                text(53,84+i as i16*16,line);
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
        panel(POINTS[id].pages,STATE.page);
      }else if let Some(id)=CONVO.active{
        panel(NPC_LINES[id].pages,CONVO.page);
      }else if let Some(id)=STATE.prompt{
        prompt_marker(camera,POINTS[id].marker,POINTS[id].label);
      }else if let Some(id)=CONVO.prompt{
        prompt_marker(camera,NPC_LINES[id].marker,NPC_LINES[id].label);
      }
    }
}
/// The one text panel, shared by a tablet and an NPC conversation.
fn panel(pages:&[&'static [&'static str]],page:usize){
    unsafe{BOXES[0]=RectFlat::new(10,46,300,152,150,150,150);BOXES[1]=RectFlat::new(12,48,296,148,0,0,0);BOX_COUNT=2;}
    for (i,line) in pages[page].iter().enumerate(){text(24,59+i as i16*14,line);}
    let footer=if page+1<pages.len(){"X: Next    O: Close"}else{"X / O: Close"};
    text(160-width(footer)/2,178,footer);
}
/// The source prompt marker's label, over the marker's own world point.
fn prompt_marker(camera:(i32,i32),p:[i32;2],label:&str){
    let x=160+((((p[0]-camera.0)>>8) as i64*crate::KNIGHT_SCALE as i64)>>20) as i32;
    let y=120-((((p[1]-camera.1)>>8) as i64*crate::KNIGHT_SCALE as i64)>>20) as i32;
    let w=width(label);let x=(x-w as i32/2).clamp(4,316-w as i32) as i16;let y=y.clamp(32,220) as i16;
    unsafe{BOXES[0]=RectFlat::new(x-3,y-2,w as u16+6,16,0,0,0);BOX_COUNT=1;}
    text(x,y,label);
}
/// Called before scenery/HUD insertions: prepend ordering puts the panel last.
pub fn append(ot:&mut OrderingTable<1>){unsafe{
    for i in (0..USED).rev(){ot.add(0,&mut GLYPHS[i],5);}
    for i in (0..BOX_COUNT).rev(){ot.add(0,&mut BOXES[i],3);}
}}
