//! Boot-only functional menu with original title assets and Perpetua glyphs.
//! Scene admission reuses its title texture storage; no gameplay allocation.
use psx_gpu::{self as gpu, framebuf::FrameBuffer, material::TextureMaterial};
use psx_pad::{button, poll_port1};
use psx_vram::{upload_bytes, Clut, TexDepth, Tpage, VramRect};

#[path="menu_state.rs"]
pub mod state;
pub use state::Settings;
use state::{Page,State};
static FONT:&[u8]=include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/read_font.hk"));
#[no_mangle]pub static mut HK_MENU_PAGE:u32=0;
#[no_mangle]pub static mut HK_MENU_ROW:u32=0;
#[no_mangle]pub static mut HK_MENU_SFX:u32=10;
#[no_mangle]pub static mut HK_MENU_AMBIENCE:u32=10;

static DATA: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/menu.hk"));
const HEADER: usize = 16;
const PALETTE: usize = 512;
const LEFT: usize = 256 * 240;
const RIGHT: usize = 64 * 240;
const PATCH: usize = 192 * 32;
const METRICS:usize=HEADER+PALETTE+LEFT+RIGHT+2*PATCH;
const LEVELS:[&str;11]=["0%","10%","20%","30%","40%","50%","60%","70%","80%","90%","100%"];

fn draw(gain: u8) {
    let clut = Clut::new(768, 480).uv_clut_word();
    for (screen_x, vram_x, width) in [(0, 384, 256), (256, 512, 64)] {
        let page = Tpage::new(vram_x, 0, TexDepth::Bit8).uv_tpage_word(0);
        gpu::draw_sprite_material(
            screen_x,
            0,
            width,
            240,
            (0, 0),
            TextureMaterial::opaque(clut, page, (gain, gain, gain)),
        );
    }
}

fn text_width(value:&str)->i16 {
    value.bytes().map(|b|DATA[METRICS+(b-32)as usize]as i16).sum()
}
fn text(x:i16,y:i16,value:&str,gain:u8){
    let mut x=x;
    for b in value.bytes(){
        assert!((32..=126).contains(&b));let i=(b-32)as usize;
        if b!=b' ' {
            gpu::draw_sprite_material(x,y,12,12,((i%21*12)as u8,244),
                TextureMaterial::opaque(Clut::new(320,481).uv_clut_word(),
                    Tpage::new((i/21*64)as u16,256,TexDepth::Bit4).uv_tpage_word(0),(gain,gain,gain)));
        }
        x+=DATA[METRICS+i]as i16;
    }
}
fn centered(y:i16,value:&str,gain:u8){text(160-text_width(value)/2,y,value,gain);}
fn draw_menu(menu:&State,gain:u8){
    draw(gain);
    match menu.page {
        Page::Main=>{
            for (i,label)in state::MAIN_ITEMS.iter().enumerate(){
                let y=151+i as i16*19;let g=if i==menu.selected{gain}else{gain-gain/4};
                centered(y,label,g);
                if i==menu.selected{text(144-text_width(label)/2,y,">",gain);}
            }
            centered(222,"Up/Down: choose    X/START: select",gain);
        }
        Page::Options=>{
            centered(143,"OPTIONS",gain);
            for (i,label)in state::OPTION_ITEMS.iter().enumerate(){
                let y=161+i as i16*17;let g=if i==menu.selected{gain}else{gain-gain/4};
                text(86,y,label,g);
                if i<2{text(215,y,LEVELS[if i==0{menu.settings.sfx}else{menu.settings.ambience}as usize],g);}
                if i==menu.selected{text(70,y,">",gain);}
            }
            centered(221,"Left/Right: volume    O: back",gain);
        }
        Page::Controls=>{
            let border=gain/2;
            gpu::draw_rect_flat(16,39,288,192,border,border,border);
            gpu::draw_rect_flat(18,41,284,188,0,0,0);
            centered(51,"CONTROLS",gain);
            for (i,line)in state::CONTROL_LINES.iter().enumerate(){text(30,74+i as i16*14,line,gain);}
            centered(214,"X / O / START: back",gain);
        }
    }
}
fn present_samples(fb:&mut FrameBuffer,mut sample:impl FnMut(u16)) {
    psx_rt::interrupts::queue_gp1_at_vblank(fb.begin_deferred_swap());
    while psx_rt::interrupts::gp1_queue_pending(){
        psx_rt::interrupts::wait_vblank();
        // Process each real sample in order, even if a frame spans several blanks.
        sample(poll_port1().buttons.bits());
    }
    fb.apply_draw_target();
}
fn present(fb:&mut FrameBuffer)->u16 {
    let mut pressed=0;present_samples(fb,|bits|pressed|=bits);pressed
}
/// First START/Cross still directly enters gameplay. Other choices are exposed
/// with D-pad navigation. Settings apply immediately and persist for this run.
#[inline(never)] // Keep title rendering outside main's MIPS PC16 branch span.
pub fn run(fb:&mut FrameBuffer)->Settings {
    restore();let mut menu=State::new();let mut gain=0u8;
    loop {
        gain=gain.saturating_add(8).min(128);draw_menu(&menu,gain);
        let mut start=false;
        present_samples(fb,|bits|{
            if start{return;}
            let old=menu.settings;start=menu.step(bits);
            if old.sfx!=menu.settings.sfx{crate::audio::set_volume(menu.settings.sfx);}
            if old.ambience!=menu.settings.ambience{crate::ambience::set_volume(menu.settings.ambience);}
            unsafe{HK_MENU_PAGE=match menu.page{Page::Main=>0,Page::Options=>1,Page::Controls=>2};HK_MENU_ROW=menu.selected as u32;HK_MENU_SFX=menu.settings.sfx as u32;HK_MENU_AMBIENCE=menu.settings.ambience as u32;}
        });
        if start{break;}
    }
    while gain>0{gain=gain.saturating_sub(16);draw_menu(&menu,gain);let _=present(fb);}
    // Consume the activating hold so Cross cannot also cause a gameplay jump.
    while poll_port1().buttons.is_held(button::START|button::CROSS){psx_rt::interrupts::wait_vblank();}
    menu.settings
}

