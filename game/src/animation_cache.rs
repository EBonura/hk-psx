//! Eight 64x64 4bpp slots in the gap beside the two framebuffers.
//! Room palettes remain immutable and resident; only texels are replaced.
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
pub fn uv(slot: usize) -> (u16, u16) {
    assert!(slot < hk_cache::SLOTS);
    ((slot as u16 % 2) * 64, (slot as u16 / 2) * 64)
}
/// Called before the frame clear or any submitted drawing commands. The prior
/// frame's idle VBlank acknowledgement must already have called `complete`.
pub fn prepare(room: &Room, ids: &[u16]) -> Stats {
    assert!(!psx_rt::interrupts::gp1_queue_pending());
    unsafe {
        let cache = &mut *(&raw mut CACHE);
        let stats = cache
            .prepare(ids, |key, slot| {
                // The Hollow Shade is not in any room's atlas: its keys sit
                // above every texture table and stream from linked RAM.
                if key >= crate::shade::KEY_BASE {
                    let (bytes, width, height) = crate::shade::texels((key - crate::shade::KEY_BASE) as usize)?;
                    if bytes.len() > 2048 {
                        return None;
                    }
                    let (u, v) = uv(slot);
                    crate::texture_upload::upload(
                        VramRect::new(320 + u / 4, v, (width + 3) / 4, height),
                        bytes,
                    );
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
                let (u, v) = uv(slot);
                crate::texture_upload::upload(
                    VramRect::new(320 + u / 4, v, (t.width + 3) / 4, t.height),
                    bytes,
                );
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
