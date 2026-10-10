//! Resident scenery plus bounded animation residency and SDK DMA packets.
use crate::animation_cache;
use crate::{
    alpha_scissor_cache,
    draw_packet::Packet,
    scenery_geometry::{self, Quad, UvRect},
};
use hk_format::coverage::CoverageView;
use hk_format::{u32_at, Room, Texture};
use psx_gpu::{self as gpu, ot::OrderingTable, prim::QuadTextured};
use psx_vram::{Clut, TexDepth, Tpage};
include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/scenery_budgets.rs"
));
#[path = "back_prebuild.rs"]
mod back_prebuild;
#[path = "core_map_cache.rs"]
mod core_map_cache;
#[path = "draw_visibility.rs"]
mod draw_visibility;
#[path = "occluder_snapshot.rs"]
mod occluder_snapshot;
#[path = "opaque_groups.rs"]
mod opaque_groups;
#[path = "scenery_color.rs"]
mod scenery_color;
#[path = "tile_coverage.rs"]
mod tile_coverage;
#[path = "tile_events.rs"]
mod tile_events;
#[path = "tile_frame_cache.rs"]
mod tile_frame_cache;
use draw_visibility::DRAW_ENABLED;
pub use draw_visibility::{reset_visibility, set_gain, set_opacity, set_visible};
static mut CORE_MAP_CACHE: core_map_cache::Cache = core_map_cache::Cache::new();
// Maximum scenery records plus the Knight and the active nail effect.
const CAP: usize = 1032;
// Final-list ownership matches HUD/dialogue: not mutated until DMA completes.
static mut TRANSITION: gpu::prim::QuadGouraudBlended = gpu::prim::QuadGouraudBlended::new(
    [(0, 0), (320, 0), (0, 240), (320, 240)],
    [(0, 0, 0); 4],
    gpu::material::BlendMode::Subtract,
);
const MAX_DRAWS: usize = crate::disc::SCENE_DRAW_CAPACITY;
const _: () = assert!(MAX_DRAWS <= 1024);
static mut TILE_DRAWS: [(u16, u16); MAX_DRAWS] = [(0, 0); MAX_DRAWS];
static mut TILE_DRAW_COUNT: usize = 0;
const MAX_TILE_GROUPS: usize = 32;
static mut TILE_GROUPS: [opaque_groups::Group; MAX_TILE_GROUPS] = [opaque_groups::Group {
    certificate: 0,
    members: [u16::MAX; 4],
}; MAX_TILE_GROUPS];
static mut TILE_GROUP_COUNT: usize = 0;
static mut TILE_MAX_RANK: u16 = 0;
static mut TILE_EVENTS: tile_events::Events<MAX_DRAWS> = tile_events::Events::new();
static mut TILE_CERTIFIED: [u32; MAX_DRAWS / 32] = [0; MAX_DRAWS / 32];
static mut TILE_GENERATION: u32 = 0;
static mut TILE_ELIGIBILITY_DIRTY: bool = false;
static mut TILE_PREPARED: tile_frame_cache::Prepared = tile_frame_cache::Prepared::new();
static mut BACK_PREFIX: back_prebuild::Prefix = back_prebuild::Prefix::new();
// Valid only with BACK_PREFIX. Save initialized entries, never scratch padding
// or uninitialized bool fields; begin clears scratch before a hit restores it.
static mut PREFIX_OCCLUDERS: occluder_snapshot::Snapshot<Occluder, MAX_OCCLUDERS> =
    occluder_snapshot::Snapshot::new();
#[cfg(target_arch = "mips")]
const _: () =
    assert!(core::mem::size_of::<occluder_snapshot::Snapshot<Occluder, MAX_OCCLUDERS>>() == 108);
static mut PREFIX_TEXTURES: [u32; MAX_TEXTURES / 32] = [0; MAX_TEXTURES / 32];
static mut BACK_PREBUILD_ALLOWED: bool = false;
#[no_mangle]
pub static mut HK_BACK_PREBUILDS: u32 = 0;
#[no_mangle]
pub static mut HK_BACK_PREBUILD_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_BACK_PREBUILD_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_BACK_PREBUILD_PACKETS: u32 = 0;
#[no_mangle]
pub static mut HK_BACK_PREBUILD_LINES: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_PREPARES: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_PREPARE_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_PREPARE_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_COVERAGE_CANDIDATES: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_COVERAGE_LOOKUPS: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_COVERAGE_TILES: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_OCCLUSION_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_SAVED_PIXELS: u32 = 0;
#[no_mangle]
pub static mut HK_TILE_FALLBACKS: u32 = 0;
const FRONT: u8 = 1;
const LEGAL_EXTENT: u8 = 2;
const BLACK_AVERAGE: u8 = 4;
const SCISSOR_CANDIDATE: u8 = 8;
const EMPTY_TEXTURE: u8 = 16;
const SOLID_BLACK: u8 = 32;
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
/// One scenery draw exactly as the view cooks it (hk_format `Room::draw`, 44
/// bytes, little-endian): read in place from the resident scene bank, which
/// stays unchanged until `release_scene`. Only the mutable state lives here:
/// `DRAW_FLAGS` and `DRAW_COLORS`. Copying every record into a 48-byte array
/// cost 33 KiB of RAM and most of a view bind's time.
#[repr(C)]
struct Draw {
    texture: u16,
    front: u16,
    scale: i32,
    xy: [i32; 8],
    tint: [u8; 3],
    black: u8,
}
const _: () = assert!(core::mem::size_of::<Draw>() == 44);
static mut DRAW_RECORDS: [*const Draw; MAX_DRAWS] = [core::ptr::null(); MAX_DRAWS];
static mut DRAW_FLAGS: [u8; MAX_DRAWS] = [0; MAX_DRAWS];
static mut DRAW_COLORS: [u32; MAX_DRAWS] = [0; MAX_DRAWS];
/// The bound view's record of draw `i` (`i < DRAW_COUNT`).
#[inline(always)]
unsafe fn draw_record(i: usize) -> &'static Draw {
    unsafe { &*DRAW_RECORDS[i] }
}
impl Draw {
    #[inline(always)]
    fn texture(&self) -> usize {
        self.texture as usize
    }
    #[inline(always)]
    fn tint(&self) -> (u8, u8, u8) {
        (self.tint[0], self.tint[1], self.tint[2])
    }
}
static mut DRAW_BOUNDS: [u8; MAX_DRAWS] = [0; MAX_DRAWS];
const MAX_TEXTURES: usize = crate::disc::SCENE_TEXTURE_CAPACITY;
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
static mut SOLID_TEXTURES: [u32; MAX_TEXTURES / 32] = [0; MAX_TEXTURES / 32];
/// Cooked largest all-opaque-black texel rectangle [x,y,w,h] per scene
/// texture (w==0: none). At full opacity that core is an exact flat opaque
/// black quad and hides everything drawn earlier beneath it.
static mut CORES: [[u8; 4]; MAX_TEXTURES] = [[0; 4]; MAX_TEXTURES];
// A colored STP-clear texel also hides earlier draws, but must retain its
// texture/color when drawn. This table is never used for flat black rendering.
static mut OPAQUE_CORES: [[u8; 4]; MAX_TEXTURES] = [[0; 4]; MAX_TEXTURES];
fn opaque_core_of(texture: usize) -> [u8; 4] {
    unsafe { OPAQUE_CORES.get(texture).copied().unwrap_or([0; 4]) }
}
fn core_of(texture: usize) -> [u8; 4] {
    unsafe { CORES.get(texture).copied().unwrap_or([0; 4]) }
}
const OCCLUDER_MIN_AREA: i32 = 2048;
// Immutable eligibility is established once per region. Dynamic visibility,
// gain and opacity are still checked before each frame's occluder selection.
static mut OCCLUDER_DRAWS: [u16; MAX_DRAWS] = [0; MAX_DRAWS];
static mut OCCLUDER_DRAW_COUNT: usize = 0;
// Fully opaque rotated back masks use their actual convex-quad interior.
static mut SOLID_CORE_DRAWS: [u16; MAX_DRAWS] = [0; MAX_DRAWS];
static mut SOLID_CORE_DRAW_COUNT: usize = 0;
static mut SOLID_CORE_CACHE: solid_occluder::Cache = solid_occluder::Cache::new();

#[no_mangle]
pub static mut HK_SOLID_CORE_CACHE_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_SOLID_CORE_CACHE_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_SOLID_CORE_OCCLUDERS: u32 = 0;

/// Frame pacing in scanlines (Timer1 counts HBlanks): time from the frame's
/// start to its first DMA kick, its final kick and its display-flip request.
#[no_mangle]
pub static mut HK_FRAME_FIRST_KICK_LINES: u32 = 0;
#[no_mangle]
pub static mut HK_FRAME_LAST_KICK_LINES: u32 = 0;
#[no_mangle]
pub static mut HK_FRAME_FLIP_LINES: u32 = 0;
static mut FRAME_START_LINE: u16 = 0;
pub fn frame_lines() -> u32 {
    psx_io::timers::counter(psx_io::timers::Timer::Timer1).wrapping_sub(unsafe { FRAME_START_LINE })
        as u32
}
pub fn mark_frame_start() {
    unsafe {
        FRAME_START_LINE = psx_io::timers::counter(psx_io::timers::Timer::Timer1);
    }
}
#[no_mangle]
pub static mut HK_CORE_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_OCCLUSION_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_OCCLUSION_SAVED_PIXELS: u32 = 0;
#[path = "occluder_admission.rs"]
mod occluder_admission;
#[path = "scenery_occlusion.rs"]
mod occlusion;
#[path = "solid_occluder.rs"]
mod solid_occluder;
use occlusion::{rect_area, subtract, Pieces};
#[path = "occlusion_scratch.rs"]
mod occlusion_scratch;
use occlusion_scratch::{Occluder, Occluders, Storage as Scratch, MAX_OCCLUDERS};
/// The renderer's RAM scratch: the occlusion workspace and the repair grid,
/// about 2.2 KiB. `main` keeps one in its frame, inside the stack reservation,
/// for the whole program and binds it with `bind_scratch` before rendering.
pub struct RenderScratch {
    occlusion: Scratch,
    grid: scenery_geometry::Grid,
}
static mut GRID: *mut scenery_geometry::Grid = core::ptr::null_mut();
/// # Safety
/// Call once, before any frame, with storage that outlives every render call.
pub unsafe fn bind_scratch(scratch: *mut RenderScratch) {
    unsafe {
        occlusion_scratch::bind(&raw mut (*scratch).occlusion);
        GRID = &raw mut (*scratch).grid;
    }
}
#[path = "scenery_bounds.rs"]
mod scenery_bounds;
static mut DRAW_COUNT: usize = 0;
static mut ACTIVE_BANK: usize = 0;
static mut TEXTURE_COUNT: usize = 0;
static mut STREAMED: [u32; MAX_TEXTURES / 32] = [0; MAX_TEXTURES / 32];
// Preserve cooked draw order while toggling authored whole-sprite variants.
// This is transient room state, reset when starting or respawning.
static mut VISIBLE: [u32; MAX_DRAWS / 32] = [0; MAX_DRAWS / 32];
static mut GAINS: [u8; MAX_DRAWS] = [128; MAX_DRAWS];
static mut OPACITIES: [u8; MAX_DRAWS] = [0; MAX_DRAWS];
static mut BLACK_MASKS: [u32; MAX_TEXTURES / 32] = [0; MAX_TEXTURES / 32];
static mut FADE_CLUT_READY: bool = false;
// Shared 16-word reservation, disjoint from scenery banks, animation and HUD.
const FADE_CLUT_X: u16 = 320;
const FADE_CLUT_Y: u16 = 480;
const FADE_CLUT: [u8; 32] = [
    0, 0, 255, 255, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128, 0, 128,
    0, 128, 0, 128, 0, 128, 0, 128,
];
// Hit flash silhouette: every drawn palette index of a 4bpp texture becomes
// STP white, index 0 stays transparent, so an Add-blended second draw of an
// actor frame through this CLUT adds `tint` over exactly its opaque texels.
// (320,492) is one of the four CLUT rows the Shade reservation leaves free
// (docs/BUDGET.md).
const FLASH_CLUT_X: u16 = 320;
const FLASH_CLUT_Y: u16 = 492;
const FLASH_CLUT: [u8; 32] = [
    0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
];
#[no_mangle]
pub static mut HK_FLASH_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_FLASH_SKIPPED: u32 = 0;
// A count of255 means the legacy pack has no precomputed alpha cover.
// init fills every active entry before drawing; zero initialization keeps this
// working storage out of the EXE's on-disc data payload.
static mut COVERS: [[u8; 20]; MAX_TEXTURES] = [[0; 20]; MAX_TEXTURES];
// One entry per admitted draw. Geometry spans may change by one pixel with camera
// rounding; the entry key captures both signed spans and the texture identity.
static mut SCISSOR_CACHE: [alpha_scissor_cache::Entry; MAX_DRAWS] =
    [alpha_scissor_cache::Entry::EMPTY; MAX_DRAWS];
static mut SCISSOR_SECONDARY: alpha_scissor_cache::Secondary =
    alpha_scissor_cache::Secondary::EMPTY;
