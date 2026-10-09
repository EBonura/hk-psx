//! Twenty-four 64x64 4bpp slots: eight in the strip beside the two
//! framebuffers and sixteen in the texture page a static scenery page gave up.
//! Room palettes remain immutable and resident; only texels are replaced.
use hk_cache::residency;
use hk_cache::{Cache, Stats};
use hk_format::Room;
use psx_hw::gpu::{gp0, GpuStat};
use psx_vram::VramRect;

static mut CACHE: Cache = Cache::new();
#[no_mangle]
pub static mut HK_ANIM_CACHE_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_ANIM_CACHE_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_ANIM_UPLOAD_BYTES: u32 = 0;
#[no_mangle]
pub static mut HK_ANIM_UPLOAD_MAX_FRAME: u32 = 0;

pub fn init() {
    unsafe { CACHE = Cache::new() };
}
/// Pixel UV of a slot inside its own texture page.
pub fn uv(slot: usize) -> (u16, u16) {
    let (_, _, u, v) = residency::slot_placement(slot);
    (u, v)
}
/// The texture page origin a slot sits on, in VRAM halfwords. The slots are
/// not all in the same page, so a draw has to ask rather than assume.
pub fn page(slot: usize) -> (u16, u16) {
    let (x, y, _, _) = residency::slot_placement(slot);
    (x, y)
}
/// Where a slot's texels go, for an upload of `width` by `height` pixels.
fn dest(slot: usize, width: u16, height: u16) -> VramRect {
    let (x, y, u, v) = residency::slot_placement(slot);
    VramRect::new(x + u / 4, y + v, width.div_ceil(4), height)
}
/// Called before the frame clear or any submitted drawing commands. The prior
/// frame's idle VBlank acknowledgement must already have called `complete`.
pub fn prepare(room: &Room, ids: &[u16]) -> Stats {
    assert!(!psx_rt::interrupts::gp1_queue_pending());
    unsafe {
        let cache = &mut *(&raw mut CACHE);
        // An early back pass (frame::render) may still be on the GPU DMA
        // channel; direct GP0 uploads must not interleave with it.
        let mut idle = false;
        let stats = cache
            .prepare(ids, |key, slot| {
                if !idle {
                    crate::render::wait_dma_idle();
                    idle = true;
                }
                // Four sets of art are in no room's atlas, because each one
                // can be wanted in any view: their keys sit above every texture
                // table, in the order they were added, and stream from linked
                // RAM. The props (Goams, stalactites, grub jars) were added
                // last and are the highest.
                if key >= crate::props::KEY_BASE {
                    // Goams, stalactites and grub jars, above the board's
                    // icons: the props cooker (host/hk-cook/src/props.rs) cuts every frame to slot-sized parts.
                    let (bytes, width, height) =
                        crate::props::texels((key - crate::props::KEY_BASE) as usize)?;
                    if bytes.len() > 2048 {
                        return None;
                    }
                    crate::texture_upload::upload(dest(slot, width, height), bytes);
                    return Some(bytes.len() as u32);
                }
                if key >= crate::charms::KEY_BASE {
                    let (bytes, width, height) =
                        crate::charms::texels((key - crate::charms::KEY_BASE) as usize)?;
                    crate::texture_upload::upload(dest(slot, width, height), bytes);
                    return Some(bytes.len() as u32);
                }
                if key >= crate::ability_art::KEY_BASE {
                    // The Knight's ability clips sit above the Shade's keys and
                    // stream from linked RAM the same way.
                    let index = (key - crate::ability_art::KEY_BASE) as usize;
                    let (width, height) = crate::ability_art::size(index)?;
                    if crate::ability_art::texel_bytes(index)? > 2048 {
                        return None;
                    }
                    return crate::ability_art::upload_frame(index, dest(slot, width, height));
                }
                if key >= crate::shade::KEY_BASE {
                    let (bytes, width, height) =
                        crate::shade::texels((key - crate::shade::KEY_BASE) as usize)?;
                    if bytes.len() > 2048 {
                        return None;
                    }
                    crate::texture_upload::upload(dest(slot, width, height), bytes);
                    return Some(bytes.len() as u32);
                }
                if key as usize >= room.counts[1] {
                    return None;
                }
                let t = room.texture(key as usize);
                let bytes = room.stream_pixels(t)?;
                if bytes.len() > 2048 {
                    return None;
                }
                crate::texture_upload::upload(dest(slot, t.width, t.height), bytes);
                Some(bytes.len() as u32)
            })
            .expect("animation working set");
        assert!(stats.upload_bytes <= hk_cache::MAX_UPLOAD_BYTES);
        if stats.misses != 0 {
            // SDK command constructor and port access. A cached texel line
            // must not survive a slot's overwrite on physical hardware.
            psx_io::gpu::wait_cmd_ready();
            psx_io::gpu::write_gp0(gp0::CLEAR_CACHE);
        }
        core::ptr::write_volatile(
            &raw mut HK_ANIM_CACHE_HITS,
            HK_ANIM_CACHE_HITS.saturating_add(stats.hits),
        );
        core::ptr::write_volatile(
            &raw mut HK_ANIM_CACHE_MISSES,
            HK_ANIM_CACHE_MISSES.saturating_add(stats.misses),
        );
        core::ptr::write_volatile(
            &raw mut HK_ANIM_UPLOAD_BYTES,
            HK_ANIM_UPLOAD_BYTES.saturating_add(stats.upload_bytes),
        );
        core::ptr::write_volatile(
            &raw mut HK_ANIM_UPLOAD_MAX_FRAME,
            HK_ANIM_UPLOAD_MAX_FRAME.max(stats.upload_bytes),
        );
        stats
    }
}
pub fn slot(id: u16) -> usize {
    unsafe { (&*(&raw const CACHE)).slot(id).expect("unprepared texture") }
}
pub fn submit() {
    unsafe { (&mut *(&raw mut CACHE)).submit().expect("cache submit") };
}
/// The queued display switch is consumed only once GPU raster work is idle.
/// Synchronous OT submission separately guarantees the DMA list is finished.
pub fn complete() {
    assert!(!psx_rt::interrupts::gp1_queue_pending());
    assert!(psx_io::gpu::gpustat().contains(GpuStat::READY_DMA_RECV));
    unsafe { (&mut *(&raw mut CACHE)).complete().expect("cache complete") };
}
