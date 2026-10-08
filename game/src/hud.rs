//! Original health-mask and SOUL-orb poses, uploaded once outside gameplay.
use hk_format::u16_at;
use psx_gpu::{material::TextureMaterial, ot::OrderingTable, prim::{RectFlat, Sprite}};
use psx_vram::{upload_bytes, Clut, TexDepth, Tpage, VramRect};
#[path="hud_state.rs"] mod hud_state;
pub use hud_state::{mask_position,geo_y};
static DATA: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/hud.hk"));
const TEXTURE_X:[u16;5]=[352,352,356,366,372];
const TEXTURE_Y:[u16;5]=[0,16,0,0,0];
const CLUT_Y:[u16;5]=[480,481,482,483,484];
fn signed(p:usize)->i16 {u16_at(DATA,p) as i16}
fn fixed(p:usize)->i32 {i32::from_le_bytes(DATA[p..p+4].try_into().unwrap())}
pub fn upload() {
    assert!(&DATA[..8] == b"HKHUD002");
    for i in 0..5 {
        upload_bytes(VramRect::new(352,CLUT_Y[i],16,1), &DATA[80+i*32..112+i*32]);
        let p=8+i*8; let w=u16_at(DATA,p);let h=u16_at(DATA,p+2);
        let start=240+u16_at(DATA,p+4) as usize;
        let len=((w as usize+3)/4)*2*h as usize;
        upload_bytes(VramRect::new(TEXTURE_X[i],TEXTURE_Y[i],(w+3)/4,h),&DATA[start..start+len]);
    }
}
// All HUD commands remain in the scenery DMA chain. No immediate GP0 writes
// or texture transfers occur while drawing a frame.
#[repr(C, align(4))]
struct Mode { tag:u32, word:u32 }
static mut MODE:Mode=Mode{tag:0,word:0};
static mut MASKS:[Sprite;9]=[const {Sprite::new(0,0,0,0,(0,0),0,128,128,128)};9];
static mut ORB:[Sprite;3]=[const {Sprite::new(0,0,0,0,(0,0),0,128,128,128)};3];
static mut PAUSE:[RectFlat;2]=[const {RectFlat::new(0,0,0,0,220,220,220)};2];
fn sprite(icon:usize,x:i16,y:i16,cut:u16,gain:u8)->Sprite {
    let p=8+icon*8;
    let material=TextureMaterial::opaque(Clut::new(352,CLUT_Y[icon]).uv_clut_word(),
        Tpage::new(320,0,TexDepth::Bit4).uv_tpage_word(0),(gain,gain,gain));
    Sprite::with_material(x,y+cut as i16,u16_at(DATA,p),u16_at(DATA,p+2)-cut,
        (((TEXTURE_X[icon]-320)*4) as u8,(TEXTURE_Y[icon]+cut) as u8),material)
}
/// Prepend before scenery, leaving the HUD last in GPU order. Caller owns
/// every static packet until both DMA and rasterization finish.
pub fn append(ot:&mut OrderingTable<1>,health:u16,max_health:u16,soul:u16,max_soul:u16,paused:bool,blue_health:u16) {
    crate::lifeblood::append_hud(ot,max_health,blue_health);
    unsafe {
    if paused {for (i,x) in [292,300].into_iter().enumerate().rev() {
        PAUSE[i]=RectFlat::new(x,16,4,12,220,220,220);
        ot.add(0,&mut PAUSE[i],RectFlat::WORDS);
    }}
    for i in (0..max_health.min(9)).rev() {
        let(x,y)=mask_position(i);
        MASKS[i as usize]=sprite(usize::from(i>=health),x,y,0,128);
        ot.add(0,&mut MASKS[i as usize],Sprite::WORDS);
    }
    let fill_h=u16_at(DATA,34);
    let state=hud_state::SoulSpec {cut_zero:fixed(56),cut_per_mp:fixed(60),height:fill_h,
        focus_cost:u16_at(DATA,64),eyes_at:u16_at(DATA,66),hide_at:u16_at(DATA,68),
        dim_gain:u16_at(DATA,70) as u8,source_max:u16_at(DATA,72)}.state(soul,max_soul);
    // Source Eyes Control changes at 50 MP independently of the 33 MP tint.
    if state.eyes {
        ORB[2]=sprite(4,12+signed(52),10+signed(54),0,128);
        ot.add(0,&mut ORB[2],Sprite::WORDS);
    }
    if state.fill {
        ORB[1]=sprite(3,12+signed(48),10+signed(50),state.cut,state.gain);
        ot.add(0,&mut ORB[1],Sprite::WORDS);
    }
    ORB[0]=sprite(2,12,10,0,128);ot.add(0,&mut ORB[0],Sprite::WORDS);
    MODE.word=TextureMaterial::opaque(0,Tpage::new(320,0,TexDepth::Bit4).uv_tpage_word(0),(128,128,128)).draw_mode_word();
    ot.add(0,&mut *(&raw mut MODE),1);
    }
}