static mut EXTRA_ALLOWANCE: usize = 0;
/// What the scenery's optional trimming may spend of `EXTRA_ALLOWANCE`. The
/// hero light's fan is optional too and comes after the back pass, so
/// trimming that spent the whole allowance used to drop the light on frames
/// the GPU had been busy on, and keep it otherwise: it came and went with the
/// frame rate. Trimming changes no pixel, so it leaves the fan its room.
static mut SCENERY_ALLOWANCE: usize = 0;
#[cfg(feature = "hero-light")]
const LIGHT_RESERVE: usize = 8;
#[cfg(not(feature = "hero-light"))]
const LIGHT_RESERVE: usize = 0;
static mut EXTRA_USED: usize = 0;
static mut FRAMEBUFFER_Y: u16 = 0;
// Cumulative diagnostics wrap after u32::MAX; they never control rendering.
#[no_mangle]
pub static mut HK_SCENERY_REPAIRS: u32 = 0;
#[no_mangle]
pub static mut HK_SCENERY_REPAIR_FAILURES: u32 = 0;
#[no_mangle]
pub static mut HK_SCISSOR_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_SCISSOR_SAVED_PIXELS: u32 = 0;
#[no_mangle]
pub static mut HK_SCISSOR_CAPACITY_FALLBACKS: u32 = 0;
fn vertex(p: (i16, i16)) -> u32 {
    (p.0 as u16 as u32) | ((p.1 as u16 as u32) << 16)
}
/// Called after final GPU/DMA retirement, before any scene/atlas replacement.
pub fn release_scene() {
    assert!(!dma_pending() && !psx_rt::interrupts::gp1_queue_pending());
    unsafe {
        BOUND_VIEW = usize::MAX;
        (&mut *(&raw mut BACK_PREFIX)).invalidate();
        (&mut *(&raw mut TILE_PREPARED)).invalidate();
        BACK_PREBUILD_ALLOWED = false;
        TEXTURE_COUNT = 0;
        DRAW_COUNT = 0;
        POOL_COUNT = 0;
        TILE_DRAW_COUNT = 0;
        TILE_GROUP_COUNT = 0;
        TILE_MAX_RANK = 0;
        TILE_GENERATION = TILE_GENERATION.wrapping_add(1);
    }
    animation_cache::init();
}
/// The view `init` last bound (its draws are the ones `scenery` draws), or
/// usize::MAX once a scene gate has released it.
static mut BOUND_VIEW: usize = usize::MAX;
pub fn bound_view() -> usize {
    unsafe { BOUND_VIEW }
}
#[inline(never)]
#[optimize(size)]
pub fn init(room: &Room, bank: usize, region: usize, coverage: CoverageView<'_>) {
    crate::input::checkpoint();
    unsafe {
        BOUND_VIEW = region;
        EARLY_BACK = EARLY_UNKNOWN;
        TRIMMING = !CPU_VIEWS.contains(&(bank, region));
        // The vignette words belong to the old view's draws: remake them all.
        #[cfg(feature = "hero-vignette")]
        {
            VIGNETTE_ON = false;
        }
        (&mut *(&raw mut BACK_PREFIX)).invalidate();
        BACK_PREBUILD_ALLOWED = false;
        (&mut *(&raw mut TILE_PREPARED)).invalidate();
        TILE_GENERATION = TILE_GENERATION.wrapping_add(1);
        TILE_ELIGIBILITY_DIRTY = false;
        TILE_CERTIFIED = [0; MAX_DRAWS / 32];
        let scene_changed = TEXTURE_COUNT == 0 || ACTIVE_BANK != bank;
        if scene_changed {
            animation_cache::init();
            POOL_INFO_VALID.fill(0);
        }
        ACTIVE_BANK = bank;
        assert!(room.counts[1] <= MAX_TEXTURES);
        DRAW_COUNT = room.counts[2];
        assert!(DRAW_COUNT <= MAX_DRAWS);
        TEXTURE_COUNT = room.counts[1];
        // The cooker bounds every mandatory child, even if all source draws
        // are visible. Optional scissors can only spend the remaining capacity.
        EXTRA_ALLOWANCE = CAP
            .checked_sub(DYNAMIC_PACKET_RESERVE + SCENERY_PACKET_BUDGETS[region] as usize)
            .expect("scenery packet budget");
        SCENERY_ALLOWANCE = EXTRA_ALLOWANCE.saturating_sub(LIGHT_RESERVE);
        if scene_changed {
            // Texture IDs, dimensions and covers are immutable within a scene
            // bank. Cache keys already include exact texture and signed spans,
            // so region changes can retain matching entries. Clear every slot
            // on a bank change, including slots beyond the new draw count.
            for entry in &mut SCISSOR_CACHE {
                entry.invalidate();
            }
            (&mut *(&raw mut SCISSOR_SECONDARY)).invalidate();
            STREAMED = [0; MAX_TEXTURES / 32];
            BLACK_MASKS = [0; MAX_TEXTURES / 32];
            SOLID_TEXTURES = [0; MAX_TEXTURES / 32];
            // Cores come from the resident bank's texture attributes; slots
            // beyond the scene's texture count stay zero (no core).
            CORES = [[0; 4]; MAX_TEXTURES];
            OPAQUE_CORES = [[0; 4]; MAX_TEXTURES];
            for i in 0..room.counts[1] {
                if i & 63 == 0 {
                    crate::input::checkpoint();
                }
                CORES[i] = crate::disc::scene_black_core(bank, i);
                OPAQUE_CORES[i] = crate::disc::scene_opaque_core(bank, i);
            }
        }
        if !FADE_CLUT_READY {
            // First room only; FIFO upload owns static bytes and precedes any
            // room packets. No per-frame transfer or DMA lifetime is added.
            gpu::draw_sync();
            psx_vram::upload_bytes(
                psx_vram::VramRect::new(FADE_CLUT_X, FADE_CLUT_Y, 16, 1),
                &FADE_CLUT,
            );
            psx_vram::upload_bytes(
                psx_vram::VramRect::new(FLASH_CLUT_X, FLASH_CLUT_Y, 16, 1),
                &FLASH_CLUT,
            );
            FADE_CLUT_READY = true;
            #[cfg(feature = "hero-light")]
            crate::hero_light::upload();
        }
        draw_visibility::begin_region();
        // Texture state first (it reads no draw), so a single pass over the
        // draws below can decode each one and file it everywhere it belongs.
        // Separate passes re-read every 48-byte draw from RAM each time; on a
        // view bind mid-fight that was most of render::init's cost.
        if scene_changed {
            for i in 0..room.counts[1] {
                if i & 31 == 0 {
                    crate::input::checkpoint();
                }
                let t = room.texture(i);
                let flags = crate::disc::scene_texture_flags(bank, i);
                if !t.is_streamed() && flags & 1 != 0 {
                    BLACK_MASKS[i / 32] |= 1 << (i % 32);
                }
                // Flags are derived from the exact final atlas words before its
                // startup upload. Compact resident scenes keep no duplicate texels.
                if !t.is_streamed() && flags & 2 != 0 {
                    SOLID_TEXTURES[i / 32] |= 1 << (i % 32);
                }
                if let Some(cover) = room.alpha_cover(t) {
                    COVERS[i].copy_from_slice(cover);
                } else {
                    COVERS[i] = [255; 20];
                }
                if t.is_streamed() {
                    STREAMED[i / 32] |= 1 << (i % 32);
                    TEMPLATES[i] = ZERO;
                } else {
                    let page = crate::disc::scene_page_base(bank) + t.page as usize;
                    let (x, y) = hk_cache::residency::page_xy(page);
                    let tp = Tpage::new(x, y, TexDepth::Bit4);
                    TEMPLATES[i] = template(t, tp, t.u, t.v);
                }
            }
            // The scene's shared draw pool, for warm_step; legacy rooms have none.
            POOL_COUNT = 0;
            WARM_NEXT = 0;
            if room.scene_resident() {
                let count = room.pool_draw_count().min(POOL_INFO_CAPACITY);
                if count > 0 {
                    POOL_RECORDS = room.pool_draw(0).as_ptr().cast::<Draw>();
                    assert!(POOL_RECORDS as usize & 3 == 0);
                    POOL_COUNT = count;
                }
            }
        }
        PREFIX_TEXTURES.fill(0);
        BACK_PREBUILD_ALLOWED = true;
        // Bind shared immutable draw identities once per region. Sorting by
        // (front, draw) lets the frame collector visit latest sources first:
        // back draws fill TILE_DRAWS from the start, front ones from the end
        // (reversed below), and the two never meet (at most DRAW_COUNT).
        TILE_DRAW_COUNT = 0;
        let mut tile_fronts = 0;
        let certs = coverage.pool_map();
        // Immutable cover/axis tests belong to activation, not every rendered
        // frame. A rejected candidate still draws its original complete quad.
        OCCLUDER_DRAW_COUNT = 0;
        SOLID_CORE_DRAW_COUNT = 0;
        for i in 0..DRAW_COUNT {
            if i & 31 == 0 {
                crate::input::checkpoint();
            }
            let record = room.draw(i).as_ptr().cast::<Draw>();
            // Scene banks and their draw sections are word aligned.
            assert!(record as usize & 3 == 0);
            DRAW_RECORDS[i] = record;
            let d = &*record;
            let texture = d.texture();
            DRAW_COLORS[i] =
                scenery_color::command(0, d.tint(), 128, 128 | if d.black == 1 { 256 } else { 0 });
            let pool = room.draw_pool_index(i);
            let info = match pool {
                Some(p) if p < POOL_INFO_CAPACITY => {
                    if POOL_INFO_VALID[p / 32] & (1 << (p % 32)) == 0 {
                        POOL_INFO[p] = derive_draw(d);
                        POOL_INFO_VALID[p / 32] |= 1 << (p % 32);
                    }
                    POOL_INFO[p]
                }
                _ => derive_draw(d),
            };
            DRAW_BOUNDS[i] = info as u8;
            let flags = DRAW_ENABLED | ((info >> 8) as u8 & STATIC_FLAGS);
            DRAW_FLAGS[i] = flags;
            let front = flags & FRONT != 0;
            let streamed = STREAMED[texture / 32] & (1 << (texture % 32)) != 0;
            if !front {
                PREFIX_TEXTURES[texture / 32] |= 1 << (texture % 32);
                if streamed {
                    BACK_PREBUILD_ALLOWED = false;
                }
            }
            if !streamed {
                if let Some(pool) = pool {
                    if let Some(&cert) = certs.get(pool) {
                        if cert != tile_coverage::NO_CERT {
                            if front {
                                tile_fronts += 1;
                                TILE_DRAWS[MAX_DRAWS - tile_fronts] = (i as u16, cert);
                            } else {
                                TILE_DRAWS[TILE_DRAW_COUNT] = (i as u16, cert);
                                TILE_DRAW_COUNT += 1;
                            }
                            TILE_CERTIFIED[i / 32] |= 1 << (i % 32);
                            // Saved tile rows also depend on foreground proofs
                            // outside the rectangular occluder candidate list.
                            PREFIX_TEXTURES[texture / 32] |= 1 << (texture % 32);
                        }
                    }
                }
            }
            if info & OCCLUDER_CANDIDATE != 0 {
                OCCLUDER_DRAWS[OCCLUDER_DRAW_COUNT] = i as u16;
                OCCLUDER_DRAW_COUNT += 1;
                // A prefix also depends on later foreground opacity proofs.
                // Any future template rebind must invalidate that saved work.
                PREFIX_TEXTURES[texture / 32] |= 1 << (texture % 32);
            }
            if info & SOLID_CORE_CANDIDATE != 0 {
                SOLID_CORE_DRAWS[SOLID_CORE_DRAW_COUNT] = i as u16;
                SOLID_CORE_DRAW_COUNT += 1;
            }
        }
        // The front entries, in draw order, after the back ones.
        let tiles = &mut *(&raw mut TILE_DRAWS);
        tiles[MAX_DRAWS - tile_fronts..].reverse();
        tiles.copy_within(MAX_DRAWS - tile_fronts.., TILE_DRAW_COUNT);
        TILE_DRAW_COUNT += tile_fronts;
        bind_tile_groups(room, coverage);
    }
    reset_visibility();
    crate::input::checkpoint();
}
/// The flags a draw's record and its scene textures fix for the whole scene.
const STATIC_FLAGS: u8 =
    FRONT | LEGAL_EXTENT | BLACK_AVERAGE | SCISSOR_CANDIDATE | EMPTY_TEXTURE | SOLID_BLACK;
/// `derive_draw` bits above the static flags (shifted by 8).
const OCCLUDER_CANDIDATE: u16 = 1 << 14;
const SOLID_CORE_CANDIDATE: u16 = 1 << 15;
/// What `derive_draw` found for each draw of the scene's shared pool, kept
/// for the scene: views share pool draws, so a view bind after the first
/// derives only draws no earlier view of the scene had. Cleared on a scene
/// change. Most of a bind's time was this derivation.
const POOL_INFO_CAPACITY: usize = opaque_groups::GROUP_POOL_CAPACITY;
static mut POOL_INFO: [u16; POOL_INFO_CAPACITY] = [0; POOL_INFO_CAPACITY];
static mut POOL_INFO_VALID: [u32; POOL_INFO_CAPACITY.div_ceil(32)] =
    [0; POOL_INFO_CAPACITY.div_ceil(32)];
