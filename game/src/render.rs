//! Resident scenery plus bounded animation residency and SDK DMA packets.
use hk_format::coverage::CoverageView;
use crate::animation_cache;
use hk_format::{i32_at, u16_at, u32_at, Room, Texture};
use psx_gpu::{self as gpu, ot::OrderingTable, prim::QuadTextured};
use psx_vram::{Clut, TexDepth, Tpage};
use crate::{alpha_scissor_cache,draw_packet::Packet,scenery_geometry::{self,UvRect,Quad}};
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/scenery_budgets.rs"));
#[path="tile_coverage.rs"] mod tile_coverage;
#[path="opaque_groups.rs"] mod opaque_groups;
#[path="tile_events.rs"] mod tile_events;
#[path="tile_frame_cache.rs"] mod tile_frame_cache;
#[path="back_prebuild.rs"] mod back_prebuild;
#[path="occluder_snapshot.rs"] mod occluder_snapshot;
#[path="core_map_cache.rs"] mod core_map_cache;
#[path="draw_visibility.rs"] mod draw_visibility;
#[path="scenery_color.rs"] mod scenery_color;
pub use draw_visibility::{reset_visibility,set_visible,set_gain,set_opacity};
use draw_visibility::DRAW_ENABLED;
static mut CORE_MAP_CACHE:core_map_cache::Cache=core_map_cache::Cache::new();
// Maximum scenery records plus the Knight and the active nail effect.
const CAP: usize = 1032;
// Final-list ownership matches HUD/dialogue: not mutated until DMA completes.
static mut TRANSITION:gpu::prim::QuadGouraudBlended=gpu::prim::QuadGouraudBlended::new(
    [(0,0),(320,0),(0,240),(320,240)],[(0,0,0);4],gpu::material::BlendMode::Subtract);
const MAX_DRAWS:usize=crate::disc::SCENE_DRAW_CAPACITY;
const _:()=assert!(MAX_DRAWS<=1024);
static mut TILE_DRAWS:[(u16,u16);MAX_DRAWS]=[(0,0);MAX_DRAWS];
static mut TILE_DRAW_COUNT:usize=0;
const MAX_TILE_GROUPS:usize=32;
static mut TILE_GROUPS:[opaque_groups::Group;MAX_TILE_GROUPS]=[opaque_groups::Group{certificate:0,members:[u16::MAX;4]};MAX_TILE_GROUPS];
static mut TILE_GROUP_COUNT:usize=0;
static mut TILE_MAX_RANK:u16=0;
static mut TILE_EVENTS:tile_events::Events<MAX_DRAWS>=tile_events::Events::new();
static mut TILE_CERTIFIED:[u32;MAX_DRAWS/32]=[0;MAX_DRAWS/32];
static mut TILE_GENERATION:u32=0;
static mut TILE_ELIGIBILITY_DIRTY:bool=false;
static mut TILE_PREPARED:tile_frame_cache::Prepared=tile_frame_cache::Prepared::new();
static mut BACK_PREFIX:back_prebuild::Prefix=back_prebuild::Prefix::new();
// Valid only with BACK_PREFIX. Save initialized entries, never scratch padding
// or uninitialized bool fields; begin clears scratch before a hit restores it.
static mut PREFIX_OCCLUDERS:occluder_snapshot::Snapshot<Occluder,MAX_OCCLUDERS>=occluder_snapshot::Snapshot::new();
#[cfg(target_arch="mips")]
const _:()=assert!(core::mem::size_of::<occluder_snapshot::Snapshot<Occluder,MAX_OCCLUDERS>>()==108);
static mut PREFIX_TEXTURES:[u32;MAX_TEXTURES/32]=[0;MAX_TEXTURES/32];
static mut BACK_PREBUILD_ALLOWED:bool=false;
#[no_mangle]pub static mut HK_BACK_PREBUILDS:u32=0;
#[no_mangle]pub static mut HK_BACK_PREBUILD_HITS:u32=0;
#[no_mangle]pub static mut HK_BACK_PREBUILD_MISSES:u32=0;
#[no_mangle]pub static mut HK_BACK_PREBUILD_PACKETS:u32=0;
#[no_mangle]pub static mut HK_BACK_PREBUILD_LINES:u32=0;
#[no_mangle]pub static mut HK_TILE_PREPARES:u32=0;
#[no_mangle]pub static mut HK_TILE_PREPARE_HITS:u32=0;
#[no_mangle]pub static mut HK_TILE_PREPARE_MISSES:u32=0;
#[no_mangle]pub static mut HK_TILE_COVERAGE_CANDIDATES:u32=0;
#[no_mangle]pub static mut HK_TILE_COVERAGE_LOOKUPS:u32=0;
#[no_mangle]pub static mut HK_TILE_COVERAGE_TILES:u32=0;
#[no_mangle]pub static mut HK_TILE_OCCLUSION_QUADS:u32=0;
#[no_mangle]pub static mut HK_TILE_SAVED_PIXELS:u32=0;
#[no_mangle]pub static mut HK_TILE_FALLBACKS:u32=0;
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
    color: u32,
}
#[cfg(target_arch="mips")]
const _:()=assert!(core::mem::size_of::<Draw>()==48);
static mut DRAWS: [Draw; MAX_DRAWS] = [Draw {
    xy: [0; 8],
    scale: 0,
    flags: 0,
    texture: 0,
    tint: (0, 0, 0),
    color: 0,
}; MAX_DRAWS];
static mut DRAW_BOUNDS:[u8;MAX_DRAWS]=[0;MAX_DRAWS];
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
// A colored STP-clear texel also hides earlier draws, but must retain its
// texture/color when drawn. This table is never used for flat black rendering.
static mut OPAQUE_CORES: &[[u8;4]] = &[];
fn opaque_core_of(texture:usize)->[u8;4] {unsafe {OPAQUE_CORES.get(texture).copied().unwrap_or([0;4])}}
fn core_of(texture:usize)->[u8;4] {unsafe {CORES.get(texture).copied().unwrap_or([0;4])}}
const OCCLUDER_MIN_AREA: i32 = 2048;
// Immutable eligibility is established once per region. Dynamic visibility,
// gain and opacity are still checked before each frame's occluder selection.
static mut OCCLUDER_DRAWS:[u16;MAX_DRAWS]=[0;MAX_DRAWS];
static mut OCCLUDER_DRAW_COUNT:usize=0;
// Fully opaque rotated back masks use their actual convex-quad interior.
static mut SOLID_CORE_DRAWS:[u16;MAX_DRAWS]=[0;MAX_DRAWS];
static mut SOLID_CORE_DRAW_COUNT:usize=0;
static mut SOLID_CORE_CACHE:solid_occluder::Cache=solid_occluder::Cache::new();

