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
/// Packets are kicked progressively as their own DMA lists whenever the
/// channel is free, so the GPU starts within the first draws of a frame and
/// rasterises while the CPU builds the rest. Packets are append-only until the
/// frame ends, so an in-flight prefix `[0, KICKED)` is never touched afterwards.
const CHUNKS: usize = 64;
static mut CHUNK_OTS: [OrderingTable<1>; CHUNKS] = [const { OrderingTable::new() }; CHUNKS];
static mut CHUNK_COUNT: usize = 0;
static mut KICKED: usize = 0;
static mut USED: usize = 0;
/// Textures whose every texel is the opaque black word0x0001. Any draw of one
/// at full opacity is an exact flat opaque black quad whatever its tint or
/// blend mode: the STP-clear texel ignores ABR and black modulates to black.
static mut SOLID_TEXTURES: [u32; MAX_TEXTURES/32] = [0; MAX_TEXTURES/32];
/// Cooked largest all-opaque-black texel rectangle [x,y,w,h] per scene
/// texture (w==0: none). At full opacity that core is an exact flat opaque
/// black quad and hides everything drawn earlier beneath it.
static mut CORES: &[[u8;4]] = &[];
fn core_of(texture:usize)->[u8;4] {unsafe {CORES.get(texture).copied().unwrap_or([0;4])}}
const MAX_OCCLUDERS: usize = 8;
const OCCLUDER_MIN_AREA: i32 = 2048;
#[derive(Clone,Copy)]
struct Occluder {front: bool, draw: u16, rect: [i16;4]}
static mut OCCLUDERS: [Occluder; MAX_OCCLUDERS] = [Occluder {front:false,draw:0,rect:[0;4]}; MAX_OCCLUDERS];
static mut OCCLUDER_COUNT: usize = 0;
/// Frame pacing in scanlines (Timer1 counts HBlanks): time from the frame's
/// start to its first DMA kick, its final kick and its display-flip request.
#[no_mangle] pub static mut HK_FRAME_FIRST_KICK_LINES:u32=0;
#[no_mangle] pub static mut HK_FRAME_LAST_KICK_LINES:u32=0;
#[no_mangle] pub static mut HK_FRAME_FLIP_LINES:u32=0;
static mut FRAME_START_LINE:u16=0;
pub fn frame_lines()->u32 {psx_io::timers::counter(psx_io::timers::Timer::Timer1).wrapping_sub(unsafe {FRAME_START_LINE}) as u32}
pub fn mark_frame_start() {unsafe {FRAME_START_LINE=psx_io::timers::counter(psx_io::timers::Timer::Timer1);}}
#[no_mangle] pub static mut HK_CORE_QUADS:u32=0;
#[no_mangle] pub static mut HK_OCCLUSION_QUADS:u32=0;
#[no_mangle] pub static mut HK_OCCLUSION_SAVED_PIXELS:u32=0;
struct Pieces {rects: [[i16;4];4], count: usize}
fn rect_area(r:&[i16;4])->i32 {((r[2]-r[0]) as i32*(r[3]-r[1]) as i32).max(0)}
fn overlap(a:&[i16;4],b:&[i16;4])->i32 {
    let l=a[0].max(b[0]);let t=a[1].max(b[1]);let r=a[2].min(b[2]);let bt=a[3].min(b[3]);
    if l<r && t<bt {(r-l) as i32*(bt-t) as i32}else{0}
}
/// Subtract `hole` from every piece; None when nothing overlaps or the result
/// needs more than four rectangles (the scissor packet budget per draw).
fn subtract(p:&Pieces,hole:&[i16;4])->Option<Pieces> {
    let mut out=Pieces {rects:[[0;4];4],count:0};let mut changed=false;
    for piece in &p.rects[..p.count] {
        let l=piece[0].max(hole[0]);let t=piece[1].max(hole[1]);let r=piece[2].min(hole[2]);let b=piece[3].min(hole[3]);
        if !(l<r && t<b) {if out.count==4 {return None;} out.rects[out.count]=*piece;out.count+=1;continue;}
        changed=true;
        for part in [[piece[0],piece[1],piece[2],t],[piece[0],b,piece[2],piece[3]],[piece[0],t,l,b],[r,t,piece[2],b]] {
            if part[0]<part[2] && part[1]<part[3] {if out.count==4 {return None;} out.rects[out.count]=part;out.count+=1;}
        }
    }
    changed.then_some(out)
}
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
            SOLID_TEXTURES = [0;MAX_TEXTURES/32];
            CORES = crate::disc::scene_black_cores(bank);
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
            // Verify actual sampled words; nearly every texture exits on its
            // first texel, so this costs little at scene admission.
            if !t.is_streamed() && solid_black(room,t) {SOLID_TEXTURES[i/32]|=1<<(i%32);}
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
            if SOLID_TEXTURES[d.texture/32]&(1<<(d.texture%32))!=0 {d.flags|=SOLID_BLACK;}
        }
    }
    reset_visibility();
    crate::input::checkpoint();
}
/// Every texel samples the opaque black word0x0001 (source tilemap patches,
/// `black_solid` sprites). Exits at the first other word.
fn solid_black(room:&Room,t:Texture)->bool {
    let page=room.page(t.page as usize);
    let pal=&room.palettes()[t.palette as usize*32..][..32];
    (0..t.height as usize).all(|y|(0..t.width as usize).all(|x| {
        let u=t.u as usize+x;let v=t.v as usize+y;
        let index=(page[v*128+u/2]>>((u&1)*4))&15;
        u16_at(pal,index as usize*2)==1
    }))
}
/// Every later opaque black rectangle of this frame: whole quads of solid
/// textures and the cores of black masks, both at full opacity. Draw order is
/// every back draw in index order, then every front draw.
#[inline(never)]
fn collect_occluders(camera:(i32,i32)) {
    unsafe {
        OCCLUDER_COUNT=0;
        for (i,d) in DRAWS[..DRAW_COUNT].iter().enumerate() {
            if i&31==0 {crate::input::checkpoint();}
            // Front-layer occluders hide every back draw, which is where the
            // pixels are; the scan stays cheap enough to precede the first kick.
            if d.flags&(FRONT|EMPTY_TEXTURE)!=FRONT || VISIBLE[i/32]&(1u32<<(i%32))==0 || GAINS[i]==0 || OPACITIES[i]!=128 {continue;}
            let solid=d.flags&SOLID_BLACK!=0;let core=core_of(d.texture);
            if !solid && core[2]==0 {continue;}
            let cx=(((camera.0>>8) as i64*d.scale as i64)>>12) as i32;let cy=(((camera.1>>8) as i64*d.scale as i64)>>12) as i32;
            let v:[(i32,i32);4]=core::array::from_fn(|k|(160+((d.xy[k*2]-cx)>>8),120-((d.xy[k*2+1]-cy)>>8)));
            if v[0].1!=v[1].1 || v[0].0!=v[2].0 || v[1].0!=v[3].0 || v[2].1!=v[3].1 {continue;}
            let bw=(v[0].0.max(v[1].0).min(320)-v[0].0.min(v[1].0).max(0)).max(0);let bh=(v[0].1.max(v[2].1).min(240)-v[0].1.min(v[2].1).max(0)).max(0);
            if bw*bh<OCCLUDER_MIN_AREA*2 {continue;}
            if !(d.flags&LEGAL_EXTENT!=0 || scenery_geometry::legal(&v)) {continue;}
            let xy=v.map(|p|(p.0 as i16,p.1 as i16));
            let rect=if solid {
                let l=xy[0].0.min(xy[1].0).max(0);let r=xy[0].0.max(xy[1].0).min(320);let t=xy[0].1.min(xy[2].1).max(0);let b=xy[0].1.max(xy[2].1).min(240);
                if !(l<r && t<b) {continue;} [l,t,r,b]
            } else {
                let uv=uv_rect(&TEMPLATES[d.texture]);
                match alpha_scissor_cache::texel_rect_to_screen(xy,uv.w,uv.h,core) {Some(r)=>r,None=>continue}
            };
            let area=rect_area(&rect);
            if area<OCCLUDER_MIN_AREA {continue;}
            // Keep the largest rectangles: replace the smallest when full.
            let slot=if OCCLUDER_COUNT<MAX_OCCLUDERS {OCCLUDER_COUNT+=1;OCCLUDER_COUNT-1} else {
                let (k,smallest)=OCCLUDERS.iter().enumerate().map(|(k,o)|(k,rect_area(&o.rect))).min_by_key(|&(_,a)|a).unwrap();
                if smallest>=area {continue;} k
            };
            OCCLUDERS[slot]=Occluder {front:d.flags&FRONT!=0,draw:i as u16,rect};
        }
    }
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
        USED = 0;KICKED = 0;CHUNK_COUNT = 0;
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
    if !front {collect_occluders(camera);}
    unsafe {
        for (i, d) in DRAWS[..DRAW_COUNT].iter().enumerate() {
            if i&31==0 {crate::input::checkpoint();}
            if i&7==0 {kick_ready();}
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
                if d.flags&SOLID_BLACK!=0 && opacity&255==128 && opacity&256!=0 {
                    assert!(USED<CAP);PACKETS[USED].write_black(xy.map(vertex));USED+=1;n+=1;continue;
                }
                let words=quad_words(t,xy,tint,None,opacity);
                let scissors=if d.flags&SCISSOR_CANDIDATE!=0 {SCISSOR_CACHE[i].map(xy,d.texture as u16,uv.w,uv.h,&COVERS[d.texture])}else{None};
                // Flat core and single-rectangle occlusion: exact partitions of
                // the original pixels, bounded to four scissor packets per draw.
                // Draw-area clipping is proven pixel-exact for axis-aligned
                // quads only; a flat black core is exact only on the
                // black-average path, which zeroes the sentinel texel's red bit.
                let axis=xy[0].1==xy[1].1 && xy[0].0==xy[2].0 && xy[1].0==xy[3].0 && xy[2].1==xy[3].1;
                let box_area=if axis {(xy[0].0.max(xy[1].0).min(320)-xy[0].0.min(xy[1].0).max(0)) as i32*(xy[0].1.max(xy[2].1).min(240)-xy[0].1.min(xy[2].1).max(0)) as i32}else{0};
                let mut pieces=match &scissors {
                    Some(s)=>Pieces {rects:s.rects,count:s.count},
                    None=>{
                        let l=xy[0].0.min(xy[1].0).max(0);let r=xy[0].0.max(xy[1].0).min(320);let tp=xy[0].1.min(xy[2].1).max(0);let b=xy[0].1.max(xy[2].1).min(240);
                        Pieces {rects:[[l,tp,r,b],[0;4],[0;4],[0;4]],count:usize::from(l<r && tp<b)}
                    }
                };
                let mut core_rect=None;let mut saved=0i32;
                let core=if axis && box_area>=OCCLUDER_MIN_AREA {core_of(d.texture)}else{[0;4]};
                if opacity&255==128 && opacity&256!=0 && core[2]!=0 {
                    if let Some(core)=alpha_scissor_cache::texel_rect_to_screen(xy,uv.w,uv.h,core) {
                        if rect_area(&core)>=256 {if let Some(p)=subtract(&pieces,&core) {pieces=p;core_rect=Some(core);}}
                    }
                }
                let mut best:Option<(i32,usize)>=None;
                for (k,o) in OCCLUDERS[..if axis && box_area>=OCCLUDER_MIN_AREA {OCCLUDER_COUNT}else{0}].iter().enumerate() {
                    if !((o.front && !front) || (o.front==front && o.draw as usize>i)) {continue;}
                    let ov:i32=pieces.rects[..pieces.count].iter().map(|p|overlap(p,&o.rect)).sum();
                    if ov>=256 && best.is_none_or(|b|ov>b.0) {best=Some((ov,k));}
                }
                if let Some((ov,k)) = best {
                    if let Some(p)=subtract(&pieces,&OCCLUDERS[k].rect) {pieces=p;saved=ov;}
                }
                if core_rect.is_some() || saved>0 {
                    let extra=(pieces.count+usize::from(core_rect.is_some())).saturating_sub(1);
                    if extra<=EXTRA_ALLOWANCE-EXTRA_USED {
                        EXTRA_USED+=extra;
                        for rect in &pieces.rects[..pieces.count] {
                            assert!(USED<CAP);PACKETS[USED].write_scissored(words,*rect,FRAMEBUFFER_Y);USED+=1;
                        }
                        if let Some(c)=core_rect {
                            let q=[(c[0],c[1]),(c[2],c[1]),(c[0],c[3]),(c[2],c[3])];
                            assert!(USED<CAP);PACKETS[USED].write_black(q.map(vertex));USED+=1;
                            HK_CORE_QUADS=HK_CORE_QUADS.wrapping_add(1);
                        }
                        if saved>0 {HK_OCCLUSION_QUADS=HK_OCCLUSION_QUADS.wrapping_add(1);HK_OCCLUSION_SAVED_PIXELS=HK_OCCLUSION_SAVED_PIXELS.wrapping_add(saved as u32);}
                        n+=pieces.count as u32+u32::from(core_rect.is_some());continue;
                    }
                    HK_SCISSOR_CAPACITY_FALLBACKS=HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
                }
                if let Some(scissors)=scissors {
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
    kick_ready();
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

/// Kick every packet built since the last kick as its own DMA list if the
/// channel is free. Never waits: a busy channel just leaves the packets for a
/// later call or the final list, so the CPU stays ahead of the GPU.
pub fn kick_ready() {
    unsafe {
        if USED==KICKED || CHUNK_COUNT==CHUNKS || dma_pending() {return;}
        let ot = &mut CHUNK_OTS[CHUNK_COUNT];
        ot.clear();
        for i in (KICKED..USED).rev() {
            let words=PACKETS[i].word_count();
            ot.add(0, &mut PACKETS[i], words);
        }
        ot.submit_async();
        if CHUNK_COUNT==0 {HK_FRAME_FIRST_KICK_LINES=frame_lines();}
        KICKED=USED;CHUNK_COUNT+=1;
    }
}
/// Build the final list (everything not yet kicked, then HUD and dialogue on
/// top). It is not kicked here: an earlier chunk may still be walking, and
/// main kicks it through `kick_front` once the channel is idle so the wait
/// runs simulation ticks and input checkpoints instead of spinning.
pub fn submit(health:u16,max_health:u16,soul:u16,max_soul:u16,paused:bool,blue_health:u16) {
    crate::input::checkpoint();
    animation_cache::submit();
    // One owner, no mutation until main observes DMA and GPU completion.
    unsafe {
        let ot = &mut *(&raw mut OT);
        ot.clear();
        crate::dialogue::append(ot);
        crate::hud::append(ot,health,max_health,soul,max_soul,paused,blue_health);
        for i in (KICKED..USED).rev() {
            if i&31==0 {crate::input::checkpoint();}
            let words=PACKETS[i].word_count();
            ot.add(0, &mut PACKETS[i], words);
        }
        KICKED=USED;
    }
    crate::input::checkpoint();
}
/// Call only after `dma_pending()` returned false for the last kicked chunk.
pub fn kick_front() {
    unsafe {(&*(&raw const OT)).submit_async();HK_FRAME_LAST_KICK_LINES=frame_lines();if CHUNK_COUNT==0 {HK_FRAME_FIRST_KICK_LINES=HK_FRAME_LAST_KICK_LINES;}}
}
