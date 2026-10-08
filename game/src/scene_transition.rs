//! Scene-gate ownership handoff. Spatial region changes never enter this path.
//! All outgoing GPU work and Room borrows end before the arena is reclaimed.
#[no_mangle]pub static mut HK_SCENE_GATE_LOADS:u32=0;
#[no_mangle]pub static mut HK_SCENE_GATE_TICKS:u32=0;
#[inline(never)]
/// `shown` and `other` are the framebuffer rows of the frame on screen and of
/// the other buffer: an exit fade still running finishes from them.
pub fn load(cache:&mut crate::disc::Cache,vram:&mut crate::vram_cache::Cache,
            region:usize,clock:u32,shown:u16,other:u16)->Result<u32,crate::disc::LoadError> {
    assert!(!crate::render::dma_pending()&&!psx_rt::interrupts::gp1_queue_pending());
    crate::gate_probe::load_start(0);
    crate::gate_probe::set(crate::gate_probe::PREP);
    crate::input::begin_scene_load(clock);
    // The load owns the drive; area music plays on from its FIFO.
    crate::disc::begin_load();crate::music::begin_gate(crate::world::scene_of(region));
    // An exit fade in progress keeps the last frame on screen and finishes
    // from it while the load runs (it needs no texture or scene byte);
    // otherwise queue the blackout at VBlank before touching either.
    let fading=crate::exit_fade::armed();
    if !fading {
        // psx-rt applies a queued word only once GPUSTAT bit 24 is up: raise
        // it (the GPU has nothing left to draw here) before queueing.
        psx_gpu::arm_draw_done();psx_gpu::signal_draw_done();
        psx_rt::interrupts::queue_gp1_at_vblank(psx_hw::gpu::gp1::display_enable(false));
        while psx_rt::interrupts::gp1_queue_pending() {crate::input::checkpoint();}
        crate::gate_probe::note(31,psx_rt::interrupts::vblank_count());
    }
    psx_gpu::draw_sync();crate::render::release_scene();vram.release();
    if fading {crate::exit_fade::start(shown,other);}
    else {psx_gpu::fill_rect(0,0,320,480,0,0,0);psx_gpu::draw_sync();}
    let result=cache.select(region);
    // Nothing of the new scene is shown before the fade has reached black.
    while crate::exit_fade::active() {crate::input::checkpoint();}
    // On failure retain the black screen and revoked readiness. No old texture
    // or scene may be presented after partial replacement; the caller reports it.
    crate::gate_probe::set(crate::gate_probe::DISPLAY);
    if result.is_ok() {
        vram.admit_scene(region,cache.atlases_ready());
        psx_gpu::arm_draw_done();psx_gpu::signal_draw_done();
        psx_rt::interrupts::queue_gp1_at_vblank(psx_hw::gpu::gp1::display_enable(true));
        while psx_rt::interrupts::gp1_queue_pending() {crate::input::checkpoint();}
    }
    crate::disc::end_load();
    let now=crate::input::end_scene_load();
    crate::gate_probe::display_on();
    unsafe {HK_SCENE_GATE_LOADS=HK_SCENE_GATE_LOADS.saturating_add(1);
        HK_SCENE_GATE_TICKS=HK_SCENE_GATE_TICKS.saturating_add(now.wrapping_sub(clock));}
    result.map(|_|now)
}
