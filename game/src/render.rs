//! Resident scenery plus bounded animation residency and SDK DMA packets.
use crate::animation_cache;
use hk_format::{i32_at, u16_at, u32_at, Room, Texture};
use psx_gpu::{self as gpu, ot::OrderingTable, prim::QuadTextured};
use psx_vram::{Clut, TexDepth, Tpage};
use crate::{alpha_scissor_cache,draw_packet::Packet,scenery_geometry::{self,UvRect,Quad}};
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scenery_budgets.rs"));
// Maximum scenery records plus the Knight and the active nail effect.
const CAP: usize = 1032;
const MAX_DRAWS:usize=crate::disc::SCENE_DRAW_CAPACITY;
const FRONT:u8=1;
const LEGAL_EXTENT:u8=2;
const BLACK_AVERAGE:u8=4;
const SCISSOR_CANDIDATE:u8=8;
const EMPTY_TEXTURE:u8=16;
const SOLID_BLACK:u8=32;
const ZERO: QuadTextured = QuadTextured {
    tag: 0,
    color_cmd: 0,
    v0: 0,
    uv0_clut: 0,
    v1: 0,
    uv1_tpage: 0,
    v2: 0,
    uv2: 0,
    v3: 0,
    uv3: 0,
};
#[derive(Clone, Copy)]
struct Draw {
    xy: [i32; 8],
    scale: i32,
    flags: u8,
    texture: usize,
    tint: (u8, u8, u8),
}
static mut DRAWS: [Draw; MAX_DRAWS] = [Draw {
    xy: [0; 8],
    scale: 0,
    flags: 0,
    texture: 0,
    tint: (0, 0, 0),
}; MAX_DRAWS];
const MAX_TEXTURES:usize=crate::disc::SCENE_TEXTURE_CAPACITY;
static mut TEMPLATES: [QuadTextured; MAX_TEXTURES] = [const { ZERO }; MAX_TEXTURES];
static mut PACKETS: [Packet; CAP] = [const { Packet::ZERO }; CAP];
static mut OT: OrderingTable<1> = OrderingTable::new();
static mut USED: usize = 0;
static mut DRAW_COUNT: usize = 0;
static mut ACTIVE_BANK: usize = 0;
static mut TEXTURE_COUNT: usize = 0;
static mut STREAMED: [u32; MAX_TEXTURES/32] = [0; MAX_TEXTURES/32];
// Preserve cooked draw order while toggling authored whole-sprite variants.
// This is transient room state, reset when starting or respawning.
static mut VISIBLE: [u32; MAX_DRAWS/32] = [0; MAX_DRAWS/32];
static mut GAINS: [u8; MAX_DRAWS] = [128; MAX_DRAWS];
static mut OPACITIES: [u8; MAX_DRAWS] = [0; MAX_DRAWS];
static mut BLACK_MASKS: [u32;MAX_TEXTURES/32] = [0;MAX_TEXTURES/32];
static mut FADE_CLUT_READY: bool = false;
// Shared 16-word reservation, disjoint from scenery banks, animation and HUD.
const FADE_CLUT_X:u16=320;
const FADE_CLUT_Y:u16=480;
const FADE_CLUT:[u8;32]=[0,0,255,255,0,128,0,128,0,128,0,128,0,128,0,128,
    0,128,0,128,0,128,0,128,0,128,0,128,0,128,0,128];
