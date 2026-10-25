//! Static atlases are uploaded before the corresponding scene is admitted.
//! Exclusive scene residency replaces atlases only behind a drained gate blackout.
use hk_cache::residency;
use psx_vram::VramRect;
pub struct Cache {
    ready: bool,
    scene: usize,
}
#[no_mangle]
pub static mut HK_VRAM_UPLOAD_CANCELLATIONS: u32 = 0;
#[no_mangle]
pub static mut HK_VRAM_UPLOAD_BYTES: u32 = 0;
#[no_mangle]
pub static mut HK_VRAM_UPLOAD_MAX_FRAME: u32 = 0;
#[no_mangle]
pub static mut HK_VRAM_BANK_HITS: u32 = 0;
#[no_mangle]
pub static mut HK_VRAM_BANK_MISSES: u32 = 0;
#[no_mangle]
pub static mut HK_VRAM_ACTIVE_BANK: u32 = u32::MAX;
#[no_mangle]
pub static mut HK_VRAM_PENDING_REGION: u32 = 0;
impl Cache {
    pub const fn new() -> Self {
        Self {
            ready: false,
            scene: usize::MAX,
        }
    }
    /// The loader has verified the selected residency set before publishing
    /// immutable scene views. No atlas payload is retained in main RAM.
    pub fn admit_scene(&mut self, region: usize, uploaded: bool) {
        assert!(uploaded);
        self.scene = crate::disc::scene_index(region);
        self.ready = true;
    }
    pub fn release(&mut self) {
        self.ready = false;
        self.scene = usize::MAX;
    }
    pub fn ready(&self, region: usize) -> bool {
        self.ready
            && (!crate::disc::SCENE_GATE_LOAD || self.scene == crate::disc::scene_index(region))
    }
    pub fn activate(&mut self, region: usize) -> usize {
        let scene = crate::disc::scene_index(region);
        assert!(self.ready(region));
        unsafe {
            HK_VRAM_ACTIVE_BANK = scene as u32;
            HK_VRAM_BANK_HITS += 1;
        }
        scene
    }
}
/// Called while the loader exclusively owns the scene arena. FIFO upload
/// plus draw_sync completes before the next CD read can reuse these bytes.
/// The generated descriptor inventory is checked for exact, disjoint page and
/// palette coverage within each scene by Cache::init before uploads start.
pub fn upload_atlas(a: &crate::disc::AtlasDesc, bytes: &[u8]) {
    assert_eq!(bytes.len(), a.raw_len);
    assert!(!crate::render::dma_pending() && !psx_rt::interrupts::gp1_queue_pending());
    crate::input::checkpoint();
    match a.kind {
        0 => {
            assert_eq!(bytes.len(), a.count * 32768);
            assert!(a.first + a.count <= residency::STATIC_PAGES);
            for i in 0..a.count {
                let (x, y) = residency::page_xy(a.first + i);
                crate::texture_upload::upload(
                    VramRect::new(x, y, 64, 256),
                    &bytes[i * 32768..(i + 1) * 32768],
                );
                crate::input::checkpoint();
            }
        }
        1 => {
            assert_eq!(bytes.len(), a.count * 32);
            assert!(a.first + a.count <= residency::SCENE_CLUTS);
            for i in 0..a.count {
                let palette = a.first + i;
                let (x, y) =
                    residency::clut_xy(palette / residency::CLUTS, palette % residency::CLUTS);
                crate::texture_upload::upload(
                    VramRect::new(x, y, 16, 1),
                    &bytes[i * 32..(i + 1) * 32],
                );
                if i & 31 == 0 {
                    crate::input::checkpoint();
                }
            }
        }
        _ => panic!("atlas kind"),
    }
    psx_io::gpu::wait_cmd_ready();
    psx_io::gpu::write_gp0(psx_hw::gpu::gp0::CLEAR_CACHE);
    psx_gpu::draw_sync();
    unsafe {
        HK_VRAM_UPLOAD_BYTES = HK_VRAM_UPLOAD_BYTES.saturating_add(bytes.len() as u32);
        HK_VRAM_UPLOAD_MAX_FRAME = HK_VRAM_UPLOAD_MAX_FRAME.max(bytes.len() as u32);
        // One admission per scene, rather than one per independent atlas chunk.
        if a.kind == 0 {
            if a.first == crate::disc::scene_page_base(a.scene_index) {
                HK_VRAM_BANK_MISSES += 1;
            }
        }
    }
    crate::input::checkpoint();
}
/// Spare static pages as extra prefetch room (disc.rs `Spill`): a page holds 16 CD sectors, each
/// sector filling 16 rows of the page's 64-word rows. Write `sectors` whole sectors of `bytes`
/// from sector `first` of static page `page`.
pub fn spill_write(page: usize, first: usize, sectors: usize, bytes: &[u8]) {
    let (x, y) = residency::page_xy(page);
    crate::texture_upload::upload(
        VramRect::new(x, y + (first * 16) as u16, 64, (sectors * 16) as u16),
        bytes,
    );
}
/// Read the sectors `spill_write` stored back into `dst` (a four-byte aligned arena address).
/// False if the GPU stopped answering; `dst` is then partly written.
///
/// # Safety
/// `dst` must be valid for `sectors * 512` words.
pub unsafe fn spill_read(page: usize, first: usize, sectors: usize, dst: *mut u32) -> bool {
    let (x, y) = residency::page_xy(page);
    let y = y + (first * 16) as u16;
    psx_io::gpu::wait_cmd_ready();
    psx_io::gpu::write_gp0(0xC000_0000);
    psx_io::gpu::write_gp0(u32::from(y) << 16 | u32::from(x));
    psx_io::gpu::write_gp0(((sectors * 16) as u32) << 16 | 64);
    for i in 0..sectors * 512 {
        let mut spins = 0u32;
        // GPUSTAT bit 27: ready to send VRAM to the CPU.
        while psx_io::gpu::status().bits() & (1 << 27) == 0 {
            spins += 1;
            if spins > 500_000 {
                return false;
            }
        }
        // SAFETY: the caller sized `dst` for every word.
        unsafe { dst.add(i).write(psx_io::gpu::read_data()) };
    }
    true
}
/// The GPU is ready for a command and for DMA (GPUSTAT bits 26 and 28) and no DMA is running:
/// what it was sent has finished, so a command now does not wait on it.
pub fn gpu_idle() -> bool {
    const READY: u32 = 1 << 26 | 1 << 28;
    !crate::render::dma_pending() && psx_io::gpu::status().bits() & READY == READY
}