/// The bound scene's draw pool, which `warm_step` derives ahead of the views
/// that will use it, and how far it has got.
static mut POOL_RECORDS: *const Draw = core::ptr::null();
static mut POOL_COUNT: usize = 0;
static mut WARM_NEXT: usize = 0;
/// Derive up to `count` more draws of the bound scene's pool into the cache,
/// in the idle time a queued flip leaves, so that the first bind of a view
/// finds them (a view bind mid-fight used to derive every one of its draws in
/// the frame that needed them). Packets are unaffected: a draw derives the
/// same way here or in `init`.
pub fn warm_step(count: usize) {
    unsafe {
        let end = (WARM_NEXT + count).min(POOL_COUNT);
        while WARM_NEXT < end {
            let p = WARM_NEXT;
            WARM_NEXT += 1;
            if POOL_INFO_VALID[p / 32] & (1 << (p % 32)) != 0 {
                continue;
            }
            let d = &*POOL_RECORDS.add(p);
            if d.texture() >= TEXTURE_COUNT {
                continue;
            }
            POOL_INFO[p] = derive_draw(d);
            POOL_INFO_VALID[p / 32] |= 1 << (p % 32);
        }
    }
}
/// A draw's bounds indices (low byte), static flags (bits 8..14) and its
/// occluder and solid-core admissions: everything `init` derives from the
/// record and the scene's texture state, none of it from the view.
#[inline(never)]
fn derive_draw(d: &Draw) -> u16 {
    unsafe {
        let texture = d.texture();
        let bounds = scenery_bounds::local_indices(&d.xy) as u16;
        let mut flags = u8::from(d.front != 0) * FRONT
            | u8::from(scenery_geometry::legal_extent_q8(&d.xy)) * LEGAL_EXTENT
            | u8::from(d.black == 1) * BLACK_AVERAGE;
        let cover = &COVERS[texture];
        if cover[0] == 0 {
            return bounds | ((flags | EMPTY_TEXTURE) as u16) << 8;
        }
        let xy = &d.xy;
        let axis = xy[1] == xy[3] && xy[0] == xy[4] && xy[2] == xy[6] && xy[5] == xy[7];
        if axis && cover[0] <= 4 {
            let uv = uv_rect(&TEMPLATES[texture]);
            let full = cover[0] == 1
                && cover[4] == 0
                && cover[5] == 0
                && cover[6] as u16 + 1 == uv.w
                && cover[7] as u16 + 1 == uv.h;
            if !full {
                flags |= SCISSOR_CANDIDATE;
            }
        }
        if SOLID_TEXTURES[texture / 32] & (1 << (texture % 32)) != 0 {
            flags |= SOLID_BLACK;
        }
        let mut info = bounds | (flags as u16) << 8;
        if flags & FRONT != 0
            && (flags & SOLID_BLACK != 0 || opaque_core_of(texture)[2] != 0)
            && occluder_admission::possible(&d.xy, OCCLUDER_MIN_AREA)
        {
            info |= OCCLUDER_CANDIDATE;
        }
        if flags & (FRONT | SOLID_BLACK) == SOLID_BLACK && !axis {
            // Optimization admission only: smaller/unproven geometry still
            // renders normally. Full opacity remains a live per-frame test.
            let w = (*xy.iter().step_by(2).max().unwrap() as i64
                - *xy.iter().step_by(2).min().unwrap() as i64
                + 255)
                >> 8;
            let h = (*xy.iter().skip(1).step_by(2).max().unwrap() as i64
                - *xy.iter().skip(1).step_by(2).min().unwrap() as i64
                + 255)
                >> 8;
            if w.min(320) * h.min(240) >= 8192 {
                info |= SOLID_CORE_CANDIDATE;
            }
        }
        info
    }
}
/// Groups are valid only when every original member is in this region. The
/// bounded table adds optional proofs; omitted groups retain normal rendering.
fn bind_tile_groups(room: &Room, coverage: CoverageView<'_>) {
    unsafe {
        TILE_GROUP_COUNT = 0;
        // One inverse map replaces repeated group×member×draw searches at
        // region activation. Duplicate pool references preserve the first local
        // occurrence, exactly matching the original membership resolver.
        const _: () = assert!(opaque_groups::GROUP_POOL_CAPACITY <= 4096);
        let mut pool_to_local = [u16::MAX; opaque_groups::GROUP_POOL_CAPACITY];
        for i in 0..DRAW_COUNT {
            if i & 31 == 0 {
                crate::input::checkpoint();
            }
            if let Some(pool) = room.draw_pool_index(i) {
                if let Some(slot) = pool_to_local.get_mut(pool) {
                    if *slot == u16::MAX {
                        *slot = i as u16;
                    }
                }
            }
        }
        for group in coverage.groups() {
            crate::input::checkpoint();
            let Some(local) = opaque_groups::resolve_indexed(group, &pool_to_local) else {
                continue;
            };
            let members = local.members.iter().copied().take_while(|&i| i != u16::MAX);
            if !members.clone().all(|i| {
                let texture = draw_record(i as usize).texture();
                DRAW_FLAGS[i as usize] & (FRONT | SOLID_BLACK | BLACK_AVERAGE)
                    == (FRONT | SOLID_BLACK | BLACK_AVERAGE)
                    && STREAMED[texture / 32] & (1 << (texture % 32)) == 0
            }) {
                continue;
            }
            if TILE_GROUP_COUNT == MAX_TILE_GROUPS {
                break;
            }
            for i in members {
                let i = i as usize;
                let texture = draw_record(i).texture();
                TILE_CERTIFIED[i / 32] |= 1 << (i % 32);
                PREFIX_TEXTURES[texture / 32] |= 1 << (texture % 32);
            }
            TILE_GROUPS[TILE_GROUP_COUNT] = local;
            TILE_GROUP_COUNT += 1;
        }
    }
}