#[no_mangle]pub static mut HK_SOLID_CORE_CACHE_HITS:u32=0;
#[no_mangle]pub static mut HK_SOLID_CORE_CACHE_MISSES:u32=0;
#[no_mangle]pub static mut HK_SOLID_CORE_OCCLUDERS:u32=0;

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
#[path="scenery_occlusion.rs"]
mod occlusion;
#[path="solid_occluder.rs"]
mod solid_occluder;
#[path="occluder_admission.rs"]
mod occluder_admission;
use occlusion::{Pieces,rect_area,subtract};
#[path="occlusion_scratch.rs"]
mod occlusion_scratch;
use occlusion_scratch::{Storage as Scratch,Occluder,Occluders,MAX_OCCLUDERS};
#[path="scenery_bounds.rs"]
mod scenery_bounds;
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
// A count of255 means the legacy pack has no precomputed alpha cover.
// init fills every active entry before drawing; zero initialization keeps this
// working storage out of the EXE's on-disc data payload.
static mut COVERS: [[u8;20];MAX_TEXTURES] = [[0;20];MAX_TEXTURES];
// One entry per admitted draw. Geometry spans may change by one pixel with camera
// rounding; the entry key captures both signed spans and the texture identity.
static mut SCISSOR_CACHE:[alpha_scissor_cache::Entry;MAX_DRAWS]=[alpha_scissor_cache::Entry::EMPTY;MAX_DRAWS];
static mut SCISSOR_SECONDARY:alpha_scissor_cache::Secondary=alpha_scissor_cache::Secondary::EMPTY;
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
/// Called after final GPU/DMA retirement, before any scene/atlas replacement.
pub fn release_scene() {
    assert!(!dma_pending()&&!psx_rt::interrupts::gp1_queue_pending());
    unsafe {
        (&mut *(&raw mut BACK_PREFIX)).invalidate();
        (&mut *(&raw mut TILE_PREPARED)).invalidate();
        BACK_PREBUILD_ALLOWED=false;TEXTURE_COUNT=0;DRAW_COUNT=0;
        TILE_DRAW_COUNT=0;TILE_GROUP_COUNT=0;TILE_MAX_RANK=0;TILE_GENERATION=TILE_GENERATION.wrapping_add(1);
    }
    animation_cache::init();
}
#[inline(never)]
pub fn init(room: &Room, bank: usize, region:usize,coverage:CoverageView<'_>) {
    crate::input::checkpoint();
    unsafe {
        (&mut *(&raw mut BACK_PREFIX)).invalidate();
        BACK_PREBUILD_ALLOWED=false;
        (&mut *(&raw mut TILE_PREPARED)).invalidate();
        TILE_GENERATION=TILE_GENERATION.wrapping_add(1);
        TILE_ELIGIBILITY_DIRTY=false;TILE_CERTIFIED=[0;MAX_DRAWS/32];
        let scene_changed=TEXTURE_COUNT==0 || ACTIVE_BANK!=bank;
        if scene_changed {animation_cache::init();}
        ACTIVE_BANK = bank;
        assert!(room.counts[1]<=MAX_TEXTURES);
        DRAW_COUNT = room.counts[2];
        assert!(DRAW_COUNT<=MAX_DRAWS);
        TEXTURE_COUNT = room.counts[1];
        // The cooker bounds every mandatory child, even if all source draws
        // are visible. Optional scissors can only spend the remaining capacity.
        EXTRA_ALLOWANCE=CAP.checked_sub(DYNAMIC_PACKET_RESERVE+SCENERY_PACKET_BUDGETS[region] as usize).expect("scenery packet budget");
        if scene_changed {
            // Texture IDs, dimensions and covers are immutable within a scene
            // bank. Cache keys already include exact texture and signed spans,
            // so region changes can retain matching entries. Clear every slot
            // on a bank change, including slots beyond the new draw count.
            for entry in &mut SCISSOR_CACHE {entry.invalidate();}
            (&mut *(&raw mut SCISSOR_SECONDARY)).invalidate();
            STREAMED = [0;MAX_TEXTURES/32];
            BLACK_MASKS = [0;MAX_TEXTURES/32];
            SOLID_TEXTURES = [0;MAX_TEXTURES/32];
            CORES = crate::disc::scene_black_cores(bank);
            OPAQUE_CORES = crate::disc::scene_opaque_cores(bank);
        }
        if !FADE_CLUT_READY {
            // First room only; FIFO upload owns static bytes and precedes any
            // room packets. No per-frame transfer or DMA lifetime is added.
            gpu::draw_sync();
            psx_vram::upload_bytes(psx_vram::VramRect::new(FADE_CLUT_X,FADE_CLUT_Y,16,1),&FADE_CLUT);
            FADE_CLUT_READY=true;
        }
        draw_visibility::begin_region();
        for i in 0..DRAW_COUNT {
            if i&31==0 {crate::input::checkpoint();}
            let d = room.draw(i);
            let xy=core::array::from_fn(|k| i32_at(d, 8 + k * 4));
            DRAW_BOUNDS[i]=scenery_bounds::local_indices(&xy);
            DRAWS[i] = Draw {
                xy,
                scale: u32_at(d, 4) as i32,
                flags: DRAW_ENABLED | u8::from(u16_at(d,2)!=0)*FRONT | u8::from(scenery_geometry::legal_extent_q8(&xy))*LEGAL_EXTENT | u8::from(d[43]==1)*BLACK_AVERAGE,
                texture: u16_at(d, 0) as usize,
                tint: (d[40], d[41], d[42]),
                color: scenery_color::command(0,(d[40],d[41],d[42]),128,128|if d[43]==1 {256}else{0}),
            };
        }
        if scene_changed {
        for i in 0..room.counts[1] {
            if i&31==0 {crate::input::checkpoint();}
            let t = room.texture(i);
            let flags=crate::disc::scene_texture_flags(bank,i);
            if !t.is_streamed() && flags&1!=0 {
                BLACK_MASKS[i/32]|=1<<(i%32);
            }
            // Flags are derived from the exact final atlas words before its
            // startup upload. Compact resident scenes keep no duplicate texels.
            if !t.is_streamed() && flags&2!=0 {SOLID_TEXTURES[i/32]|=1<<(i%32);}
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
        PREFIX_TEXTURES.fill(0);BACK_PREBUILD_ALLOWED=true;
        for d in &DRAWS[..DRAW_COUNT] {
            if d.flags&FRONT==0 {
                PREFIX_TEXTURES[d.texture/32]|=1<<(d.texture%32);
                if STREAMED[d.texture/32]&(1<<(d.texture%32))!=0 {BACK_PREBUILD_ALLOWED=false;}
            }
        }
        // Bind shared immutable draw identities once per region. Sorting by
        // (front, draw) lets the frame collector visit latest sources first.
        TILE_DRAW_COUNT=0;let certs=coverage.pool_map();
        for front in [false,true] {
            for i in 0..DRAW_COUNT {
                let d=&DRAWS[i];
                if (d.flags&FRONT!=0)!=front || STREAMED[d.texture/32]&(1<<(d.texture%32))!=0 {continue;}
                if let Some(pool)=room.draw_pool_index(i) {
                    if let Some(&cert)=certs.get(pool) {
                        if cert!=tile_coverage::NO_CERT {
                            TILE_DRAWS[TILE_DRAW_COUNT]=(i as u16,cert);TILE_DRAW_COUNT+=1;
                            TILE_CERTIFIED[i/32]|=1<<(i%32);
                            // Saved tile rows also depend on foreground proofs
                            // outside the rectangular occluder candidate list.
                            PREFIX_TEXTURES[d.texture/32]|=1<<(d.texture%32);
                        }
                    }
                }
            }
        }
        // Immutable cover/axis tests belong to activation, not every rendered
        // frame. A rejected candidate still draws its original complete quad.
        OCCLUDER_DRAW_COUNT=0;SOLID_CORE_DRAW_COUNT=0;
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
            if d.flags&FRONT!=0 && (d.flags&SOLID_BLACK!=0 || opaque_core_of(d.texture)[2]!=0)
                && occluder_admission::possible(&d.xy,OCCLUDER_MIN_AREA) {
                OCCLUDER_DRAWS[OCCLUDER_DRAW_COUNT]=i as u16;
                OCCLUDER_DRAW_COUNT+=1;
                // A prefix also depends on later foreground opacity proofs.
                // Any future template rebind must invalidate that saved work.
                PREFIX_TEXTURES[d.texture/32]|=1<<(d.texture%32);
            }
            if d.flags&(FRONT|SOLID_BLACK)==SOLID_BLACK && !axis {
                // Optimization admission only: smaller/unproven geometry still
                // renders normally. Full opacity remains a live per-frame test.
                let w=(*xy.iter().step_by(2).max().unwrap() as i64-*xy.iter().step_by(2).min().unwrap() as i64+255)>>8;
                let h=(*xy.iter().skip(1).step_by(2).max().unwrap() as i64-*xy.iter().skip(1).step_by(2).min().unwrap() as i64+255)>>8;
                if w.min(320)*h.min(240)>=8192 {SOLID_CORE_DRAWS[SOLID_CORE_DRAW_COUNT]=i as u16;SOLID_CORE_DRAW_COUNT+=1;}
            }


        }
        bind_tile_groups(room,coverage);
    }
    reset_visibility();
    crate::input::checkpoint();
}
/// Groups are valid only when every original member is in this region. The
/// bounded table adds optional proofs; omitted groups retain normal rendering.
fn bind_tile_groups(room:&Room,coverage:CoverageView<'_>) {
    unsafe {
        TILE_GROUP_COUNT=0;
        // One inverse map replaces repeated group×member×draw searches at
        // region activation. Duplicate pool references preserve the first local
        // occurrence, exactly matching the original membership resolver.
        const _:()=assert!(opaque_groups::GROUP_POOL_CAPACITY<=4096);
        let mut pool_to_local=[u16::MAX;opaque_groups::GROUP_POOL_CAPACITY];
        for i in 0..DRAW_COUNT {
            if i&31==0 {crate::input::checkpoint();}
            if let Some(pool)=room.draw_pool_index(i) {
                if let Some(slot)=pool_to_local.get_mut(pool) {
                    if *slot==u16::MAX {*slot=i as u16;}
                }
            }
        }
        for group in coverage.groups() {
            crate::input::checkpoint();
            let Some(local)=opaque_groups::resolve_indexed(group,&pool_to_local)else{continue;};
            let members=local.members.iter().copied().take_while(|&i|i!=u16::MAX);
            if !members.clone().all(|i| {
                let d=&DRAWS[i as usize];
                d.flags&(FRONT|SOLID_BLACK|BLACK_AVERAGE)==(FRONT|SOLID_BLACK|BLACK_AVERAGE)
                    && STREAMED[d.texture/32]&(1<<(d.texture%32))==0
            }) {continue;}
            if TILE_GROUP_COUNT==MAX_TILE_GROUPS {break;}
            for i in members {
                let i=i as usize;let texture=DRAWS[i].texture;
                TILE_CERTIFIED[i/32]|=1<<(i%32);
                PREFIX_TEXTURES[texture/32]|=1<<(texture%32);
            }
            TILE_GROUPS[TILE_GROUP_COUNT]=local;TILE_GROUP_COUNT+=1;
        }
    }
}

/// Every later opaque rectangle of this frame: whole quads of solid black
/// textures and nonzero STP-clear cores of any texture, at full opacity. Draw order is
/// every back draw in index order, then every front draw.
#[inline(never)]
fn collect_occluders(camera:(i32,i32)) {
    unsafe {
        let scratch=occlusion_scratch::base();
        let occluders=&mut *Scratch::occluders(scratch);occluders.clear();
        for candidate in 0..OCCLUDER_DRAW_COUNT {
            if candidate&31==0 {crate::input::checkpoint();}
            let i=OCCLUDER_DRAWS[candidate] as usize;let d=&DRAWS[i];
            // Admission proved nonempty art with an opaque core.
            // Gameplay may still hide or fade it, so check the live state.
            if d.flags&DRAW_ENABLED==0 || OPACITIES[i]!=128 {continue;}
            let solid=d.flags&SOLID_BLACK!=0;let core=opaque_core_of(d.texture);
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
                match (&mut *(&raw mut CORE_MAP_CACHE)).map(xy,uv.w,uv.h,core) {Some(r)=>r,None=>continue}
            };
            let area=rect_area(&rect);
            if area<OCCLUDER_MIN_AREA {continue;}
            // Keep the largest rectangles: replace the smallest when full.
            let slot=if occluders.len()<MAX_OCCLUDERS {occluders.len()} else {
                let (k,smallest)=occluders.as_slice().iter().enumerate().map(|(k,o)|(k,rect_area(&o.rect))).min_by_key(|&(_,a)|a).unwrap();
                if smallest>=area {continue;} k
            };
            occluders.set(slot,Occluder {front:d.flags&FRONT!=0,draw:i as u16,rect});
        }
        collect_solid_core(camera,occluders);
        let union=&mut *Scratch::union(scratch);
        *union=[i16::MAX,i16::MAX,i16::MIN,i16::MIN];
        for o in occluders.as_slice() {
            *union=[union[0].min(o.rect[0]),union[1].min(o.rect[1]),union[2].max(o.rect[2]),union[3].max(o.rect[3])];
        }
    }
}
/// Add one conservative interior of a later opaque back mask. Source-order
/// tests in both occlusion paths restrict it to earlier back draws. Existing
/// foreground occluders keep their slots. No rendering geometry is altered.
#[inline(never)]
fn collect_solid_core(camera:(i32,i32),occluders:&mut Occluders) {
    unsafe {
        if occluders.len()==MAX_OCCLUDERS || SOLID_CORE_DRAW_COUNT==0 {return;}
        let cache=&mut *(&raw mut SOLID_CORE_CACHE);
        let mut best=None;let mut score=OCCLUDER_MIN_AREA-1;
        for candidate in 0..SOLID_CORE_DRAW_COUNT {
            let i=SOLID_CORE_DRAWS[candidate]as usize;let d=&DRAWS[i];
            if d.flags&DRAW_ENABLED==0 || OPACITIES[i]!=128 {continue;}
            let cx=(((camera.0>>8)as i64*d.scale as i64)>>12)as i32;let cy=(((camera.1>>8)as i64*d.scale as i64)>>12)as i32;
            let v=core::array::from_fn::<_,4,_>(|k|(160+((d.xy[k*2]-cx)>>8),120-((d.xy[k*2+1]-cy)>>8)));
            let l=v.iter().map(|p|p.0).min().unwrap().max(0);let r=v.iter().map(|p|p.0).max().unwrap().min(320);
            let t=v.iter().map(|p|p.1).min().unwrap().max(0);let b=v.iter().map(|p|p.1).max().unwrap().min(240);
            if l>=r || t>=b || (r-l)*(b-t)<8192 {continue;}
            // This is exactly the original packet path. Oversized repair quads
            // remain untouched; there is no subdivision cost in this collector.
            if !(d.flags&LEGAL_EXTENT!=0 || scenery_geometry::legal(&v)) {continue;}
            if let Some(rect)=cache.map(v.map(|(x,y)|(x as i16,y as i16))) {
                let area=rect_area(&rect);if area>score {score=area;best=Some((i,rect));}
            }
        }
        if let Some((i,rect))=best {
            occluders.set(occluders.len(),Occluder{front:false,draw:i as u16,rect});
            HK_SOLID_CORE_OCCLUDERS=HK_SOLID_CORE_OCCLUDERS.wrapping_add(1);
        }
        let (hits,misses)=cache.stats();HK_SOLID_CORE_CACHE_HITS=hits;HK_SOLID_CORE_CACHE_MISSES=misses;
    }
}
/// Translate host-certified masks; the grid owns no GPU or DMA references.
#[inline(never)]
fn collect_tile_coverage(camera:(i32,i32),coverage:CoverageView<'_>) {
    unsafe {
        let grid=&mut *Scratch::tile_owners(occlusion_scratch::base());
        let certs=coverage.tile_certs();let bits=coverage.tile_bits();
        let group_certs=coverage.group_certs();let group_bits=coverage.group_bits();
        TILE_MAX_RANK=0;
        let(mut candidates,mut lookups,mut tiles)=(0u32,0u32,0u32);
        for candidate in (0..TILE_DRAW_COUNT).rev() {
            if candidate&31==0 {crate::input::checkpoint();}
            let (draw,cert)=TILE_DRAWS[candidate];let i=draw as usize;let d=&DRAWS[i];
            if d.flags&DRAW_ENABLED==0 || OPACITIES[i]!=128 {continue;}
            let cx=(((camera.0>>8)as i64*d.scale as i64)>>12)as i32;
            let cy=(((camera.1>>8)as i64*d.scale as i64)>>12)as i32;
            let origin=(160+((d.xy[0]-cx)>>8),120-((d.xy[1]-cy)>>8));
            if !tile_coverage::intersects(certs,cert,origin) {continue;}
            // Cropped certificate bounds lie inside every phase's source
            // AABB. Its tile preflight proves viewport overlap, satisfying
            // LEGAL_EXTENT's absolute-coordinate and triangle-edge contract.
            if d.flags&LEGAL_EXTENT==0 {
                let v=core::array::from_fn::<_,4,_>(|k|(160+((d.xy[k*2]-cx)>>8),120-((d.xy[k*2+1]-cy)>>8)));
                if !scenery_geometry::legal(&v) {continue;}
            }
            let rank=tile_coverage::rank(i,d.flags&FRONT!=0);candidates+=1;
            let (added,reads)=tile_coverage::claim(certs,bits,cert,origin,rank,grid);
            if added!=0 && TILE_MAX_RANK==0 {TILE_MAX_RANK=rank;}
            tiles+=added;lookups+=reads;
        }
        // Joint seam coverage remains valid only until its earliest member.
        // Upgrade earlier owners without displacing any later single proof.
        for group in &TILE_GROUPS[..TILE_GROUP_COUNT] {
            crate::input::checkpoint();
            let first=&DRAWS[group.members[0]as usize];
            let cx=(((camera.0>>8)as i64*first.scale as i64)>>12)as i32;
            let cy=(((camera.1>>8)as i64*first.scale as i64)>>12)as i32;
            let origin=(160+((first.xy[0]-cx)>>8),120-((first.xy[1]-cy)>>8));
            if !opaque_groups::intersects(group_certs,group.certificate,origin){continue;}
            let mut guard=opaque_groups::Guard::new();
            for i in group.members.iter().copied().take_while(|&i|i!=u16::MAX) {
                let i=i as usize;let d=&DRAWS[i];
                let eligible=d.flags&(DRAW_ENABLED|FRONT|SOLID_BLACK|BLACK_AVERAGE)==(DRAW_ENABLED|FRONT|SOLID_BLACK|BLACK_AVERAGE)
                    && OPACITIES[i]==128 && d.scale==first.scale
                    && STREAMED[d.texture/32]&(1<<(d.texture%32))==0;
                let v=core::array::from_fn(|k|(160+((d.xy[k*2]-cx)>>8),120-((d.xy[k*2+1]-cy)>>8)));
                guard.push(tile_coverage::rank(i,true),d.scale,eligible,&v);
            }
            if let Some((origin,rank))=guard.finish() {
                let (added,reads)=opaque_groups::claim(group_certs,group_bits,group.certificate,origin,rank,grid);
                if added!=0 {TILE_MAX_RANK=TILE_MAX_RANK.max(rank);}
                candidates+=1;tiles+=added;lookups+=reads;
            }
        }
        (&mut *(&raw mut TILE_EVENTS)).build(grid,&mut *Scratch::tile_rows(occlusion_scratch::base()));
        HK_TILE_COVERAGE_CANDIDATES=HK_TILE_COVERAGE_CANDIDATES.wrapping_add(candidates);
        HK_TILE_COVERAGE_LOOKUPS=HK_TILE_COVERAGE_LOOKUPS.wrapping_add(lookups);
        HK_TILE_COVERAGE_TILES=HK_TILE_COVERAGE_TILES.wrapping_add(tiles);
    }
}
/// Called after both scenery passes and all current frame packet emission.
/// Only CPU scratch/events change; prior DMA packets and VRAM remain untouched.
/// Any later certified eligibility change or camera change rejects this result.
#[inline(never)]
pub fn prepare_tile_coverage(camera:(i32,i32),coverage:CoverageView<'_>) {
    unsafe {
        if (&*(&raw const TILE_PREPARED)).matches(camera,TILE_GENERATION) {return;}
        let scratch=occlusion_scratch::base();
        // Switch the exclusive workspace to owner collection; claim requires0.
        Scratch::initialize_owners_at(scratch);
        collect_tile_coverage(camera,coverage);
        // Events and rows now own all coverage information. No owner reference
        // survives the collector; reinitialize the overlaid drawing workspace.
        Scratch::initialize_draw_at(scratch);
        (&mut *(&raw mut TILE_PREPARED)).save(camera,TILE_GENERATION,&*Scratch::tile_rows(scratch),TILE_MAX_RANK);
        HK_TILE_PREPARES=HK_TILE_PREPARES.wrapping_add(1);
    }
}
/// Prepare CPU packet words only after the previous final DMA has drained.
/// Main supplies the next framebuffer and final visual state, once per new
/// simulation clock while its presentation is queued. A replacement discards
/// the old prefix before touching tile Events. No GP0 commands are issued.
#[inline(never)]
pub fn prepare_back_prefix(camera:(i32,i32),framebuffer_y:u16,coverage:CoverageView<'_>) {
    if !crate::presentation::queued() || !psx_rt::interrupts::gp1_queue_pending(){return;}
    unsafe {
        let prefix=&mut *(&raw mut BACK_PREFIX);
        if prefix.next==DRAW_COUNT && prefix.matches(camera,framebuffer_y)
            && draw_visibility::matches_snapshot(prefix.state()){return;}
        prefix.invalidate();
        prepare_tile_coverage(camera,coverage);
        if !BACK_PREBUILD_ALLOWED || !psx_rt::interrupts::gp1_queue_pending(){return;}
        let Some(count)=draw_visibility::sparse_snapshot(prefix.state_mut())else{return;};
        // Pair the SDK's completed async DMA lease before reusing packet RAM.
        // Animation uploads/completion still belong to the post-flip begin.
        gpu::submit_linked_list_wait();
        collect_occluders(camera);restore_tile_coverage(camera);
        if !psx_rt::interrupts::gp1_queue_pending(){return;}
        USED=0;EXTRA_USED=0;KICKED=0;CHUNK_COUNT=0;FRAMEBUFFER_Y=framebuffer_y;
        let started=psx_io::timers::counter(psx_io::timers::Timer::Timer1);
        let (emitted,next)=scenery_range(camera,false,0,true);
        let scratch=occlusion_scratch::base();
        (&mut *(&raw mut PREFIX_OCCLUDERS)).save(
            (&*Scratch::occluders(scratch)).as_slice(),&*Scratch::union(scratch));
        prefix.save(camera,framebuffer_y,count,next,USED,EXTRA_USED,emitted,TILE_MAX_RANK,
            &*Scratch::tile_rows(occlusion_scratch::base()));
        HK_BACK_PREBUILDS=HK_BACK_PREBUILDS.wrapping_add(1);
        HK_BACK_PREBUILD_LINES=HK_BACK_PREBUILD_LINES.wrapping_add(psx_io::timers::counter(psx_io::timers::Timer::Timer1).wrapping_sub(started)as u32);
    }
}
#[inline]
fn tile_eligibility_changed(draw:usize) {
    unsafe {if TILE_CERTIFIED[draw/32]&(1<<(draw%32))!=0 {
        TILE_ELIGIBILITY_DIRTY=true;TILE_GENERATION=TILE_GENERATION.wrapping_add(1);
        (&mut *(&raw mut TILE_PREPARED)).invalidate();
    }}
}
/// Stage tile scissors after the existing exact alpha/core/occlusion pieces.
/// The output scratch is disjoint. Failed bounds or budget checks retain input.
#[inline(never)]
fn tile_visibility(input:&Pieces,out:&mut Pieces,rank:u16,core_packets:usize)->i32 {
    unsafe {
        if rank>=TILE_MAX_RANK || input.len()==0 {return 0;}
        let before=input.as_slice().iter().map(rect_area).sum::<i32>();
        if before<occlusion::MIN_SAVED_PIXELS {return 0;}
        let rows=&*Scratch::tile_rows(occlusion_scratch::base());
        if !occlusion::tile_runs::any_hidden(input.as_slice(),rows) {return 0;}
        if !occlusion::tile_runs::visible_runs_rows(input.as_slice(),rows,out) {
            HK_TILE_FALLBACKS=HK_TILE_FALLBACKS.wrapping_add(1);return 0;
        }
        let removed=before-out.as_slice().iter().map(rect_area).sum::<i32>();
        if removed<occlusion::MIN_SAVED_PIXELS {return 0;}
        let extra=(out.len()+core_packets).saturating_sub(1);
        if extra>EXTRA_ALLOWANCE-EXTRA_USED {
            HK_TILE_FALLBACKS=HK_TILE_FALLBACKS.wrapping_add(1);return 0;
        }
        HK_TILE_OCCLUSION_QUADS=HK_TILE_OCCLUSION_QUADS.wrapping_add(1);
        HK_TILE_SAVED_PIXELS=HK_TILE_SAVED_PIXELS.wrapping_add(removed as u32);
        removed
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
/// Supply all animation textures this frame will reference (Knight + nail).
/// Prepare before framebuffer clear; texture uploads never follow draw packets.
pub fn begin(room: &Room, animation_ids: &[u16],framebuffer_y:u16) -> hk_cache::Stats {
    let stats = animation_cache::prepare(room, animation_ids);
    unsafe {
        // The948-byte CPU-only scratchpad lease starts after runtime/cache
        // setup. Prior-frame references have ended; DMA only owns PACKETS.
        Scratch::initialize_at(occlusion_scratch::base());
        USED = 0;KICKED = 0;CHUNK_COUNT = 0;
        EXTRA_USED=0;FRAMEBUFFER_Y=framebuffer_y;
        for &id in animation_ids {
            // Future callers must not rebind any template referenced by a
            // retained BACK prefix or its foreground occluders, even if a
            // nonstreamed ID is supplied.
            if PREFIX_TEXTURES[id as usize/32]&(1<<(id as usize%32))!=0 {
                (&mut *(&raw mut BACK_PREFIX)).invalidate();
            }
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
#[no_mangle]pub static mut HK_DYNAMIC_HIDDEN_QUADS:u32=0;
pub fn texture(id: usize, verts: [(i16, i16); 4], tint: (u8, u8, u8)) {
    texture_material_alpha(id,verts,tint,0);
}
/// Small, independently resident Geo art uses the same bounded DMA packet pool.
pub fn resident_quad(template:&QuadTextured,verts:[(i16,i16);4]) {
    unsafe {
        if occlusion_scratch::dynamic_hidden(verts,(&*Scratch::occluders(occlusion_scratch::base())).as_slice()) {HK_DYNAMIC_HIDDEN_QUADS=HK_DYNAMIC_HIDDEN_QUADS.wrapping_add(1);return;}
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
        assert!(id < TEXTURE_COUNT);
        if occlusion_scratch::dynamic_hidden(verts,(&*Scratch::occluders(occlusion_scratch::base())).as_slice()) {HK_DYNAMIC_HIDDEN_QUADS=HK_DYNAMIC_HIDDEN_QUADS.wrapping_add(1);return;}
        assert!(USED < CAP);
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
/// Subtract later opaque black rectangles from rotated quads too. The
/// emulator clips spans per scanline and samples texels from per-pixel plane
/// equations, so the draw area cannot change what the surviving pixels
/// sample; verified by byte comparison of final frames (PERFORMANCE.md).
const ROTATED_OCCLUSION:bool=true;
#[no_mangle] pub static mut HK_ROTATED_OCCLUSION_QUADS:u32=0;
#[no_mangle] pub static mut HK_HIDDEN_QUADS:u32=0;
#[no_mangle] pub static mut HK_HIDDEN_PIXELS:u32=0;
/// True when every on-screen pixel of the quad's bounding box is covered by
/// the union of occluder rectangles drawn after draw `i`.
fn hidden_by_occluders(bbox:&[i32;4],i:usize,front:bool)->bool {
    unsafe {
        let scratch=occlusion_scratch::base();
        let occluders=(&*Scratch::occluders(scratch)).as_slice();
        if occluders.is_empty() {return false;}
        let [l,t,r,b]=*bbox;
        if !(l<r && t<b) {return false;}
        let u=*Scratch::union(scratch);
        if l<i32::from(u[0]) || t<i32::from(u[1]) || r>i32::from(u[2]) || b>i32::from(u[3]) {return false;}
        // Cheap passes first: keep only later occluders that overlap the box,
        // and answer at once when one of them contains it.
        let mut hits=0u8;let mut n=0;
        for (k,o) in occluders.iter().enumerate() {
            if !((o.front && !front) || (o.front==front && o.draw as usize>i)) {continue;}
            let q=&o.rect;
            if i32::from(q[0])>=r || i32::from(q[2])<=l || i32::from(q[1])>=b || i32::from(q[3])<=t {continue;}
            if i32::from(q[0])<=l && i32::from(q[1])<=t && i32::from(q[2])>=r && i32::from(q[3])>=b {
                HK_HIDDEN_QUADS=HK_HIDDEN_QUADS.wrapping_add(1);
                HK_HIDDEN_PIXELS=HK_HIDDEN_PIXELS.wrapping_add((r-l) as u32*(b-t) as u32);
                return true;
            }
            hits|=1u8<<k;n+=1;
        }
        if n<2 {return false;}
        let packed_bbox=bbox.map(|v|v as i16);
        let pieces=&mut *Scratch::hidden(scratch);pieces.set_slice(core::slice::from_ref(&packed_bbox));
        for (k,o) in occluders.iter().enumerate() {
            if hits&(1u8<<k)==0 {continue;}
            // Capacity overflow keeps the current
            // partition, which stays conservative.
            if subtract(pieces,&o.rect) {
                if pieces.len()==0 {
                    HK_HIDDEN_QUADS=HK_HIDDEN_QUADS.wrapping_add(1);
                    HK_HIDDEN_PIXELS=HK_HIDDEN_PIXELS.wrapping_add((r-l) as u32*(b-t) as u32);
                    return true;
                }
            }
        }
        false
    }
}
#[inline(never)]
fn restore_tile_coverage(camera:(i32,i32)) {
    unsafe {
            if let Some(max_rank)=(&mut *(&raw mut TILE_PREPARED)).take(camera,TILE_GENERATION,&mut *Scratch::tile_rows(occlusion_scratch::base())) {
                TILE_MAX_RANK=max_rank;HK_TILE_PREPARE_HITS=HK_TILE_PREPARE_HITS.wrapping_add(1);
            }else{
                // Coverage is optional. CPU-busy frames use the established
                // alpha/rectangle path instead of delaying their first DMA.
                TILE_MAX_RANK=0;HK_TILE_PREPARE_MISSES=HK_TILE_PREPARE_MISSES.wrapping_add(1);
            }
    }
}
#[inline(never)]
pub fn scenery(camera: (i32, i32), front: bool) -> u32 {
    let mut start=0;let mut emitted=0;
    if !front {
        unsafe {
            let prefix=&mut *(&raw mut BACK_PREFIX);
            if prefix.matches(camera,FRAMEBUFFER_Y)&&draw_visibility::matches_snapshot(prefix.state()) {
                let scratch=occlusion_scratch::base();
                let saved=&*(&raw const PREFIX_OCCLUDERS);
                let occluders=&mut *Scratch::occluders(scratch);occluders.clear();
                for (i,&value) in saved.entries().iter().enumerate(){occluders.set(i,value);}
                *Scratch::union(scratch)=saved.union();
                USED=prefix.packets;EXTRA_USED=prefix.extra;start=prefix.next;emitted=prefix.emitted;
                TILE_MAX_RANK=prefix.max_rank;
                (*Scratch::tile_rows(occlusion_scratch::base())).copy_from_slice(&prefix.rows);
                HK_BACK_PREBUILD_HITS=HK_BACK_PREBUILD_HITS.wrapping_add(1);
                HK_BACK_PREBUILD_PACKETS=HK_BACK_PREBUILD_PACKETS.wrapping_add(USED as u32);
            }else{
                HK_BACK_PREBUILD_MISSES=HK_BACK_PREBUILD_MISSES.wrapping_add(1);
                collect_occluders(camera);restore_tile_coverage(camera);
            }
            prefix.invalidate();
        }
    }
    emitted+scenery_range(camera,front,start,false).0
}
#[inline(never)]
fn scenery_range(camera:(i32,i32),front:bool,start:usize,prebuilding:bool)->(u32,usize) {
    let mut n=0;let mut next=start;
    unsafe {
        let scratch=occlusion_scratch::base();
        let occluders=(&*Scratch::occluders(scratch)).as_slice();
        let occluder_union=&*Scratch::union(scratch);
        for (i, d) in DRAWS[..DRAW_COUNT].iter().enumerate().skip(start) {
            // Never stop inside a source's repair/scissor group or after its
            // tile event without emitting it. One whole source is atomic.
            if prebuilding && !psx_rt::interrupts::gp1_queue_pending(){break;}
            next=i+1;
            if i&31==0 {crate::input::checkpoint();}
            // Start rasterising after the first emitted draw; subsequent
            // chunks retain the established eight-source-draw cadence.
            if !prebuilding && (i&7==0 || (CHUNK_COUNT==0 && USED!=0)) {kick_ready();}
            if (d.flags&FRONT!=0) != front {continue;}
            // Retire ownership only in the pass that actually draws this
            // source. Front ownership must survive the entire back pass so a
            // foreground mask hides back geometry regardless of authored
            // source index; the front pass then retires those owners in its
            // own source order. Invisible sources are still retired when they
            // belong to the active pass, preserving the event protocol.
            if TILE_MAX_RANK!=0 && (d.flags&FRONT!=0)==front {
                (&*(&raw const TILE_EVENTS)).reach(i,front,&mut *Scratch::tile_rows(scratch));
            }
            if d.flags&(EMPTY_TEXTURE|DRAW_ENABLED)!=DRAW_ENABLED {continue;}
            // Distant parallax layers can overflow the pre-shift i32 product.
            // The cooker proves the shifted values/subtractions fit i32.
            let cx = (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let cy = (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let Some(bbox)=scenery_bounds::project_clipped(&d.xy,DRAW_BOUNDS[i],cx,cy) else {continue;};
            if hidden_by_occluders(&bbox,i,front) {continue;}
            let mut v = [(0i32, 0i32); 4];
            for k in 0..4 {
                v[k] = (
                    160 + ((d.xy[k * 2] - cx) >> 8),
                    120 - ((d.xy[k * 2 + 1] - cy) >> 8),
                );
            }
            let t=&TEMPLATES[d.texture];let uv=uv_rect(t);
            let opacity=OPACITIES[i] as u16 | if d.flags&BLACK_AVERAGE!=0 {256}else{0};
            if d.flags&LEGAL_EXTENT!=0 || scenery_geometry::legal(&v) {
                let xy=v.map(|p|(p.0 as i16,p.1 as i16));
                if d.flags&SOLID_BLACK!=0 && opacity&255==128 && opacity&256!=0 {
                    assert!(USED<CAP);PACKETS[USED].write_black(xy.map(vertex));USED+=1;n+=1;continue;
                }
                // Cache RGB only: the current template still supplies its
                // command byte, including animation-template rebinding.
                let words=scenery_color::with_material([
                    t.color_cmd,vertex(xy[0]),t.uv0_clut,vertex(xy[1]),t.uv1_tpage,
                    vertex(xy[2]),t.uv2,vertex(xy[3]),t.uv3],d.color,opacity,
                    Clut::new(FADE_CLUT_X,FADE_CLUT_Y).uv_clut_word() as u32);
                // Keep the original alpha partition alive for the capacity
                // fallback below while scenery scratch undergoes subtraction.
                let mut alpha_storage=core::mem::MaybeUninit::uninit();
                let scissors=if d.flags&SCISSOR_CANDIDATE!=0 {
                    let out=alpha_scissor_cache::MappedScissors::initialize(&mut alpha_storage);
                    if SCISSOR_CACHE[i].map_cached_into(xy,d.texture as u16,uv.w,uv.h,&COVERS[d.texture],&mut *(&raw mut SCISSOR_SECONDARY),out) {Some(&*out)}else{None}
                }else{None};
                // Flat core and bounded multi-rectangle occlusion: exact partitions of
                // the original pixels, bounded by MAX_PIECES and the packet allowance.
                // Draw-area clipping retains original vertices and UV planes for
                // rotated quads too; a flat black core is exact only on the
                // black-average path, which zeroes the sentinel texel's red bit.
                let axis=xy[0].1==xy[1].1 && xy[0].0==xy[2].0 && xy[1].0==xy[3].0 && xy[2].1==xy[3].1;
                // On-screen bounding box of the quad; for axis-aligned quads
                // this is the quad itself.
                let bbox=bbox.map(|v|v as i16);
                let [bl,bt,br,bb]=bbox;
                let box_area=if bl<br && bt<bb {(br-bl) as i32*(bb-bt) as i32}else{0};
                let pieces=&mut *Scratch::scenery(scratch);pieces.clear();
                if let Some(s)=&scissors {pieces.set_slice(s.as_slice());}
                else if box_area>0 {pieces.push([bl,bt,br,bb]);}
                let mut core_rect=None;let mut saved=0i32;
                // Ordinary RGB zero modulates every STP-clear texel to opaque
                // black; unlike the material-specific sentinel correction, its
                // larger general opaque core is also an exact flat replacement.
                let zero_tint=opacity==128 && d.color==0;
                let core=if axis && box_area>=OCCLUDER_MIN_AREA {if zero_tint {opaque_core_of(d.texture)}else{core_of(d.texture)}}else{[0;4]};
                if opacity&255==128 && (opacity&256!=0 || zero_tint) && core[2]!=0 {
                    if let Some(core)=(&mut *(&raw mut CORE_MAP_CACHE)).map(xy,uv.w,uv.h,core) {
                        if rect_area(&core)>=256 && subtract(pieces,&core) {core_rect=Some(core);}
                    }
                }
                let holes=&mut *Scratch::holes(scratch);holes.clear();
                // Reject impossible savings before copying and ranking holes.
                // Alpha scissors and core subtraction only remove bbox pixels.
                let possible_holes=(axis || ROTATED_OCCLUSION) && box_area>=OCCLUDER_MIN_AREA
                    && !occluders.is_empty() && occlusion::can_save_minimum(&bbox,occluder_union);
                for o in &occluders[..if possible_holes {occluders.len()}else{0}] {
                    if !((o.front && !front) || (o.front==front && o.draw as usize>i)) {continue;}
                    if !occlusion::can_save_minimum(&bbox,&o.rect) {continue;}
                    holes.push(o.rect);
                }
                if holes.len()!=0 {
                    saved=occlusion::subtract_many(pieces,holes.as_slice());
                }
                let tile_saved=tile_visibility(pieces,holes,tile_coverage::rank(i,front),usize::from(core_rect.is_some()));
                let pieces=if tile_saved>0 {saved+=tile_saved;holes}else{pieces};
                if core_rect.is_some() || saved>0 {
                    let extra=(pieces.len()+usize::from(core_rect.is_some())).saturating_sub(1);
                    if extra<=EXTRA_ALLOWANCE-EXTRA_USED {
                        EXTRA_USED+=extra;
                        for rect in pieces.as_slice() {
                            assert!(USED<CAP);PACKETS[USED].write_scissored(words,*rect,FRAMEBUFFER_Y);USED+=1;
                        }
                        if let Some(c)=core_rect {
                            let q=[(c[0],c[1]),(c[2],c[1]),(c[0],c[3]),(c[2],c[3])];
                            assert!(USED<CAP);PACKETS[USED].write_black(q.map(vertex));USED+=1;
                            HK_CORE_QUADS=HK_CORE_QUADS.wrapping_add(1);
                        }
                        if saved>0 {HK_OCCLUSION_QUADS=HK_OCCLUSION_QUADS.wrapping_add(1);HK_OCCLUSION_SAVED_PIXELS=HK_OCCLUSION_SAVED_PIXELS.wrapping_add(saved as u32);if !axis {HK_ROTATED_OCCLUSION_QUADS=HK_ROTATED_OCCLUSION_QUADS.wrapping_add(1);}}
                        n+=pieces.len() as u32+u32::from(core_rect.is_some());continue;
                    }
                    HK_SCISSOR_CAPACITY_FALLBACKS=HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
                }
                if let Some(scissors)=scissors {
                    let extra=scissors.len().saturating_sub(1);
                    if extra<=EXTRA_ALLOWANCE-EXTRA_USED {
                        EXTRA_USED+=extra;
                        for rect in scissors.as_slice() {
                            assert!(USED<CAP);PACKETS[USED].write_scissored(words,*rect,FRAMEBUFFER_Y);USED+=1;
                        }
                        HK_SCISSOR_QUADS=HK_SCISSOR_QUADS.wrapping_add(scissors.len() as u32);
                        HK_SCISSOR_SAVED_PIXELS=HK_SCISSOR_SAVED_PIXELS.wrapping_add(scissors.saved_pixels());
                        n+=scissors.len() as u32;continue;
                    }
                    HK_SCISSOR_CAPACITY_FALLBACKS=HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
                }
                assert!(USED<CAP);PACKETS[USED].write_plain(words);USED+=1;n+=1;
            } else {
                // Oversized repair retains its existing packet construction.
                let gain=GAINS[i] as u16;
                let tint=((d.tint.0 as u16*gain/128) as u8,(d.tint.1 as u16*gain/128) as u8,(d.tint.2 as u16*gain/128) as u8);
                n+=repair(t,v,uv,tint,opacity,i,front);
            }
        }
    }
    if !prebuilding {kick_ready();}
    (n,next)
}
// Repair output is copied into DMA-owned packets before returning. No DMA or
// interrupt references this scratch; keeping it here avoids clearing1.5KiB
// and inflating the common scenery loop's stack for every oversized layer.
static mut REPAIR_CHILDREN:[Quad;64]=[Quad::ZERO;64];
#[no_mangle]pub static mut HK_REPAIR_OCCLUSION_QUADS:u32=0;
#[no_mangle]pub static mut HK_REPAIR_OCCLUSION_SAVED_PIXELS:u32=0;
#[inline(never)]
fn repair(t:&QuadTextured,v:[(i32,i32);4],uv:UvRect,tint:(u8,u8,u8),opacity:u16,draw:usize,front:bool)->u32 {
    crate::input::checkpoint();
    unsafe {
        let children=&mut *(&raw mut REPAIR_CHILDREN);
        let count=match scenery_geometry::subdivide(v,uv,children) {
            Ok(count)=>count,
            Err(_)=>{HK_SCENERY_REPAIR_FAILURES=HK_SCENERY_REPAIR_FAILURES.wrapping_add(1);panic!("scenery subdivision");}
        };
        let scratch=occlusion_scratch::base();
        let occluders=(&*Scratch::occluders(scratch)).as_slice();
        let union=&*Scratch::union(scratch);
        let pieces=&mut *Scratch::scenery(scratch);
        let holes=&mut *Scratch::holes(scratch);
        let mut emitted=0;
        for child in &children[..count] {
            let words=quad_words(t,child.xy,tint,Some(child.uv),opacity);
            // Clip only the original emitted child packet. Restarting geometry
            // or UV interpolation here would change its samples and seams.
            let l=child.xy.iter().map(|p|p.0).min().unwrap().max(0);let r=child.xy.iter().map(|p|p.0).max().unwrap().min(320);
            let top=child.xy.iter().map(|p|p.1).min().unwrap().max(0);let bottom=child.xy.iter().map(|p|p.1).max().unwrap().min(240);
            if l>=r || top>=bottom {continue;}
            let bbox=[l,top,r,bottom];holes.clear();
            if !occluders.is_empty() && occlusion::can_save_minimum(&bbox,union) {
                for o in occluders {
                    if !((o.front && !front) || (o.front==front && o.draw as usize>draw)) {continue;}
                    if occlusion::can_save_minimum(&bbox,&o.rect) {holes.push(o.rect);}
                }
            }
            pieces.clear();pieces.push(bbox);
            let mut saved=if holes.len()!=0 {occlusion::subtract_many(pieces,holes.as_slice())}else{0};
            let tile_saved=tile_visibility(pieces,holes,tile_coverage::rank(draw,front),0);
            let chosen=if tile_saved>0 {saved+=tile_saved;&*holes}else{&*pieces};
            if saved>0 {
                // Mandatory repair children were reserved by the cooker.
                // Only final extra scissors consume the shared optional reserve.
                let extra=chosen.len().saturating_sub(1);
                if extra<=EXTRA_ALLOWANCE-EXTRA_USED {
                    EXTRA_USED+=extra;
                    for rect in chosen.as_slice() {
                        assert!(USED<CAP);PACKETS[USED].write_scissored(words,*rect,FRAMEBUFFER_Y);USED+=1;
                    }
                    emitted+=chosen.len()as u32;
                    HK_REPAIR_OCCLUSION_QUADS=HK_REPAIR_OCCLUSION_QUADS.wrapping_add(1);
                    HK_REPAIR_OCCLUSION_SAVED_PIXELS=HK_REPAIR_OCCLUSION_SAVED_PIXELS.wrapping_add(saved as u32);
                    continue;
                }
                HK_SCISSOR_CAPACITY_FALLBACKS=HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
            }
            assert!(USED<CAP);PACKETS[USED].write_plain(words);USED+=1;emitted+=1;
        }
        HK_SCENERY_REPAIRS=HK_SCENERY_REPAIRS.wrapping_add(1);
        crate::input::checkpoint();
        emitted
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
pub fn submit(health:u16,max_health:u16,soul:u16,max_soul:u16,paused:bool,blue_health:u16,transition:u8) {
    crate::input::checkpoint();
    animation_cache::submit();
    // One owner, no mutation until main observes DMA and GPU completion.
    unsafe {
        let ot = &mut *(&raw mut OT);
        ot.clear();
        // OT insertion prepends: this packet executes after scenery and HUD.
        if transition!=0 {
            TRANSITION=gpu::prim::QuadGouraudBlended::new([(0,0),(320,0),(0,240),(320,240)],
                [(transition,transition,transition);4],gpu::material::BlendMode::Subtract);
            ot.add(0,&mut *(&raw mut TRANSITION),gpu::prim::QuadGouraudBlended::WORDS);
        }
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