fn binary_black_palette(p:&[u8])->bool {
    p.len()==32 && u16_at(p,0)==0 && u16_at(p,2)==1
        && (2..16).all(|i|u16_at(p,i*2)==0x8000)
}
// A count of255 means the legacy pack has no precomputed alpha cover.
// init fills every active entry before drawing; zero initialization keeps this
// working storage out of the EXE's on-disc data payload.
static mut COVERS: [[u8;20];MAX_TEXTURES] = [[0;20];MAX_TEXTURES];
// One entry per admitted draw. Geometry spans may change by one pixel with camera
// rounding; the entry key captures both signed spans and the texture identity.
static mut SCISSOR_CACHE:[alpha_scissor_cache::Entry;MAX_DRAWS]=[alpha_scissor_cache::Entry::EMPTY;MAX_DRAWS];
static mut EXTRA_ALLOWANCE: usize = 0;
static mut EXTRA_USED: usize = 0;
static mut FRAMEBUFFER_Y: u16 = 0;
// Cumulative diagnostics wrap after u32::MAX; they never control rendering.
#[no_mangle] pub static mut HK_SCENERY_REPAIRS:u32=0;
#[no_mangle] pub static mut HK_SCENERY_REPAIR_FAILURES:u32=0;
#[no_mangle] pub static mut HK_SCISSOR_QUADS:u32=0;
#[no_mangle] pub static mut HK_SCISSOR_SAVED_PIXELS:u32=0;
#[no_mangle] pub static mut HK_SCISSOR_CAPACITY_FALLBACKS:u32=0;
fn vertex(p: (i16, i16)) -> u32 {
    (p.0 as u16 as u32) | ((p.1 as u16 as u32) << 16)
}
#[inline(never)]
pub fn init(room: &Room, bank: usize, region:usize) {
    crate::input::checkpoint();
    unsafe {
        let scene_changed=TEXTURE_COUNT==0 || ACTIVE_BANK!=bank;
        if scene_changed {animation_cache::init();}
        ACTIVE_BANK = bank;
        assert!(room.counts[1]<=MAX_TEXTURES);
        DRAW_COUNT = room.counts[2];
        assert!(DRAW_COUNT<=MAX_DRAWS);
        TEXTURE_COUNT = room.counts[1];
        // The cooker bounds every mandatory child, even if all source draws
        // are visible. Optional scissors can only spend the remaining capacity.
        EXTRA_ALLOWANCE=CAP.checked_sub(DYNAMIC_PACKET_RESERVE+SCENERY_PACKET_BUDGETS[region]).expect("scenery packet budget");
        if scene_changed {
            STREAMED = [0;MAX_TEXTURES/32];
            BLACK_MASKS = [0;MAX_TEXTURES/32];
        }
        if !FADE_CLUT_READY {
            // First room only; FIFO upload owns static bytes and precedes any
            // room packets. No per-frame transfer or DMA lifetime is added.
            gpu::draw_sync();
            psx_vram::upload_bytes(psx_vram::VramRect::new(FADE_CLUT_X,FADE_CLUT_Y,16,1),&FADE_CLUT);
            FADE_CLUT_READY=true;
        }
        for i in 0..DRAW_COUNT {
            if i&31==0 {crate::input::checkpoint();}
            SCISSOR_CACHE[i].invalidate();
            let d = room.draw(i);
            let xy=core::array::from_fn(|k| i32_at(d, 8 + k * 4));
            DRAWS[i] = Draw {
                xy,
                scale: u32_at(d, 4) as i32,
                flags: u8::from(u16_at(d,2)!=0)*FRONT | u8::from(scenery_geometry::legal_extent_q8(&xy))*LEGAL_EXTENT | u8::from(d[43]==1)*BLACK_AVERAGE,
                texture: u16_at(d, 0) as usize,
                tint: (d[40], d[41], d[42]),
            };
        }
        if scene_changed {
        for i in 0..room.counts[1] {
            if i&31==0 {crate::input::checkpoint();}
            let t = room.texture(i);
            let p=t.palette as usize*32;
            if !t.is_streamed() && binary_black_palette(&room.palettes()[p..p+32]) {
                BLACK_MASKS[i/32]|=1<<(i%32);
            }
            if let Some(cover)=room.alpha_cover(t) {COVERS[i].copy_from_slice(cover);}
            else {COVERS[i]=[255;20];}
            if t.is_streamed() {
                STREAMED[i / 32] |= 1 << (i % 32);
                TEMPLATES[i] = ZERO;
            } else {
                let page=crate::disc::scene_page_base(bank)+t.page as usize;
                let (x,y)=hk_cache::residency::page_xy(page/hk_cache::residency::PAGES,page%hk_cache::residency::PAGES);
                let tp = Tpage::new(x,y,TexDepth::Bit4);
                TEMPLATES[i] = template(t, tp, t.u, t.v);
            }
        }
    }
        // Immutable cover/axis tests belong to activation, not every rendered
        // frame. A rejected candidate still draws its original complete quad.
        for (i,d) in DRAWS[..DRAW_COUNT].iter_mut().enumerate() {
            if i&31==0 {crate::input::checkpoint();}
            let cover=&COVERS[d.texture];
            if cover[0]==0 {d.flags|=EMPTY_TEXTURE;continue;}
            let xy=&d.xy;
            let axis=xy[1]==xy[3]&&xy[0]==xy[4]&&xy[2]==xy[6]&&xy[5]==xy[7];
            if axis && cover[0]<=4 {
                let uv=uv_rect(&TEMPLATES[d.texture]);
                let full=cover[0]==1&&cover[4]==0&&cover[5]==0&&cover[6]as u16+1==uv.w&&cover[7]as u16+1==uv.h;
                if !full {d.flags|=SCISSOR_CANDIDATE;}
            }
            // The source tilemap uses a constant opaque black patch. Verify
            // actual sampled words, not just a palette or an alpha bounding box.
            // Restrict the scan to tiny patches; larger textures use FT4 as usual.
            if d.flags&BLACK_AVERAGE!=0 {
                let t=room.texture(d.texture);
                if !t.is_streamed()&&t.width<=4&&t.height<=4 {
                    let page=room.page(t.page as usize);
                    let pal=&room.palettes()[t.palette as usize*32..][..32];
                    let solid=(0..t.height as usize).all(|y|(0..t.width as usize).all(|x| {
                        let u=t.u as usize+x;let v=t.v as usize+y;
                        let index=(page[v*128+u/2]>>((u&1)*4))&15;
                        u16_at(pal,index as usize*2)==1
                    }));
                    if solid {d.flags|=SOLID_BLACK;}
                }
            }
        }
    }
    reset_visibility();
    crate::input::checkpoint();
}
fn template(t: Texture, tp: Tpage, u: u16, v: u16) -> QuadTextured {
    let palette=crate::disc::scene_palette_base(unsafe {ACTIVE_BANK})+t.palette as usize;
    let (x,y)=hk_cache::residency::clut_xy(palette/hk_cache::residency::CLUTS,palette%hk_cache::residency::CLUTS);
    let cl = Clut::new(x,y);
    let uv = [
        (u as u8, v as u8),
        ((u + t.width - 1) as u8, v as u8),
        (u as u8, (v + t.height - 1) as u8),
        ((u + t.width - 1) as u8, (v + t.height - 1) as u8),
    ];
    QuadTextured::with_material(
        [(0, 0); 4],
        uv,
        gpu::material::TextureMaterial::blended(
            cl.uv_clut_word(),
            tp.uv_tpage_word(0),
            (128, 128, 128),
            gpu::material::BlendMode::Add,
        ),
    )
}
pub fn reset_visibility() {
    unsafe {
        VISIBLE = [u32::MAX; MAX_DRAWS/32];
        GAINS = [128; MAX_DRAWS];
        OPACITIES = [128; MAX_DRAWS];
    }
}
pub fn set_visible(draw: usize, visible: bool) {
    unsafe {
        assert!(draw < DRAW_COUNT);
        let bit = 1u32 << (draw % 32);
        if visible {
            VISIBLE[draw / 32] |= bit;
        } else {
            VISIBLE[draw / 32] &= !bit;
        }
    }
}
pub fn set_gain(draw: usize, gain:u8) {
    unsafe { assert!(draw<DRAW_COUNT); GAINS[draw]=gain.min(128); }
}
/// Source alpha, separately from RGB gain. Host admission must verify the
/// bound renderer has the supported binary black palette; never guess a shader.
pub fn set_opacity(draw:usize,alpha128:u8) {
    unsafe {
        assert!(draw<DRAW_COUNT);
        let t=DRAWS[draw].texture;
        assert!(BLACK_MASKS[t/32]&(1<<(t%32))!=0,"unsupported mask opacity palette");
        OPACITIES[draw]=alpha128.min(128);
    }
}
/// Supply all animation textures this frame will reference (Knight + nail).
/// Prepare before framebuffer clear; texture uploads never follow draw packets.
pub fn begin(room: &Room, animation_ids: &[u16],framebuffer_y:u16) -> hk_cache::Stats {
    let stats = animation_cache::prepare(room, animation_ids);
    unsafe {
        USED = 0;
        EXTRA_USED=0;FRAMEBUFFER_Y=framebuffer_y;
        for &id in animation_ids {
            let t = room.texture(id as usize);
            let (u, v) = animation_cache::uv(animation_cache::slot(id));
            TEMPLATES[id as usize] = template(t, Tpage::new(320, 0, TexDepth::Bit4), u, v);
        }
    }
    stats
}
pub fn finish_gpu_frame() {
    // Main has already observed DMA idle. Pair the SDK async submission with
    // its completion API before any packet or texture-cache storage is reused.
    gpu::submit_linked_list_wait();
    animation_cache::complete();
}
pub fn dma_pending() -> bool {
    psx_io::dma::is_busy(psx_io::dma::Channel::Gpu)
}
pub const SOURCE_ALPHA_COVERAGE:u8=1;
pub fn texture(id: usize, verts: [(i16, i16); 4], tint: (u8, u8, u8)) {
    texture_material_alpha(id,verts,tint,0);
}
/// Small, independently resident Geo art uses the same bounded DMA packet pool.
pub fn resident_quad(template:&QuadTextured,verts:[(i16,i16);4]) {
    unsafe {
        assert!(USED<CAP);
        PACKETS[USED].write_plain(quad_words(template,verts,(128,128,128),None,128));
        USED+=1;
    }
}
/// The selected texture has baked source/lifetime alpha coverage0,1/2,1.
/// Keep straight RGB: Average supplies source-over at the half-alpha midpoint.
/// This is spatial alpha quantization, not arbitrary continuous GPU blending.
pub fn texture_material_alpha(id:usize,verts:[(i16,i16);4],tint:(u8,u8,u8),material:u8) {
    assert!(material<=SOURCE_ALPHA_COVERAGE);
    unsafe {
        assert!(USED < CAP && id < TEXTURE_COUNT);
        if STREAMED[id / 32] & (1 << (id % 32)) != 0 {
            animation_cache::slot(id as u16);
        }
        let mut words=quad_words(&TEMPLATES[id],verts,tint,None,128);
        if material==SOURCE_ALPHA_COVERAGE {words[4]&=!(3<<21);}
        PACKETS[USED].write_plain(words);
        USED += 1;
    }
}
fn quad_words(t:&QuadTextured,v:[(i16,i16);4],tint:(u8,u8,u8),uv:Option<[(u8,u8);4]>,opacity:u16)->[u32;9] {
    let mut words=[(t.color_cmd&0xff00_0000)|tint.0 as u32|((tint.1 as u32)<<8)|((tint.2 as u32)<<16),
        vertex(v[0]),t.uv0_clut,vertex(v[1]),t.uv1_tpage,vertex(v[2]),t.uv2,vertex(v[3]),t.uv3];
    if let Some(uv)=uv {
        for i in 0..4 {words[2+i*2]=(words[2+i*2]&0xffff_0000)|uv[i].0 as u32|((uv[i].1 as u32)<<8);}
    }
    // Bit8 is explicit source material admission; STP0 opaque texels ignore ABR.
    if opacity&256!=0 {
        words[4]&=!(3<<21);
        // The nonzero opaque sentinel0x0001 must sample successfully, then
        // modulate to true black. Red127 maps its one red bit to zero; STP
        // remains clear so its coverage is opaque on every background.
        words[0]=(words[0]&!255)|127;
    }
    let opacity=opacity&255;
    if opacity<128 {
        // An opaque black source texel ignores ABR; use the admitted alternate
        // CLUT to make only its solid mask samples subtractive. Intermediate
        // alpha approximates B-alpha*white, not Unity's B*(1-alpha).
        // Alpha0 is culled before packet construction;128 retains exact source.
        let a=opacity as u32;
        words[0]=(words[0]&0xff00_0000)|a|(a<<8)|(a<<16);
        words[2]=(words[2]&0xffff)|((Clut::new(FADE_CLUT_X,FADE_CLUT_Y).uv_clut_word() as u32)<<16);
        words[4]=(words[4]&!(3<<21))|(2<<21);
    }
    words
}
fn uv_rect(t:&QuadTextured)->UvRect {
    let u=(t.uv0_clut&255) as u16;let v=((t.uv0_clut>>8)&255) as u16;
    UvRect {u,v,w:(t.uv1_tpage&255) as u16-u+1,h:((t.uv2>>8)&255) as u16-v+1}
}
#[inline(never)]
pub fn scenery(camera: (i32, i32), front: bool) -> u32 {
    let mut n = 0;
    unsafe {
        for (i, d) in DRAWS[..DRAW_COUNT].iter().enumerate() {
            if i&31==0 {crate::input::checkpoint();}
            if (d.flags&FRONT!=0) != front || d.flags&EMPTY_TEXTURE!=0 || VISIBLE[i / 32] & (1u32 << (i % 32)) == 0 || GAINS[i]==0 || OPACITIES[i]==0 {
                continue;
            }
            // Distant parallax layers can overflow the pre-shift i32 product.
            // The cooker proves the shifted values/subtractions fit i32.
            let cx = (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let cy = (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let mut v = [(0i32, 0i32); 4];
            for k in 0..4 {
                v[k] = (
                    160 + ((d.xy[k * 2] - cx) >> 8),
                    120 - ((d.xy[k * 2 + 1] - cy) >> 8),
                );
            }
            if v.iter().all(|p| p.0 < 0)
                || v.iter().all(|p| p.0 > 320)
                || v.iter().all(|p| p.1 < 0)
                || v.iter().all(|p| p.1 > 240)
            {
                continue;
            }
            let gain=GAINS[i] as u16;
            let tint=((d.tint.0 as u16*gain/128) as u8,(d.tint.1 as u16*gain/128) as u8,(d.tint.2 as u16*gain/128) as u8);
            let t=&TEMPLATES[d.texture];let uv=uv_rect(t);
            let opacity=OPACITIES[i] as u16 | if d.flags&BLACK_AVERAGE!=0 {256}else{0};
            if d.flags&LEGAL_EXTENT!=0 || scenery_geometry::legal(&v) {
                let xy=v.map(|p|(p.0 as i16,p.1 as i16));
                if d.flags&SOLID_BLACK!=0 && opacity&255==128 {
                    assert!(USED<CAP);PACKETS[USED].write_black(xy.map(vertex));USED+=1;n+=1;continue;
                }
                let words=quad_words(t,xy,tint,None,opacity);
                if let Some(scissors)=if d.flags&SCISSOR_CANDIDATE!=0 {SCISSOR_CACHE[i].map(xy,d.texture as u16,uv.w,uv.h,&COVERS[d.texture])}else{None} {
                    let extra=scissors.count.saturating_sub(1);
                    if extra<=EXTRA_ALLOWANCE-EXTRA_USED {
                        EXTRA_USED+=extra;
                        for rect in &scissors.rects[..scissors.count] {
                            assert!(USED<CAP);PACKETS[USED].write_scissored(words,*rect,FRAMEBUFFER_Y);USED+=1;
                        }
                        HK_SCISSOR_QUADS=HK_SCISSOR_QUADS.wrapping_add(scissors.count as u32);
                        HK_SCISSOR_SAVED_PIXELS=HK_SCISSOR_SAVED_PIXELS.wrapping_add(scissors.saved_pixels);
                        n+=scissors.count as u32;continue;
                    }
                    HK_SCISSOR_CAPACITY_FALLBACKS=HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
                }
                assert!(USED<CAP);PACKETS[USED].write_plain(words);USED+=1;n+=1;
            } else {
                n+=repair(t,v,uv,tint,opacity);
            }
        }
    }
    n
}
// Repair output is copied into DMA-owned packets before returning. No DMA or
// interrupt references this scratch; keeping it here avoids clearing1.5KiB
// and inflating the common scenery loop's stack for every oversized layer.
static mut REPAIR_CHILDREN:[Quad;64]=[Quad::ZERO;64];
#[inline(never)]
fn repair(t:&QuadTextured,v:[(i32,i32);4],uv:UvRect,tint:(u8,u8,u8),opacity:u16)->u32 {
    crate::input::checkpoint();
    unsafe {
        let children=&mut *(&raw mut REPAIR_CHILDREN);
        let count=match scenery_geometry::subdivide(v,uv,children) {
            Ok(count)=>count,
            Err(_)=>{HK_SCENERY_REPAIR_FAILURES=HK_SCENERY_REPAIR_FAILURES.wrapping_add(1);panic!("scenery subdivision");}
        };
        for child in &children[..count] {
            assert!(USED<CAP);PACKETS[USED].write_plain(quad_words(t,child.xy,tint,Some(child.uv),opacity));USED+=1;
        }
        HK_SCENERY_REPAIRS=HK_SCENERY_REPAIRS.wrapping_add(1);
        crate::input::checkpoint();
        count as u32
    }
}

pub fn submit(health:u16,max_health:u16,soul:u16,max_soul:u16,paused:bool,blue_health:u16) {
    crate::input::checkpoint();
    animation_cache::submit();
    // One owner, no mutation until main observes DMA and GPU completion.
    unsafe {
        let ot = &mut *(&raw mut OT);
        ot.clear();
        crate::dialogue::append(ot);
        crate::hud::append(ot,health,max_health,soul,max_soul,paused,blue_health);
        for i in (0..USED).rev() {
            if i&31==0 {crate::input::checkpoint();}
            let words=PACKETS[i].word_count();
            ot.add(0, &mut PACKETS[i], words);
        }
        crate::input::checkpoint();
        ot.submit_async();
        crate::input::checkpoint();
    }
}