/// Every later opaque rectangle of this frame: whole quads of solid black
/// textures and nonzero STP-clear cores of any texture, at full opacity. Draw order is
/// every back draw in index order, then every front draw.
#[inline(never)]
fn collect_occluders(camera: (i32, i32)) {
    unsafe {
        let scratch = occlusion_scratch::base();
        let occluders = &mut *Scratch::occluders(scratch);
        occluders.clear();
        for candidate in 0..OCCLUDER_DRAW_COUNT {
            if candidate & 31 == 0 {
                crate::input::checkpoint();
            }
            let i = OCCLUDER_DRAWS[candidate] as usize;
            let d = draw_record(i);
            // Admission proved nonempty art with an opaque core.
            // Gameplay may still hide or fade it, so check the live state.
            if DRAW_FLAGS[i] & DRAW_ENABLED == 0 || OPACITIES[i] != 128 {
                continue;
            }
            let solid = DRAW_FLAGS[i] & SOLID_BLACK != 0;
            let core = opaque_core_of(d.texture());
            if !solid && core[2] == 0 {
                continue;
            }
            let cx = (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let cy = (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let v: [(i32, i32); 4] = core::array::from_fn(|k| {
                (
                    160 + ((d.xy[k * 2] - cx) >> 8),
                    120 - ((d.xy[k * 2 + 1] - cy) >> 8),
                )
            });
            if v[0].1 != v[1].1 || v[0].0 != v[2].0 || v[1].0 != v[3].0 || v[2].1 != v[3].1 {
                continue;
            }
            let bw = (v[0].0.max(v[1].0).min(320) - v[0].0.min(v[1].0).max(0)).max(0);
            let bh = (v[0].1.max(v[2].1).min(240) - v[0].1.min(v[2].1).max(0)).max(0);
            if bw * bh < OCCLUDER_MIN_AREA * 2 {
                continue;
            }
            if !(DRAW_FLAGS[i] & LEGAL_EXTENT != 0 || scenery_geometry::legal(&v)) {
                continue;
            }
            let xy = v.map(|p| (p.0 as i16, p.1 as i16));
            let rect = if solid {
                let l = xy[0].0.min(xy[1].0).max(0);
                let r = xy[0].0.max(xy[1].0).min(320);
                let t = xy[0].1.min(xy[2].1).max(0);
                let b = xy[0].1.max(xy[2].1).min(240);
                if !(l < r && t < b) {
                    continue;
                }
                [l, t, r, b]
            } else {
                let uv = uv_rect(&TEMPLATES[d.texture()]);
                match (&mut *(&raw mut CORE_MAP_CACHE)).map(xy, uv.w, uv.h, core) {
                    Some(r) => r,
                    None => continue,
                }
            };
            let area = rect_area(&rect);
            if area < OCCLUDER_MIN_AREA {
                continue;
            }
            // Keep the largest rectangles: replace the smallest when full.
            let slot = if occluders.len() < MAX_OCCLUDERS {
                occluders.len()
            } else {
                let (k, smallest) = occluders
                    .as_slice()
                    .iter()
                    .enumerate()
                    .map(|(k, o)| (k, rect_area(&o.rect)))
                    .min_by_key(|&(_, a)| a)
                    .unwrap();
                if smallest >= area {
                    continue;
                }
                k
            };
            occluders.set(
                slot,
                Occluder {
                    front: DRAW_FLAGS[i] & FRONT != 0,
                    draw: i as u16,
                    rect,
                },
            );
        }
        collect_solid_core(camera, occluders);
        let union = &mut *Scratch::union(scratch);
        *union = [i16::MAX, i16::MAX, i16::MIN, i16::MIN];
        for o in occluders.as_slice() {
            *union = [
                union[0].min(o.rect[0]),
                union[1].min(o.rect[1]),
                union[2].max(o.rect[2]),
                union[3].max(o.rect[3]),
            ];
        }
    }
}
/// Add one conservative interior of a later opaque back mask. Source-order
/// tests in both occlusion paths restrict it to earlier back draws. Existing
/// foreground occluders keep their slots. No rendering geometry is altered.
#[inline(never)]
fn collect_solid_core(camera: (i32, i32), occluders: &mut Occluders) {
    unsafe {
        if occluders.len() == MAX_OCCLUDERS || SOLID_CORE_DRAW_COUNT == 0 {
            return;
        }
        let cache = &mut *(&raw mut SOLID_CORE_CACHE);
        let mut best = None;
        let mut score = OCCLUDER_MIN_AREA - 1;
        for candidate in 0..SOLID_CORE_DRAW_COUNT {
            let i = SOLID_CORE_DRAWS[candidate] as usize;
            let d = draw_record(i);
            if DRAW_FLAGS[i] & DRAW_ENABLED == 0 || OPACITIES[i] != 128 {
                continue;
            }
            let cx = (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let cy = (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let v = core::array::from_fn::<_, 4, _>(|k| {
                (
                    160 + ((d.xy[k * 2] - cx) >> 8),
                    120 - ((d.xy[k * 2 + 1] - cy) >> 8),
                )
            });
            let l = v.iter().map(|p| p.0).min().unwrap().max(0);
            let r = v.iter().map(|p| p.0).max().unwrap().min(320);
            let t = v.iter().map(|p| p.1).min().unwrap().max(0);
            let b = v.iter().map(|p| p.1).max().unwrap().min(240);
            if l >= r || t >= b || (r - l) * (b - t) < 8192 {
                continue;
            }
            // This is exactly the original packet path. Oversized repair quads
            // remain untouched; there is no subdivision cost in this collector.
            if !(DRAW_FLAGS[i] & LEGAL_EXTENT != 0 || scenery_geometry::legal(&v)) {
                continue;
            }
            if let Some(rect) = cache.map(v.map(|(x, y)| (x as i16, y as i16))) {
                let area = rect_area(&rect);
                if area > score {
                    score = area;
                    best = Some((i, rect));
                }
            }
        }
        if let Some((i, rect)) = best {
            occluders.set(
                occluders.len(),
                Occluder {
                    front: false,
                    draw: i as u16,
                    rect,
                },
            );
            HK_SOLID_CORE_OCCLUDERS = HK_SOLID_CORE_OCCLUDERS.wrapping_add(1);
        }
        let (hits, misses) = cache.stats();
        HK_SOLID_CORE_CACHE_HITS = hits;
        HK_SOLID_CORE_CACHE_MISSES = misses;
    }
}
/// Translate host-certified masks; the grid owns no GPU or DMA references.
#[inline(never)]
fn collect_tile_coverage(camera: (i32, i32), coverage: CoverageView<'_>) {
    unsafe {
        let grid = &mut *Scratch::tile_owners(occlusion_scratch::base());
        let certs = coverage.tile_certs();
        let bits = coverage.tile_bits();
        let group_certs = coverage.group_certs();
        let group_bits = coverage.group_bits();
        TILE_MAX_RANK = 0;
        let (mut candidates, mut lookups, mut tiles) = (0u32, 0u32, 0u32);
        for candidate in (0..TILE_DRAW_COUNT).rev() {
            if candidate & 31 == 0 {
                crate::input::checkpoint();
            }
            let (draw, cert) = TILE_DRAWS[candidate];
            let i = draw as usize;
            let d = draw_record(i);
            if DRAW_FLAGS[i] & DRAW_ENABLED == 0 || OPACITIES[i] != 128 {
                continue;
            }
            let cx = (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let cy = (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let origin = (160 + ((d.xy[0] - cx) >> 8), 120 - ((d.xy[1] - cy) >> 8));
            if !tile_coverage::intersects(certs, cert, origin) {
                continue;
            }
            // Cropped certificate bounds lie inside every phase's source
            // AABB. Its tile preflight proves viewport overlap, satisfying
            // LEGAL_EXTENT's absolute-coordinate and triangle-edge contract.
            if DRAW_FLAGS[i] & LEGAL_EXTENT == 0 {
                let v = core::array::from_fn::<_, 4, _>(|k| {
                    (
                        160 + ((d.xy[k * 2] - cx) >> 8),
                        120 - ((d.xy[k * 2 + 1] - cy) >> 8),
                    )
                });
                if !scenery_geometry::legal(&v) {
                    continue;
                }
            }
            let rank = tile_coverage::rank(i, DRAW_FLAGS[i] & FRONT != 0);
            candidates += 1;
            let (added, reads) = tile_coverage::claim(certs, bits, cert, origin, rank, grid);
            if added != 0 && TILE_MAX_RANK == 0 {
                TILE_MAX_RANK = rank;
            }
            tiles += added;
            lookups += reads;
        }
        // Joint seam coverage remains valid only until its earliest member.
        // Upgrade earlier owners without displacing any later single proof.
        for group in &TILE_GROUPS[..TILE_GROUP_COUNT] {
            crate::input::checkpoint();
            let first = draw_record(group.members[0] as usize);
            let cx = (((camera.0 >> 8) as i64 * first.scale as i64) >> 12) as i32;
            let cy = (((camera.1 >> 8) as i64 * first.scale as i64) >> 12) as i32;
            let origin = (
                160 + ((first.xy[0] - cx) >> 8),
                120 - ((first.xy[1] - cy) >> 8),
            );
            if !opaque_groups::intersects(group_certs, group.certificate, origin) {
                continue;
            }
            let mut guard = opaque_groups::Guard::new();
            for i in group.members.iter().copied().take_while(|&i| i != u16::MAX) {
                let i = i as usize;
                let d = draw_record(i);
                let eligible = DRAW_FLAGS[i] & (DRAW_ENABLED | FRONT | SOLID_BLACK | BLACK_AVERAGE)
                    == (DRAW_ENABLED | FRONT | SOLID_BLACK | BLACK_AVERAGE)
                    && OPACITIES[i] == 128
                    && d.scale == first.scale
                    && STREAMED[d.texture() / 32] & (1 << (d.texture() % 32)) == 0;
                let v = core::array::from_fn(|k| {
                    (
                        160 + ((d.xy[k * 2] - cx) >> 8),
                        120 - ((d.xy[k * 2 + 1] - cy) >> 8),
                    )
                });
                guard.push(tile_coverage::rank(i, true), d.scale, eligible, &v);
            }
            if let Some((origin, rank)) = guard.finish() {
                let (added, reads) = opaque_groups::claim(
                    group_certs,
                    group_bits,
                    group.certificate,
                    origin,
                    rank,
                    grid,
                );
                if added != 0 {
                    TILE_MAX_RANK = TILE_MAX_RANK.max(rank);
                }
                candidates += 1;
                tiles += added;
                lookups += reads;
            }
        }
        (&mut *(&raw mut TILE_EVENTS))
            .build(grid, &mut *Scratch::tile_rows(occlusion_scratch::base()));
        HK_TILE_COVERAGE_CANDIDATES = HK_TILE_COVERAGE_CANDIDATES.wrapping_add(candidates);
        HK_TILE_COVERAGE_LOOKUPS = HK_TILE_COVERAGE_LOOKUPS.wrapping_add(lookups);
        HK_TILE_COVERAGE_TILES = HK_TILE_COVERAGE_TILES.wrapping_add(tiles);
    }
}
/// Called after both scenery passes and all current frame packet emission.
/// Only CPU scratch/events change; prior DMA packets and VRAM remain untouched.
/// Any later certified eligibility change or camera change rejects this result.
#[inline(never)]
pub fn prepare_tile_coverage(camera: (i32, i32), coverage: CoverageView<'_>) {
    unsafe {
        if (&*(&raw const TILE_PREPARED)).matches(camera, TILE_GENERATION) {
            return;
        }
        let scratch = occlusion_scratch::base();
        // Switch the exclusive workspace to owner collection; claim requires0.
        Scratch::initialize_owners_at(scratch);
        collect_tile_coverage(camera, coverage);
        // Events and rows now own all coverage information. No owner reference
        // survives the collector; reinitialize the overlaid drawing workspace.
        Scratch::initialize_draw_at(scratch);
        (&mut *(&raw mut TILE_PREPARED)).save(
            camera,
            TILE_GENERATION,
            &*Scratch::tile_rows(scratch),
            TILE_MAX_RANK,
        );
        HK_TILE_PREPARES = HK_TILE_PREPARES.wrapping_add(1);
    }
}
/// Prepare CPU packet words only after the previous final DMA has drained.
/// Main supplies the next framebuffer and final visual state, once per new
/// simulation clock while its presentation is queued. A replacement discards
/// the old prefix before touching tile Events. No GP0 commands are issued.
#[inline(never)]
pub fn prepare_back_prefix(camera: (i32, i32), framebuffer_y: u16, coverage: CoverageView<'_>) {
    let camera = scenery_camera(camera);
    if !crate::presentation::queued() || !psx_rt::interrupts::gp1_queue_pending() {
        return;
    }
    unsafe {
        let prefix = &mut *(&raw mut BACK_PREFIX);
        if prefix.next == DRAW_COUNT
            && prefix.matches(camera, framebuffer_y)
            && draw_visibility::matches_snapshot(prefix.state())
        {
            return;
        }
        prefix.invalidate();
        prepare_tile_coverage(camera, coverage);
        if !BACK_PREBUILD_ALLOWED || !psx_rt::interrupts::gp1_queue_pending() {
            return;
        }
        let Some(count) = draw_visibility::sparse_snapshot(prefix.state_mut()) else {
            return;
        };
        // Pair the SDK's completed async DMA lease before reusing packet RAM.
        // Animation uploads/completion still belong to the post-flip begin.
        gpu::submit_linked_list_wait();
        collect_occluders(camera);
        restore_tile_coverage(camera);
        if !psx_rt::interrupts::gp1_queue_pending() {
            return;
        }
        USED = 0;
        EXTRA_USED = 0;
        KICKED = 0;
        CHUNK_COUNT = 0;
        FRAMEBUFFER_Y = framebuffer_y;
        let started = psx_io::timers::counter(psx_io::timers::Timer::Timer1);
        let (emitted, next) = scenery_range(camera, false, 0, true);
        let scratch = occlusion_scratch::base();
        (&mut *(&raw mut PREFIX_OCCLUDERS)).save(
            (&*Scratch::occluders(scratch)).as_slice(),
            &*Scratch::union(scratch),
        );
        prefix.save(
            camera,
            framebuffer_y,
            count,
            next,
            USED,
            EXTRA_USED,
            emitted,
            TILE_MAX_RANK,
            &*Scratch::tile_rows(occlusion_scratch::base()),
        );
        HK_BACK_PREBUILDS = HK_BACK_PREBUILDS.wrapping_add(1);
        HK_BACK_PREBUILD_LINES = HK_BACK_PREBUILD_LINES.wrapping_add(
            psx_io::timers::counter(psx_io::timers::Timer::Timer1).wrapping_sub(started) as u32,
        );
    }
}
#[inline]
fn tile_eligibility_changed(draw: usize) {
    unsafe {
        if TILE_CERTIFIED[draw / 32] & (1 << (draw % 32)) != 0 {
            TILE_ELIGIBILITY_DIRTY = true;
            TILE_GENERATION = TILE_GENERATION.wrapping_add(1);
            (&mut *(&raw mut TILE_PREPARED)).invalidate();
        }
    }
}
/// Stage tile scissors after the existing exact alpha/core/occlusion pieces.
/// The output scratch is disjoint. Failed bounds or budget checks retain input.
#[inline(never)]
fn tile_visibility(input: &Pieces, out: &mut Pieces, rank: u16, core_packets: usize) -> i32 {
    unsafe {
        if rank >= TILE_MAX_RANK || input.len() == 0 {
            return 0;
        }
        let before = input.as_slice().iter().map(rect_area).sum::<i32>();
        if before < occlusion::MIN_SAVED_PIXELS {
            return 0;
        }
        let rows = &*Scratch::tile_rows(occlusion_scratch::base());
        if !occlusion::tile_runs::any_hidden(input.as_slice(), rows) {
            return 0;
        }
        if !occlusion::tile_runs::visible_runs_rows(input.as_slice(), rows, out) {
            HK_TILE_FALLBACKS = HK_TILE_FALLBACKS.wrapping_add(1);
            return 0;
        }
        let removed = before - out.as_slice().iter().map(rect_area).sum::<i32>();
        if removed < occlusion::MIN_SAVED_PIXELS {
            return 0;
        }
        let extra = (out.len() + core_packets).saturating_sub(1);
        if extra > SCENERY_ALLOWANCE.saturating_sub(EXTRA_USED) {
            HK_TILE_FALLBACKS = HK_TILE_FALLBACKS.wrapping_add(1);
            return 0;
        }
        HK_TILE_OCCLUSION_QUADS = HK_TILE_OCCLUSION_QUADS.wrapping_add(1);
        HK_TILE_SAVED_PIXELS = HK_TILE_SAVED_PIXELS.wrapping_add(removed as u32);
        removed
    }
}
fn template(t: Texture, tp: Tpage, u: u16, v: u16) -> QuadTextured {
    let palette = crate::disc::scene_palette_base(unsafe { ACTIVE_BANK }) + t.palette as usize;
    let (x, y) = hk_cache::residency::clut_xy(
        palette / hk_cache::residency::CLUTS,
        palette % hk_cache::residency::CLUTS,
    );
    let cl = Clut::new(x, y);
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
// Never inlined: `main` is one 611-line function whose generated code sits
// against the 128 KB a MIPS PC16 branch reaches, and absorbing a body this
// size pushes a branch out of range. The assembler reports only "out of
// range PC16 fixup" when that happens, naming nothing.
#[inline(never)]
pub fn begin(room: &Room, animation_ids: &[u16], framebuffer_y: u16) -> hk_cache::Stats {
    begin_frame(framebuffer_y);
    bind_animation(room, animation_ids, false)
}
/// The packet half of `begin`: the frame's packet pool and occlusion workspace.
/// Prior-frame references have ended; DMA only owns PACKETS.
#[inline(never)]
pub fn begin_frame(framebuffer_y: u16) {
    unsafe {
        // A tail of about 15 to 23 lines is the last chunk's own drawing, not
        // a GPU-bound frame: the occluder holes and tile runs cost more CPU
        // than they spare there (secret-kp, measured with 16, 24 and 32).
        // Which resource the last slow frame ran out of decides whether the
        // exact trimming below (occluder holes, tile runs, alpha scissors, flat
        // cores) is worth its CPU time. It saves GPU pixels and spends CPU
        // cycles. A frame past two vblanks whose GPU kept working well after the
        // CPU's last kick (a long tail) was GPU-bound: trim. One whose tail was
        // short was CPU-bound: stop trimming, since the GPU has room for the
        // pixels (Crossroads_04 runs the GPU at about 0.7 vblank). Frames inside
        // the budget change nothing, and a loading gap is not a slow frame.
        let tail = HK_FRAME_FLIP_LINES.saturating_sub(HK_FRAME_LAST_KICK_LINES);
        let vblanks = crate::presentation::HK_FRAME_VBLANKS;
        if vblanks > 2 && vblanks <= 8 {
            if TRIMMING && tail < TRIM_OFF_TAIL {
                TRIMMING = false;
                remember_cpu_view(true);
            } else if !TRIMMING && tail >= TRIM_ON_TAIL {
                TRIMMING = true;
                remember_cpu_view(false);
            }
        }
        CPU_BOUND = !TRIMMING;
        Scratch::initialize_at(occlusion_scratch::base());
        USED = 0;
        KICKED = 0;
        CHUNK_COUNT = 0;
        EXTRA_USED = 0;
        FRAMEBUFFER_Y = framebuffer_y;
    }
}
/// Whether the back pass may be built and kicked before this frame's
/// animation working set is bound (see frame::render): no back draw streams
/// through the animation cache, and no frame of the view's room binds a
/// texture the back pass, its occluders or its tile proofs read, so the
/// rebinding below cannot change one of its packets. Worked out after an
/// activation by scanning every frame of the room, a slice per asking frame
/// (the whole scan in one frame cost about as much as the rest of an activation
/// and pushed a boss-fight frame past two vblanks). Only a frame after a
/// GPU-bound one asks, so the slice spends the CPU time the last frame left
/// idle: its GPU tail less EARLY_SCAN_MARGIN lines. Until the scan ends the
/// answer is "not early", which draws the same packets. Remembered per view
/// for the scene, so crossing back and forth between views pays once.
static mut EARLY_BACK: u8 = EARLY_UNKNOWN;
static mut CPU_BOUND: bool = false;
/// Whether the last slow frame was GPU-bound (see `begin_frame`), and the GPU
/// tails in lines after the CPU's last kick that tell the two apart: under
/// TRIM_OFF_TAIL the CPU was the limit, from TRIM_ON_TAIL the GPU was.
static mut TRIMMING: bool = true;
/// Views (bank, view) found CPU-bound, so they start without trimming when
/// they are bound again instead of paying a slow frame to find out.
static mut CPU_VIEWS: [(usize, usize); 8] = [(usize::MAX, 0); 8];
static mut CPU_VIEW_NEXT: usize = 0;
#[inline(never)]
fn remember_cpu_view(cpu: bool) {
    unsafe {
        let key = (ACTIVE_BANK, BOUND_VIEW);
        if cpu {
            if !CPU_VIEWS.contains(&key) {
                CPU_VIEWS[CPU_VIEW_NEXT] = key;
                CPU_VIEW_NEXT = (CPU_VIEW_NEXT + 1) % CPU_VIEWS.len();
            }
        } else {
            for v in CPU_VIEWS.iter_mut() {
                if *v == key {
                    *v = (usize::MAX, 0);
                }
            }
        }
    }
}
const TRIM_OFF_TAIL: u32 = 60;
const TRIM_ON_TAIL: u32 = 140;
const EARLY_UNKNOWN: u8 = 0;
const EARLY_NO: u8 = 1;
const EARLY_YES: u8 = 2;
static mut EARLY_VIEWS: [(usize, usize, u8); 8] = [(usize::MAX, 0, EARLY_UNKNOWN); 8];
static mut EARLY_NEXT: usize = 0;
const EARLY_SCAN_MARGIN: u32 = 24;
/// The scan in progress: its view key and the next room frame to look at.
static mut EARLY_SCAN: (usize, usize, usize) = (usize::MAX, usize::MAX, 0);
#[inline(never)]
fn early_back_allowed(room: &Room) -> bool {
    unsafe {
        if EARLY_BACK == EARLY_UNKNOWN {
            let key = (ACTIVE_BANK, BOUND_VIEW);
            if let Some(e) = EARLY_VIEWS.iter().find(|e| (e.0, e.1) == key) {
                EARLY_BACK = e.2;
            } else {
                if (EARLY_SCAN.0, EARLY_SCAN.1) != key {
                    EARLY_SCAN = (key.0, key.1, 0);
                }
                let budget = HK_FRAME_FLIP_LINES
                    .saturating_sub(HK_FRAME_LAST_KICK_LINES)
                    .saturating_sub(EARLY_SCAN_MARGIN)
                    .max(8);
                let started = frame_lines();
                let mut end = EARLY_SCAN.2;
                let mut conflict = !BACK_PREBUILD_ALLOWED;
                while !conflict && end < room.counts[3] {
                    let f = end;
                    end += 1;
                    if f & 31 == 0 {
                        crate::input::checkpoint();
                    }
                    let base = u32_at(room.frame(f), 0) as usize;
                    if base >= room.counts[1] {
                        conflict = true;
                        break;
                    }
                    let (base, cols, rows) = room.frame_grid(f);
                    conflict = (base..base + cols * rows).any(|id| {
                        id >= MAX_TEXTURES || PREFIX_TEXTURES[id / 32] & (1 << (id % 32)) != 0
                    });
                    if f & 15 == 15 && frame_lines().wrapping_sub(started) >= budget {
                        break;
                    }
                }
                EARLY_SCAN.2 = end;
                if !conflict && end < room.counts[3] {
                    return false;
                }
                let value = if conflict { EARLY_NO } else { EARLY_YES };
                EARLY_VIEWS[EARLY_NEXT] = (key.0, key.1, value);
                EARLY_NEXT = (EARLY_NEXT + 1) % EARLY_VIEWS.len();
                EARLY_SCAN = (usize::MAX, usize::MAX, 0);
                EARLY_BACK = value;
            }
        }
        EARLY_BACK == EARLY_YES
    }
}
#[no_mangle]
pub static mut HK_EARLY_BACK_CONFLICTS: u32 = 0;
#[no_mangle]
pub static mut HK_EARLY_BACK_FRAMES: u32 = 0;
/// Only worth it while the GPU is the bottleneck: the uploads may have to
/// wait for the early back pass's DMA, which costs a CPU-bound frame time. The
/// last frame's GPU tail (its flip was queued this many lines after its final
/// CPU kick) says which; either order emits the same packets.
const EARLY_BACK_TAIL_LINES: u32 = 40;
pub fn early_back(room: &Room) -> bool {
    unsafe {
        HK_FRAME_FLIP_LINES > HK_FRAME_LAST_KICK_LINES
            && HK_FRAME_FLIP_LINES - HK_FRAME_LAST_KICK_LINES >= EARLY_BACK_TAIL_LINES
            && early_back_allowed(room)
    }
}
/// Wait until the GPU DMA channel has taken every kicked packet, so the CPU
/// may write GP0 directly (animation uploads) without interleaving with it.
#[inline(never)]
pub fn wait_dma_idle() {
    while dma_pending() {
        crate::input::checkpoint();
    }
}
/// The animation half of `begin`: upload missing frames and point their
/// templates at the cache slots. After an early back pass the uploads wait for
/// its DMA to finish (animation_cache::prepare) and land before every packet
/// that samples them.
#[inline(never)]
pub fn bind_animation(room: &Room, animation_ids: &[u16], early: bool) -> hk_cache::Stats {
    let stats = animation_cache::prepare(room, animation_ids);
    unsafe {
        for &id in animation_ids {
            // Shade keys have no room texture or template; they are drawn
            // through resident_quad with the slot UV from animation_uv.
            if id >= crate::shade::KEY_BASE {
                continue;
            }
            // Future callers must not rebind any template referenced by a
            // retained BACK prefix or its foreground occluders, even if a
            // nonstreamed ID is supplied.
            if PREFIX_TEXTURES[id as usize / 32] & (1 << (id as usize % 32)) != 0 {
                (&mut *(&raw mut BACK_PREFIX)).invalidate();
                // An early back pass would already have drawn with the old
                // template. EARLY_BACK rules this out; the count proves it.
                if early {
                    HK_EARLY_BACK_CONFLICTS = HK_EARLY_BACK_CONFLICTS.wrapping_add(1);
                }
            }
            let t = room.texture(id as usize);
            let slot = animation_cache::slot(id);
            let (u, v) = animation_cache::uv(slot);
            let (px, py) = animation_cache::page(slot);
            TEMPLATES[id as usize] = template(t, Tpage::new(px, py, TexDepth::Bit4), u, v);
        }
    }
    stats
}
/// Every animation key one frame needs. A frame too large for one 64x64 slot
/// binds a rectangle of them, so it contributes one key per tile.
pub fn append_frame_keys(room: &Room, frame: usize, needed: &mut [u16], len: &mut usize) {
    let (base, cols, rows) = room.frame_grid(frame);
    assert!(*len + cols * rows <= needed.len(), "animation working set");
    for tile in 0..cols * rows {
        needed[*len] = (base + tile) as u16;
        *len += 1;
    }
}
/// The slot UV of a prepared animation key, for art with no room template.
pub fn animation_uv(id: u16) -> (u8, u8) {
    let (u, v) = animation_cache::uv(animation_cache::slot(id));
    (u as u8, v as u8)
}
/// The texture page word that goes with `animation_uv`. The slots span two
/// VRAM regions, so this is not the same page for every key.
pub fn animation_tpage_word(id: u16) -> u16 {
    let (x, y) = animation_cache::page(animation_cache::slot(id));
    Tpage::new(x, y, TexDepth::Bit4).uv_tpage_word(0)
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
pub const SOURCE_ALPHA_COVERAGE: u8 = 1;
#[no_mangle]
pub static mut HK_DYNAMIC_HIDDEN_QUADS: u32 = 0;
/// Whether a scene texture streams through the animation cache, rather than
/// sitting in the scene's texture pages.
pub fn streamed(id: usize) -> bool {
    unsafe { STREAMED[id / 32] & (1 << (id % 32)) != 0 }
}
pub fn texture(id: usize, verts: [(i16, i16); 4], tint: (u8, u8, u8)) {
    texture_material_alpha(id, verts, tint, 0);
}
/// The hit flash over an actor frame already drawn with `texture`: the same
/// quad through the white silhouette CLUT, Add-blended and modulated by `tint`.
/// It is optional, so it only spends packets the view's scissor allowance has
/// left; without them the flash is skipped and counted, never asserted.
pub fn texture_flash(id: usize, verts: [(i16, i16); 4], tint: (u8, u8, u8)) {
    unsafe {
        assert!(id < TEXTURE_COUNT);
        if EXTRA_USED >= EXTRA_ALLOWANCE || USED + 1 >= CAP {
            HK_FLASH_SKIPPED = HK_FLASH_SKIPPED.wrapping_add(1);
            return;
        }
        if occlusion_scratch::dynamic_hidden(
            verts,
            (&*Scratch::occluders(occlusion_scratch::base())).as_slice(),
        ) {
            return;
        }
        if STREAMED[id / 32] & (1 << (id % 32)) != 0 {
            animation_cache::slot(id as u16);
        }
        let mut words = quad_words(&TEMPLATES[id], verts, tint, None, 128);
        // Semi-transparent command bit, the silhouette CLUT, ABR 1 (B+F).
        words[0] |= 1 << 25;
        words[2] = (words[2] & 0xffff)
            | ((Clut::new(FLASH_CLUT_X, FLASH_CLUT_Y).uv_clut_word() as u32) << 16);
        words[4] = (words[4] & !(3 << 21)) | (1 << 21);
        PACKETS[USED].write_plain(words);
        USED += 1;
        EXTRA_USED += 1;
        HK_FLASH_QUADS = HK_FLASH_QUADS.wrapping_add(1);
    }
}
/// A texel rectangle `[x, y, w, h]` of one texture, measured from its origin:
/// art cooked as several sprites in one texture (battle_gate_art.rs packs every
/// arena gate pose into one animation slot), so it costs a view one texture
/// record however many poses it shows.
#[inline(never)]
pub fn texture_sub(id: usize, verts: [(i16, i16); 4], tint: (u8, u8, u8), rect: [u8; 4]) {
    unsafe {
        assert!(id < TEXTURE_COUNT);
        if occlusion_scratch::dynamic_hidden(
            verts,
            (&*Scratch::occluders(occlusion_scratch::base())).as_slice(),
        ) {
            HK_DYNAMIC_HIDDEN_QUADS = HK_DYNAMIC_HIDDEN_QUADS.wrapping_add(1);
            return;
        }
        assert!(USED < CAP);
        if STREAMED[id / 32] & (1 << (id % 32)) != 0 {
            animation_cache::slot(id as u16);
        }
        let t = &TEMPLATES[id];
        let (u, v) = (
            (t.uv0_clut & 255) as u8 + rect[0],
            ((t.uv0_clut >> 8) & 255) as u8 + rect[1],
        );
        let (r, b) = (u + rect[2] - 1, v + rect[3] - 1);
        PACKETS[USED].write_plain(quad_words(
            t,
            verts,
            tint,
            Some([(u, v), (r, v), (u, b), (r, b)]),
            128,
        ));
        USED += 1;
    }
}
#[cfg(feature = "hero-light")]
#[no_mangle]
pub static mut HK_GLOW_QUADS: u32 = 0;
#[cfg(feature = "hero-light")]
#[no_mangle]
pub static mut HK_GLOW_SKIPPED: u32 = 0;
/// The hero light's fan (hero_light.rs): GP0(26h) textured triangles. Like the
/// hit flash it only spends the frame's optional allowance and is skipped,
/// counted, without it.
#[cfg(feature = "hero-light")]
pub fn light_fan(tris: &[[u32; 7]]) -> u32 {
    unsafe {
        if tris.is_empty() {
            return 0;
        }
        if EXTRA_USED + tris.len() > EXTRA_ALLOWANCE || USED + tris.len() >= CAP {
            HK_GLOW_SKIPPED = HK_GLOW_SKIPPED.wrapping_add(1);
            return 0;
        }
        for tri in tris {
            let p = &mut PACKETS[USED];
            p.tag = 7 << 24;
            p.words[..7].copy_from_slice(tri);
            USED += 1;
            EXTRA_USED += 1;
        }
        HK_GLOW_QUADS = HK_GLOW_QUADS.wrapping_add(tris.len() as u32);
        tris.len() as u32
    }
}
/// The vignette centre on screen for this frame's scenery, or None.
/// The hero vignette (hero_light.rs) darkens each scenery draw by 1-alpha at
/// its nearest point to the Knight, through the draw's own texture
/// modulation: B*(1-alpha), at no GPU cost. The words are made in
/// `set_vignette`'s own loop, a sixteenth of the draws a frame, and the
/// scenery loop only reads the word instead of the draw's colour: any work
/// added inside that loop measured far dearer than the same work outside it.
/// A word's darkening is at most fifteen frames old; the vignette changes over
/// tens of pixels, so while the camera scrolls the darkening trails it
/// slightly. A word is indexed by the bound view's draw, so `init` turns the
/// vignette off and the next `set_vignette` remakes every word: before, a view
/// bind kept the old view's words for up to sixteen frames, and scenery took
/// another draw's colour and darkness (black where that draw was far from the
/// Knight), then came back a sixteenth at a time.
#[cfg(feature = "hero-vignette")]
static mut VIGNETTE_COLORS: [u32; MAX_DRAWS] = [0; MAX_DRAWS];
/// Each word's factor, so a colour change (a fade's gain, `refresh_color`)
/// re-darkens that word at once instead of waiting for its sixteenth.
#[cfg(feature = "hero-vignette")]
static mut VIGNETTE_FACTORS: [u16; MAX_DRAWS] = [256; MAX_DRAWS];
#[cfg(feature = "hero-vignette")]
static mut VIGNETTE_ON: bool = false;
/// Vignette words found not to match their own draw's colour and factor: a
/// word left from another view's draw, or from a colour that has changed
/// since. validate requires 0 on every route.
#[no_mangle]
pub static mut HK_VIGNETTE_STALE: u32 = 0;
#[cfg(feature = "hero-vignette")]
static mut VIGNETTE_CHECK: usize = 0;
#[cfg(feature = "hero-vignette")]
#[inline(always)]
fn vignette_word(c: u32, f: u32) -> u32 {
    let ch = |shift: u32| (((c >> shift) & 255) * f >> 8) << shift;
    if f < 256 {
        (c & 0xff00_0000) | ch(0) | ch(8) | ch(16)
    } else {
        c
    }
}
/// `DRAW_COLORS[draw]` changed: remake its vignette word with its factor.
#[inline]
#[allow(unused_variables)]
pub(super) fn vignette_recolor(draw: usize) {
    #[cfg(feature = "hero-vignette")]
    unsafe {
        if VIGNETTE_ON {
            VIGNETTE_COLORS[draw] = vignette_word(DRAW_COLORS[draw], VIGNETTE_FACTORS[draw] as u32);
        }
    }
}
#[cfg(feature = "hero-vignette")]
static mut VIGNETTE_PHASE: usize = 0;
#[cfg(feature = "hero-vignette")]
#[inline(never)]
pub fn set_vignette(centre: Option<(i32, i32)>, camera: (i32, i32)) {
    unsafe {
        let Some((kx, ky)) = centre else {
            VIGNETTE_ON = false;
            return;
        };
        let first = !VIGNETTE_ON;
        VIGNETTE_ON = true;
        VIGNETTE_PHASE = VIGNETTE_PHASE.wrapping_add(1);
        // Every draw on the first frame, then every sixteenth from a moving
        // start: stepping, not skipping, so idle entries cost nothing.
        let (start, step) = if first {
            (0, 1)
        } else {
            (VIGNETTE_PHASE & 15, 16)
        };
        // Every word must be its own draw's colour darkened by its own factor,
        // whenever it is read. One word a frame is checked, in turn, which
        // costs next to nothing and still sees the words a bind left behind
        // (they stay up to sixteen frames).
        if !first && DRAW_COUNT > 0 {
            if VIGNETTE_CHECK >= DRAW_COUNT {
                VIGNETTE_CHECK = 0;
            }
            let k = VIGNETTE_CHECK;
            VIGNETTE_CHECK += 1;
            if VIGNETTE_COLORS[k] != vignette_word(DRAW_COLORS[k], VIGNETTE_FACTORS[k] as u32) {
                HK_VIGNETTE_STALE = HK_VIGNETTE_STALE.wrapping_add(1);
            }
        }
        for i in (start..DRAW_COUNT).step_by(step) {
            let d = draw_record(i);
            // The draw's projected bounds, as render::scenery projects them.
            // A 32-bit product when it fits: |camera>>8| stays under 2^16 and
            // all but the nearest foreground layers' scales under 2^15.
            let (cx, cy) = if d.scale >= 0 && d.scale < 1 << 15 {
                (
                    ((camera.0 >> 8) * d.scale) >> 12,
                    ((camera.1 >> 8) * d.scale) >> 12,
                )
            } else {
                (
                    (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32,
                    (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32,
                )
            };
            let b = scenery_bounds::project_unclipped(&d.xy, DRAW_BOUNDS[i], cx, cy);
            let dx = if kx < b[0] {
                b[0] - kx
            } else if kx > b[2] {
                kx - b[2]
            } else {
                0
            };
            let dy = if ky < b[1] {
                b[1] - ky
            } else if ky > b[3] {
                ky - b[3]
            } else {
                0
            };
            let f = crate::hero_light::vignette_factor(dx, dy);
            VIGNETTE_FACTORS[i] = f as u16;
            VIGNETTE_COLORS[i] = vignette_word(DRAW_COLORS[i], f);
        }
    }
}
#[inline(always)]
#[allow(unused_variables)]
fn vignette_color(draw: usize, color: u32, opacity: u16) -> u32 {
    #[cfg(feature = "hero-vignette")]
    unsafe {
        if VIGNETTE_ON && opacity & 255 == 128 {
            return VIGNETTE_COLORS[draw];
        }
    }
    color
}
/// Whether scenery draw `draw` of the bound region has a binary black texture,
/// the only kind the subtractive reveal fade (`set_opacity`) can take down.
pub fn black_mask(draw: usize) -> bool {
    unsafe {
        draw < DRAW_COUNT && {
            let t = draw_record(draw).texture();
            BLACK_MASKS[t / 32] & (1 << (t % 32)) != 0
        }
    }
}
/// Whether scenery draw `draw` belongs to the front pass (source z < 0).
pub fn draw_is_front(draw: usize) -> bool {
    unsafe { draw < DRAW_COUNT && DRAW_FLAGS[draw] & FRONT != 0 }
}
/// A scenery draw moved by a world offset (Q16), for a hidden wall's recoil.
/// The caller has hidden the cooked draw for this frame.
pub fn draw_scenery_offset(draw: usize, offset: [i32; 2], camera: (i32, i32)) -> u32 {
    unsafe {
        if draw >= DRAW_COUNT {
            return 0;
        }
        let d = draw_record(draw);
        let shift = |v: i32| (((v >> 8) as i64 * d.scale as i64) >> 12) as i32;
        let (dx, dy) = (shift(offset[0]), shift(offset[1]));
        let xy: [i32; 8] = core::array::from_fn(|k| d.xy[k] + if k % 2 == 0 { dx } else { dy });
        draw_scenery_xy(draw, xy, camera)
    }
}
/// A scenery draw at other world corners (Q16, the cook's corner order), for
/// a cracked floor's sagging planks. The caller has hidden the cooked draw.
pub fn draw_scenery_quad(draw: usize, quad: [[i32; 2]; 4], camera: (i32, i32)) -> u32 {
    unsafe {
        if draw >= DRAW_COUNT {
            return 0;
        }
        let scale = draw_record(draw).scale as i64;
        let xy: [i32; 8] =
            core::array::from_fn(|k| (((quad[k / 2][k % 2] >> 8) as i64 * scale) >> 12) as i32);
        draw_scenery_xy(draw, xy, camera)
    }
}
/// One scenery texture at projected Q8 corners through the dynamic packet
/// path, with the draw's own tint and material. Streamed textures and
/// corners outside the GPU's range draw nothing.
fn draw_scenery_xy(draw: usize, xy: [i32; 8], camera: (i32, i32)) -> u32 {
    unsafe {
        let d = draw_record(draw);
        if STREAMED[d.texture() / 32] & (1 << (d.texture() % 32)) != 0
            || DRAW_FLAGS[draw] & EMPTY_TEXTURE != 0
            || USED >= CAP
        {
            return 0;
        }
        let cx = (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32;
        let cy = (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32;
        let v: [(i32, i32); 4] = core::array::from_fn(|k| {
            (
                160 + ((xy[k * 2] - cx) >> 8),
                120 - ((xy[k * 2 + 1] - cy) >> 8),
            )
        });
        if v.iter()
            .any(|p| p.0 < -1023 || p.0 > 1023 || p.1 < -1023 || p.1 > 1023)
        {
            return 0;
        }
        if v.iter().all(|p| p.0 < 0)
            || v.iter().all(|p| p.0 > 320)
            || v.iter().all(|p| p.1 < 0)
            || v.iter().all(|p| p.1 > 240)
        {
            return 0;
        }
        let verts = v.map(|p| (p.0 as i16, p.1 as i16));
        let opacity = 128
            | if DRAW_FLAGS[draw] & BLACK_AVERAGE != 0 {
                256
            } else {
                0
            };
        PACKETS[USED].write_plain(quad_words(
            &TEMPLATES[d.texture()],
            verts,
            d.tint(),
            None,
            opacity,
        ));
        USED += 1;
        1
    }
}
/// Small, independently resident Geo art uses the same bounded DMA packet pool.
/// A quad drawn over everything the scenery drew, the HUD's Geo coin: never
/// hidden by an occluder, because it is drawn after them all.
pub fn hud_quad(template: &QuadTextured, verts: [(i16, i16); 4], tint: (u8, u8, u8)) {
    unsafe {
        assert!(USED < CAP);
        PACKETS[USED].write_plain(quad_words(template, verts, tint, None, 128));
        USED += 1;
    }
}
pub fn resident_quad(template: &QuadTextured, verts: [(i16, i16); 4]) {
    resident_quad_tinted(template, verts, (128, 128, 128));
}
/// `resident_quad` modulated by `tint` (128 is the texel's own colour): the
/// break effects' authored particle colours.
pub fn resident_quad_tinted(template: &QuadTextured, verts: [(i16, i16); 4], tint: (u8, u8, u8)) {
    unsafe {
        if occlusion_scratch::dynamic_hidden(
            verts,
            (&*Scratch::occluders(occlusion_scratch::base())).as_slice(),
        ) {
            HK_DYNAMIC_HIDDEN_QUADS = HK_DYNAMIC_HIDDEN_QUADS.wrapping_add(1);
            return;
        }
        assert!(USED < CAP);
        PACKETS[USED].write_plain(quad_words(template, verts, tint, None, 128));
        USED += 1;
    }
}
/// The selected texture has baked source/lifetime alpha coverage0,1/2,1.
/// Keep straight RGB: Average supplies source-over at the half-alpha midpoint.
/// This is spatial alpha quantization, not arbitrary continuous GPU blending.
pub fn texture_material_alpha(id: usize, verts: [(i16, i16); 4], tint: (u8, u8, u8), material: u8) {
    assert!(material <= SOURCE_ALPHA_COVERAGE);
    unsafe {
        assert!(id < TEXTURE_COUNT);
        if occlusion_scratch::dynamic_hidden(
            verts,
            (&*Scratch::occluders(occlusion_scratch::base())).as_slice(),
        ) {
            HK_DYNAMIC_HIDDEN_QUADS = HK_DYNAMIC_HIDDEN_QUADS.wrapping_add(1);
            return;
        }
        assert!(USED < CAP);
        if STREAMED[id / 32] & (1 << (id % 32)) != 0 {
            animation_cache::slot(id as u16);
        }
        let mut words = quad_words(&TEMPLATES[id], verts, tint, None, 128);
        if material == SOURCE_ALPHA_COVERAGE {
            words[4] &= !(3 << 21);
        }
        PACKETS[USED].write_plain(words);
        USED += 1;
    }
}
fn quad_words(
    t: &QuadTextured,
    v: [(i16, i16); 4],
    tint: (u8, u8, u8),
    uv: Option<[(u8, u8); 4]>,
    opacity: u16,
) -> [u32; 9] {
    let mut words = [
        (t.color_cmd & 0xff00_0000)
            | tint.0 as u32
            | ((tint.1 as u32) << 8)
            | ((tint.2 as u32) << 16),
        vertex(v[0]),
        t.uv0_clut,
        vertex(v[1]),
        t.uv1_tpage,
        vertex(v[2]),
        t.uv2,
        vertex(v[3]),
        t.uv3,
    ];
    if let Some(uv) = uv {
        for i in 0..4 {
            words[2 + i * 2] =
                (words[2 + i * 2] & 0xffff_0000) | uv[i].0 as u32 | ((uv[i].1 as u32) << 8);
        }
    }
    // Bit8 is explicit source material admission; STP0 opaque texels ignore ABR.
    if opacity & 256 != 0 {
        words[4] &= !(3 << 21);
        // The nonzero opaque sentinel0x0001 must sample successfully, then
        // modulate to true black. Red127 maps its one red bit to zero; STP
        // remains clear so its coverage is opaque on every background.
        words[0] = (words[0] & !255) | 127;
    }
    let opacity = opacity & 255;
    if opacity < 128 {
        // An opaque black source texel ignores ABR; use the admitted alternate
        // CLUT to make only its solid mask samples subtractive. Intermediate
        // alpha approximates B-alpha*white, not Unity's B*(1-alpha).
        // Alpha0 is culled before packet construction;128 retains exact source.
        let a = opacity as u32;
        words[0] = (words[0] & 0xff00_0000) | a | (a << 8) | (a << 16);
        words[2] = (words[2] & 0xffff)
            | ((Clut::new(FADE_CLUT_X, FADE_CLUT_Y).uv_clut_word() as u32) << 16);
        words[4] = (words[4] & !(3 << 21)) | (2 << 21);
    }
    words
}
fn uv_rect(t: &QuadTextured) -> UvRect {
    let u = (t.uv0_clut & 255) as u16;
    let v = ((t.uv0_clut >> 8) & 255) as u16;
    UvRect {
        u,
        v,
        w: (t.uv1_tpage & 255) as u16 - u + 1,
        h: ((t.uv2 >> 8) & 255) as u16 - v + 1,
    }
}
/// Subtract later opaque black rectangles from rotated quads too. The
/// emulator clips spans per scanline and samples texels from per-pixel plane
/// equations, so the draw area cannot change what the surviving pixels
/// sample; verified by byte comparison of final frames (PERFORMANCE.md).
const ROTATED_OCCLUSION: bool = true;
#[no_mangle]
pub static mut HK_ROTATED_OCCLUSION_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_HIDDEN_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_HIDDEN_PIXELS: u32 = 0;
/// True when every on-screen pixel of the quad's bounding box is covered by
/// the union of occluder rectangles drawn after draw `i`.
fn hidden_by_occluders(bbox: &[i32; 4], i: usize, front: bool) -> bool {
    unsafe {
        let scratch = occlusion_scratch::base();
        let occluders = (&*Scratch::occluders(scratch)).as_slice();
        if occluders.is_empty() {
            return false;
        }
        let [l, t, r, b] = *bbox;
        if !(l < r && t < b) {
            return false;
        }
        let u = *Scratch::union(scratch);
        if l < i32::from(u[0]) || t < i32::from(u[1]) || r > i32::from(u[2]) || b > i32::from(u[3])
        {
            return false;
        }
        // Cheap passes first: keep only later occluders that overlap the box,
        // and answer at once when one of them contains it.
        let mut hits = 0u8;
        let mut n = 0;
        let mut covered = 0i32;
        for (k, o) in occluders.iter().enumerate() {
            if !((o.front && !front) || (o.front == front && o.draw as usize > i)) {
                continue;
            }
            let q = &o.rect;
            if i32::from(q[0]) >= r
                || i32::from(q[2]) <= l
                || i32::from(q[1]) >= b
                || i32::from(q[3]) <= t
            {
                continue;
            }
            if i32::from(q[0]) <= l
                && i32::from(q[1]) <= t
                && i32::from(q[2]) >= r
                && i32::from(q[3]) >= b
            {
                HK_HIDDEN_QUADS = HK_HIDDEN_QUADS.wrapping_add(1);
                HK_HIDDEN_PIXELS = HK_HIDDEN_PIXELS.wrapping_add((r - l) as u32 * (b - t) as u32);
                return true;
            }
            covered += (i32::from(q[2]).min(r) - i32::from(q[0]).max(l))
                * (i32::from(q[3]).min(b) - i32::from(q[1]).max(t));
            hits |= 1u8 << k;
            n += 1;
        }
        // The union of the overlaps is at most their summed area, so a box
        // larger than that sum keeps a visible pixel whatever the subtraction
        // below would do. Most partly covered boxes stop here instead of
        // paying for one rectangle subtraction per occluder.
        if n < 2 || covered < (r - l) * (b - t) {
            return false;
        }
        let packed_bbox = bbox.map(|v| v as i16);
        let pieces = &mut *Scratch::hidden(scratch);
        pieces.set_slice(core::slice::from_ref(&packed_bbox));
        for (k, o) in occluders.iter().enumerate() {
            if hits & (1u8 << k) == 0 {
                continue;
            }
            // Capacity overflow keeps the current
            // partition, which stays conservative.
            if subtract(pieces, &o.rect) {
                if pieces.len() == 0 {
                    HK_HIDDEN_QUADS = HK_HIDDEN_QUADS.wrapping_add(1);
                    HK_HIDDEN_PIXELS =
                        HK_HIDDEN_PIXELS.wrapping_add((r - l) as u32 * (b - t) as u32);
                    return true;
                }
            }
        }
        false
    }
}
#[no_mangle]
pub static mut HK_TILE_HIDDEN_QUADS: u32 = 0;
/// True when every 16-pixel tile under the quad's on-screen box is certified
/// opaque by strictly later draws (the rows `tile_visibility` trims with), so
/// none of its pixels survives. Skipping it up front saves the scissor, core,
/// hole and tile partitions that would otherwise reduce it to nothing.
#[inline]
fn tile_hidden(bbox: &[i32; 4], i: usize, front: bool) -> bool {
    unsafe {
        let [l, t, r, b] = *bbox;
        if TILE_MAX_RANK == 0 || tile_coverage::rank(i, front) >= TILE_MAX_RANK || !(l < r && t < b)
        {
            return false;
        }
        let first = l as usize >> 4;
        let end = (r as usize + 15) >> 4;
        let mask = ((1u32 << end) - 1) ^ ((1u32 << first) - 1);
        let rows = &*Scratch::tile_rows(occlusion_scratch::base());
        for row in (t as usize >> 4)..((b as usize + 15) >> 4) {
            if rows[row] & mask != mask {
                return false;
            }
        }
        HK_TILE_HIDDEN_QUADS = HK_TILE_HIDDEN_QUADS.wrapping_add(1);
        true
    }
}
#[inline(never)]
fn restore_tile_coverage(camera: (i32, i32)) {
    unsafe {
        if let Some(max_rank) = (&mut *(&raw mut TILE_PREPARED)).take(
            camera,
            TILE_GENERATION,
            &mut *Scratch::tile_rows(occlusion_scratch::base()),
        ) {
            TILE_MAX_RANK = max_rank;
            HK_TILE_PREPARE_HITS = HK_TILE_PREPARE_HITS.wrapping_add(1);
        } else {
            // Coverage is optional. CPU-busy frames use the established
            // alpha/rectangle path instead of delaying their first DMA.
            TILE_MAX_RANK = 0;
            HK_TILE_PREPARE_MISSES = HK_TILE_PREPARE_MISSES.wrapping_add(1);
        }
    }
}
/// Scenery reads the camera only as `camera>>8` (the Q8 world offset every
/// projection multiplies by a draw's scale). Dropping the bits below that
/// changes no packet, and lets the exact-key prefix and tile caches hit while
/// the follow camera's damping creeps by a few Q16 units a tick.
#[inline]
fn scenery_camera(camera: (i32, i32)) -> (i32, i32) {
    (camera.0 & !255, camera.1 & !255)
}
#[inline(never)]
pub fn scenery(camera: (i32, i32), front: bool) -> u32 {
    let camera = scenery_camera(camera);
    let mut start = 0;
    let mut emitted = 0;
    if !front {
        unsafe {
            let prefix = &mut *(&raw mut BACK_PREFIX);
            if prefix.matches(camera, FRAMEBUFFER_Y)
                && draw_visibility::matches_snapshot(prefix.state())
            {
                let scratch = occlusion_scratch::base();
                let saved = &*(&raw const PREFIX_OCCLUDERS);
                let occluders = &mut *Scratch::occluders(scratch);
                occluders.clear();
                for (i, &value) in saved.entries().iter().enumerate() {
                    occluders.set(i, value);
                }
                *Scratch::union(scratch) = saved.union();
                USED = prefix.packets;
                EXTRA_USED = prefix.extra;
                start = prefix.next;
                emitted = prefix.emitted;
                TILE_MAX_RANK = prefix.max_rank;
                (*Scratch::tile_rows(occlusion_scratch::base())).copy_from_slice(&prefix.rows);
                HK_BACK_PREBUILD_HITS = HK_BACK_PREBUILD_HITS.wrapping_add(1);
                HK_BACK_PREBUILD_PACKETS = HK_BACK_PREBUILD_PACKETS.wrapping_add(USED as u32);
            } else {
                HK_BACK_PREBUILD_MISSES = HK_BACK_PREBUILD_MISSES.wrapping_add(1);
                collect_occluders(camera);
                restore_tile_coverage(camera);
            }
            prefix.invalidate();
        }
    }
    emitted + scenery_range(camera, front, start, false).0
}
#[inline(never)]
fn scenery_range(camera: (i32, i32), front: bool, start: usize, prebuilding: bool) -> (u32, usize) {
    let mut n = 0;
    let mut next = start;
    let mut visited = 0u32;
    unsafe {
        let scratch = occlusion_scratch::base();
        let occluders = (&*Scratch::occluders(scratch)).as_slice();
        let occluder_union = &*Scratch::union(scratch);
        for i in start..DRAW_COUNT {
            // Never stop inside a source's repair/scissor group or after its
            // tile event without emitting it. One whole source is atomic.
            // The other pass's sources first: half of every pass is the other
            // pass's draws, and they need nothing but stepping over.
            let flags = DRAW_FLAGS[i];
            if (flags & FRONT != 0) != front {
                next = i + 1;
                continue;
            }
            if prebuilding && !psx_rt::interrupts::gp1_queue_pending() {
                break;
            }
            next = i + 1;
            visited = visited.wrapping_add(1);
            if visited & 31 == 0 {
                crate::input::checkpoint();
            }
            // Start rasterising after the first emitted draw; subsequent
            // chunks retain the established eight-source-draw cadence.
            if !prebuilding && (visited & 7 == 0 || (CHUNK_COUNT == 0 && USED != 0)) {
                kick_ready();
            }
            // Retire ownership only in the pass that actually draws this
            // source. Front ownership must survive the entire back pass so a
            // foreground mask hides back geometry regardless of authored
            // source index; the front pass then retires those owners in its
            // own source order. Invisible sources are still retired when they
            // belong to the active pass, preserving the event protocol.
            if TILE_MAX_RANK != 0 && (flags & FRONT != 0) == front {
                (&*(&raw const TILE_EVENTS)).reach(i, front, &mut *Scratch::tile_rows(scratch));
            }
            if flags & (EMPTY_TEXTURE | DRAW_ENABLED) != DRAW_ENABLED {
                continue;
            }
            let d = draw_record(i);
            // Distant parallax layers can overflow the pre-shift i32 product.
            // The cooker proves the shifted values/subtractions fit i32.
            let cx = (((camera.0 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let cy = (((camera.1 >> 8) as i64 * d.scale as i64) >> 12) as i32;
            let Some(bbox) = scenery_bounds::project_clipped(&d.xy, DRAW_BOUNDS[i], cx, cy) else {
                continue;
            };
            if hidden_by_occluders(&bbox, i, front) || tile_hidden(&bbox, i, front) {
                continue;
            }
            let mut v = [(0i32, 0i32); 4];
            for k in 0..4 {
                v[k] = (
                    160 + ((d.xy[k * 2] - cx) >> 8),
                    120 - ((d.xy[k * 2 + 1] - cy) >> 8),
                );
            }
            let t = &TEMPLATES[d.texture()];
            let uv = uv_rect(t);
            let opacity = OPACITIES[i] as u16 | if flags & BLACK_AVERAGE != 0 { 256 } else { 0 };
            if flags & LEGAL_EXTENT != 0 || scenery_geometry::legal(&v) {
                let xy = v.map(|p| (p.0 as i16, p.1 as i16));
                if flags & SOLID_BLACK != 0 && opacity & 255 == 128 && opacity & 256 != 0 {
                    assert!(USED < CAP);
                    PACKETS[USED].write_black(xy.map(vertex));
                    USED += 1;
                    n += 1;
                    continue;
                }
                // Cache RGB only: the current template still supplies its
                // command byte, including animation-template rebinding.
                let words = scenery_color::with_material(
                    [
                        t.color_cmd,
                        vertex(xy[0]),
                        t.uv0_clut,
                        vertex(xy[1]),
                        t.uv1_tpage,
                        vertex(xy[2]),
                        t.uv2,
                        vertex(xy[3]),
                        t.uv3,
                    ],
                    vignette_color(i, DRAW_COLORS[i], opacity),
                    opacity,
                    Clut::new(FADE_CLUT_X, FADE_CLUT_Y).uv_clut_word() as u32,
                );
                // Keep the original alpha partition alive for the capacity
                // fallback below while scenery scratch undergoes subtraction.
                let mut alpha_storage = core::mem::MaybeUninit::uninit();
                let scissors = if flags & SCISSOR_CANDIDATE != 0 {
                    let out = alpha_scissor_cache::MappedScissors::initialize(&mut alpha_storage);
                    if SCISSOR_CACHE[i].map_cached_into(
                        xy,
                        d.texture() as u16,
                        uv.w,
                        uv.h,
                        &COVERS[d.texture()],
                        &mut *(&raw mut SCISSOR_SECONDARY),
                        out,
                    ) {
                        Some(&*out)
                    } else {
                        None
                    }
                } else {
                    None
                };
                // Flat core and bounded multi-rectangle occlusion: exact partitions of
                // the original pixels, bounded by MAX_PIECES and the packet allowance.
                // Draw-area clipping retains original vertices and UV planes for
                // rotated quads too; a flat black core is exact only on the
                // black-average path, which zeroes the sentinel texel's red bit.
                let axis = xy[0].1 == xy[1].1
                    && xy[0].0 == xy[2].0
                    && xy[1].0 == xy[3].0
                    && xy[2].1 == xy[3].1;
                // On-screen bounding box of the quad; for axis-aligned quads
                // this is the quad itself.
                let bbox = bbox.map(|v| v as i16);
                let [bl, bt, br, bb] = bbox;
                let box_area = if bl < br && bt < bb {
                    (br - bl) as i32 * (bb - bt) as i32
                } else {
                    0
                };
                let pieces = &mut *Scratch::scenery(scratch);
                pieces.clear();
                if let Some(s) = &scissors {
                    pieces.set_slice(s.as_slice());
                } else if box_area > 0 {
                    pieces.push([bl, bt, br, bb]);
                }
                let mut core_rect = None;
                let mut saved = 0i32;
                // Ordinary RGB zero modulates every STP-clear texel to opaque
                // black; unlike the material-specific sentinel correction, its
                // larger general opaque core is also an exact flat replacement.
                let zero_tint = opacity == 128 && DRAW_COLORS[i] == 0;
                let core = if axis && box_area >= OCCLUDER_MIN_AREA {
                    if zero_tint {
                        opaque_core_of(d.texture())
                    } else {
                        core_of(d.texture())
                    }
                } else {
                    [0; 4]
                };
                if opacity & 255 == 128 && (opacity & 256 != 0 || zero_tint) && core[2] != 0 {
                    if let Some(core) = (&mut *(&raw mut CORE_MAP_CACHE)).map(xy, uv.w, uv.h, core)
                    {
                        if rect_area(&core) >= 256 && subtract(pieces, &core) {
                            core_rect = Some(core);
                        }
                    }
                }
                let holes = &mut *Scratch::holes(scratch);
                holes.clear();
                // Reject impossible savings before copying and ranking holes.
                // Alpha scissors and core subtraction only remove bbox pixels.
                let possible_holes = !CPU_BOUND
                    && (axis || ROTATED_OCCLUSION)
                    && box_area >= OCCLUDER_MIN_AREA
                    && !occluders.is_empty()
                    && occlusion::can_save_minimum(&bbox, occluder_union);
                for o in &occluders[..if possible_holes { occluders.len() } else { 0 }] {
                    if !((o.front && !front) || (o.front == front && o.draw as usize > i)) {
                        continue;
                    }
                    if !occlusion::can_save_minimum(&bbox, &o.rect) {
                        continue;
                    }
                    holes.push(o.rect);
                }
                if holes.len() != 0 {
                    saved = occlusion::subtract_many(pieces, holes.as_slice());
                }
                let tile_saved = if CPU_BOUND {
                    0
                } else {
                    tile_visibility(
                        pieces,
                        holes,
                        tile_coverage::rank(i, front),
                        usize::from(core_rect.is_some()),
                    )
                };
                let pieces = if tile_saved > 0 {
                    saved += tile_saved;
                    holes
                } else {
                    pieces
                };
                if core_rect.is_some() || saved > 0 {
                    let extra = (pieces.len() + usize::from(core_rect.is_some())).saturating_sub(1);
                    if extra <= SCENERY_ALLOWANCE.saturating_sub(EXTRA_USED) {
                        EXTRA_USED += extra;
                        for rect in pieces.as_slice() {
                            assert!(USED < CAP);
                            PACKETS[USED].write_scissored(words, *rect, FRAMEBUFFER_Y);
                            USED += 1;
                        }
                        if let Some(c) = core_rect {
                            let q = [(c[0], c[1]), (c[2], c[1]), (c[0], c[3]), (c[2], c[3])];
                            assert!(USED < CAP);
                            PACKETS[USED].write_black(q.map(vertex));
                            USED += 1;
                            HK_CORE_QUADS = HK_CORE_QUADS.wrapping_add(1);
                        }
                        if saved > 0 {
                            HK_OCCLUSION_QUADS = HK_OCCLUSION_QUADS.wrapping_add(1);
                            HK_OCCLUSION_SAVED_PIXELS =
                                HK_OCCLUSION_SAVED_PIXELS.wrapping_add(saved as u32);
                            if !axis {
                                HK_ROTATED_OCCLUSION_QUADS =
                                    HK_ROTATED_OCCLUSION_QUADS.wrapping_add(1);
                            }
                        }
                        n += pieces.len() as u32 + u32::from(core_rect.is_some());
                        continue;
                    }
                    HK_SCISSOR_CAPACITY_FALLBACKS = HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
                }
                if let Some(scissors) = scissors {
                    let extra = scissors.len().saturating_sub(1);
                    if extra <= SCENERY_ALLOWANCE.saturating_sub(EXTRA_USED) {
                        EXTRA_USED += extra;
                        for rect in scissors.as_slice() {
                            assert!(USED < CAP);
                            PACKETS[USED].write_scissored(words, *rect, FRAMEBUFFER_Y);
                            USED += 1;
                        }
                        HK_SCISSOR_QUADS = HK_SCISSOR_QUADS.wrapping_add(scissors.len() as u32);
                        HK_SCISSOR_SAVED_PIXELS =
                            HK_SCISSOR_SAVED_PIXELS.wrapping_add(scissors.saved_pixels());
                        n += scissors.len() as u32;
                        continue;
                    }
                    HK_SCISSOR_CAPACITY_FALLBACKS = HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
                }
                assert!(USED < CAP);
                PACKETS[USED].write_plain(words);
                USED += 1;
                n += 1;
            } else {
                // Oversized repair retains its existing packet construction.
                let gain = GAINS[i] as u16;
                let tint = (
                    (d.tint[0] as u16 * gain / 128) as u8,
                    (d.tint[1] as u16 * gain / 128) as u8,
                    (d.tint[2] as u16 * gain / 128) as u8,
                );
                n += repair(t, v, uv, tint, opacity, i, front);
            }
        }
    }
    if !prebuilding {
        kick_ready();
    }
    (n, next)
}
// Repair output is copied into DMA-owned packets before returning. No DMA or
// interrupt references this scratch; keeping it here avoids clearing1.5KiB
// and inflating the common scenery loop's stack for every oversized layer.
static mut REPAIR_CHILDREN: [Quad; 64] = [Quad::ZERO; 64];
#[no_mangle]
pub static mut HK_REPAIR_OCCLUSION_QUADS: u32 = 0;
#[no_mangle]
pub static mut HK_REPAIR_OCCLUSION_SAVED_PIXELS: u32 = 0;
#[inline(never)]
fn repair(
    t: &QuadTextured,
    v: [(i32, i32); 4],
    uv: UvRect,
    tint: (u8, u8, u8),
    opacity: u16,
    draw: usize,
    front: bool,
) -> u32 {
    crate::input::checkpoint();
    unsafe {
        let children = &mut *(&raw mut REPAIR_CHILDREN);
        let count = match scenery_geometry::subdivide_in(v, uv, children, &mut *GRID) {
            Ok(count) => count,
            Err(_) => {
                HK_SCENERY_REPAIR_FAILURES = HK_SCENERY_REPAIR_FAILURES.wrapping_add(1);
                panic!("scenery subdivision");
            }
        };
        let scratch = occlusion_scratch::base();
        let occluders = (&*Scratch::occluders(scratch)).as_slice();
        let union = &*Scratch::union(scratch);
        let pieces = &mut *Scratch::scenery(scratch);
        let holes = &mut *Scratch::holes(scratch);
        let mut emitted = 0;
        for child in &children[..count] {
            let words = quad_words(t, child.xy, tint, Some(child.uv), opacity);
            // Clip only the original emitted child packet. Restarting geometry
            // or UV interpolation here would change its samples and seams.
            let l = child.xy.iter().map(|p| p.0).min().unwrap().max(0);
            let r = child.xy.iter().map(|p| p.0).max().unwrap().min(320);
            let top = child.xy.iter().map(|p| p.1).min().unwrap().max(0);
            let bottom = child.xy.iter().map(|p| p.1).max().unwrap().min(240);
            if l >= r || top >= bottom {
                continue;
            }
            let bbox = [l, top, r, bottom];
            holes.clear();
            if !occluders.is_empty() && occlusion::can_save_minimum(&bbox, union) {
                for o in occluders {
                    if !((o.front && !front) || (o.front == front && o.draw as usize > draw)) {
                        continue;
                    }
                    if occlusion::can_save_minimum(&bbox, &o.rect) {
                        holes.push(o.rect);
                    }
                }
            }
            pieces.clear();
            pieces.push(bbox);
            let mut saved = if holes.len() != 0 {
                occlusion::subtract_many(pieces, holes.as_slice())
            } else {
                0
            };
            let tile_saved = tile_visibility(pieces, holes, tile_coverage::rank(draw, front), 0);
            let chosen = if tile_saved > 0 {
                saved += tile_saved;
                &*holes
            } else {
                &*pieces
            };
            if saved > 0 {
                // Mandatory repair children were reserved by the cooker.
                // Only final extra scissors consume the shared optional reserve.
                let extra = chosen.len().saturating_sub(1);
                if extra <= SCENERY_ALLOWANCE.saturating_sub(EXTRA_USED) {
                    EXTRA_USED += extra;
                    for rect in chosen.as_slice() {
                        assert!(USED < CAP);
                        PACKETS[USED].write_scissored(words, *rect, FRAMEBUFFER_Y);
                        USED += 1;
                    }
                    emitted += chosen.len() as u32;
                    HK_REPAIR_OCCLUSION_QUADS = HK_REPAIR_OCCLUSION_QUADS.wrapping_add(1);
                    HK_REPAIR_OCCLUSION_SAVED_PIXELS =
                        HK_REPAIR_OCCLUSION_SAVED_PIXELS.wrapping_add(saved as u32);
                    continue;
                }
                HK_SCISSOR_CAPACITY_FALLBACKS = HK_SCISSOR_CAPACITY_FALLBACKS.wrapping_add(1);
            }
            assert!(USED < CAP);
            PACKETS[USED].write_plain(words);
            USED += 1;
            emitted += 1;
        }
        HK_SCENERY_REPAIRS = HK_SCENERY_REPAIRS.wrapping_add(1);
        crate::input::checkpoint();
        emitted
    }
}

/// Kick every packet built since the last kick as its own DMA list if the
/// channel is free. Never waits: a busy channel just leaves the packets for a
/// later call or the final list, so the CPU stays ahead of the GPU.
pub fn kick_ready() {
    unsafe {
        if USED == KICKED || CHUNK_COUNT == CHUNKS || dma_pending() {
            return;
        }
        // The frame's first kick: clear GPUSTAT bit 24 so the GP0(1Fh) that
        // ends the final list is what raises it (the previous frame's flip has
        // landed; main waited for it before rendering this one).
        if CHUNK_COUNT == 0 {
            gpu::arm_draw_done();
        }
        let ot = &mut CHUNK_OTS[CHUNK_COUNT];
        ot.clear();
        for i in (KICKED..USED).rev() {
            let words = PACKETS[i].word_count();
            crate::display::ot_add(ot, 0, &mut PACKETS[i], words);
        }
        ot.submit_async();
        if CHUNK_COUNT == 0 {
            HK_FRAME_FIRST_KICK_LINES = frame_lines();
        }
        KICKED = USED;
        CHUNK_COUNT += 1;
    }
}
/// Build the final list (everything not yet kicked, then HUD and dialogue on
/// top). It is not kicked here: an earlier chunk may still be walking, and
/// main kicks it through `kick_front` once the channel is idle so the wait
/// runs simulation ticks and input checkpoints instead of spinning.
// Never inlined: `main` is one 611-line function whose generated code sits
// against the 128 KB a MIPS PC16 branch reaches, and absorbing a body this
// size pushes a branch out of range. The assembler reports only "out of
// range PC16 fixup" when that happens, naming nothing.
#[inline(never)]
pub fn submit(
    health: u16,
    max_health: u16,
    soul: u16,
    max_soul: u16,
    paused: bool,
    blue_health: u16,
    transition: u8,
) {
    crate::input::checkpoint();
    animation_cache::submit();
    // One owner, no mutation until main observes DMA and GPU completion.
    unsafe {
        let ot = &mut *(&raw mut OT);
        ot.clear();
        // The list's last node is GP0(1Fh): psx-rt applies the queued flip
        // only once the GPU has drawn everything before it. Everything below
        // goes in at slot 0, ahead of it.
        ot.end_with_draw_done();
        // OT insertion prepends: this packet executes after scenery and HUD.
        if transition != 0 {
            TRANSITION = gpu::prim::QuadGouraudBlended::new(
                [(0, 0), (320, 0), (0, 240), (320, 240)],
                [(transition, transition, transition); 4],
                gpu::material::BlendMode::Subtract,
            );
            crate::display::ot_add(
                ot,
                0,
                &mut *(&raw mut TRANSITION),
                gpu::prim::QuadGouraudBlended::WORDS,
            );
        }
        // BRIGHTNESS (display.rs): over everything below, under the fade to black.
        crate::display::append(ot);
        crate::dialogue::append(ot);
        // Between the HUD and the panel text: the map's backdrop and rooms go
        // over the world and the HUD, its area name over them.
        crate::game_map::append(ot);
        // The Shaman's black and fade: over the world and the HUD, under the
        // panel text, so the spell's message shows on black.
        crate::shaman::append(ot);
        crate::title_card::append(ot);
        crate::hud::append(ot, health, max_health, soul, max_soul, paused, blue_health);
        for i in (KICKED..USED).rev() {
            if i & 31 == 0 {
                crate::input::checkpoint();
            }
            let words = PACKETS[i].word_count();
            crate::display::ot_add(ot, 0, &mut PACKETS[i], words);
        }
        KICKED = USED;
    }
    crate::input::checkpoint();
}
/// Call only after `dma_pending()` returned false for the last kicked chunk.
pub fn kick_front() {
    // A frame with no earlier chunk arms here, before its only kick.
    if unsafe { CHUNK_COUNT } == 0 {
        gpu::arm_draw_done();
    }
    unsafe {
        (&*(&raw const OT)).submit_async();
        HK_FRAME_LAST_KICK_LINES = frame_lines();
        if CHUNK_COUNT == 0 {
            HK_FRAME_FIRST_KICK_LINES = HK_FRAME_LAST_KICK_LINES;
        }
    }
}