fn status(fb: &mut FrameBuffer, patch: u8) -> u16 {
    draw(128);
    gpu::draw_sprite_material(
        64,
        177,
        192,
        32,
        (64, patch * 32),
        TextureMaterial::opaque(
            Clut::new(768, 480).uv_clut_word(),
            Tpage::new(512, 0, TexDepth::Bit8).uv_tpage_word(0),
            (128, 128, 128),
        ),
    );
    present(fb)
}

/// Keep the title visible while the blocking room read completes.
pub fn loading(fb: &mut FrameBuffer) {
    let _ = status(fb, 0);
}

/// A failed read can be retried after releasing the previous activating press.
pub fn retry(fb: &mut FrameBuffer) {
    let mut released = false;
    loop {
        let pressed = status(fb, 1) & (button::START | button::CROSS) != 0;
        if released && pressed {
            return;
        }
        released |= !pressed;
    }
}

/// Restore boot UI texture storage after a failed in-game region load.
pub fn restore() {
    assert!(DATA.len() == METRICS+96);
    assert!(&DATA[..8] == b"HKMENU03");
    assert!(DATA[8..12] == [64, 1, 240, 0]);
    assert!(
        u32::from_le_bytes(DATA[12..16].try_into().unwrap()) as usize == LEFT + RIGHT + 2 * PATCH
    );
    upload_bytes(
        VramRect::new(768, 480, 256, 1),
        &DATA[HEADER..HEADER + PALETTE],
    );
    assert!(DATA[METRICS..METRICS+95].iter().all(|&n|(1..=12).contains(&n)) && DATA[METRICS+95]==0);
    let pixels = &DATA[HEADER + PALETTE..METRICS];
    upload_bytes(VramRect::new(384, 0, 128, 240), &pixels[..LEFT]);
    upload_bytes(VramRect::new(512, 0, 32, 240), &pixels[LEFT..LEFT + RIGHT]);
    upload_bytes(VramRect::new(544, 0, 96, 64), &pixels[LEFT + RIGHT..]);
    // Reuse the exact original Perpetua glyph sheet and its gameplay reservation.
    // No new font asset or VRAM region is introduced by the menu.
    assert_eq!(FONT.len(),7712);
    upload_bytes(VramRect::new(320,481,16,1),&FONT[..32]);
    for page in 0..5{upload_bytes(VramRect::new(page*64,500,64,12),&FONT[32+page as usize*1536..32+(page as usize+1)*1536]);}
}
