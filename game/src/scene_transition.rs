//! Scene-gate ownership handoff. Spatial region changes never enter this path.
//! All outgoing GPU work and Room borrows end before the arena is reclaimed.
#[no_mangle]pub static mut HK_SCENE_GATE_LOADS:u32=0;
#[no_mangle]pub static mut HK_SCENE_GATE_TICKS:u32=0;
#[inline(never)]
pub fn load(cache:&mut crate::disc::Cache,vram:&mut crate::vram_cache::Cache,
            region:usize,clock:u32)->Result<u32,crate::disc::LoadError> {
    assert!(!crate::render::dma_pending()&&!psx_rt::interrupts::gp1_queue_pending());
    crate::input::begin_scene_load(clock);
    // Queue the blackout at VBlank before touching either texture or scene bank.
    psx_rt::interrupts::queue_gp1_at_vblank(psx_hw::gpu::gp1::display_enable(false));
    while psx_rt::interrupts::gp1_queue_pending() {crate::input::checkpoint();}
    psx_gpu::draw_sync();crate::render::release_scene();vram.release();
    psx_gpu::fill_rect(0,0,320,480,0,0,0);psx_gpu::draw_sync();
    let result=cache.select(region);
    // On failure retain the black screen and revoked readiness. No old texture
    // or scene may be presented after partial replacement; the caller reports it.
    if result.is_ok() {
        vram.admit_scene(region,cache.atlases_ready());
        psx_rt::interrupts::queue_gp1_at_vblank(psx_hw::gpu::gp1::display_enable(true));
        while psx_rt::interrupts::gp1_queue_pending() {crate::input::checkpoint();}
    }
    let now=crate::input::end_scene_load();
    unsafe {HK_SCENE_GATE_LOADS=HK_SCENE_GATE_LOADS.saturating_add(1);
        HK_SCENE_GATE_TICKS=HK_SCENE_GATE_TICKS.saturating_add(now.wrapping_sub(clock));}
    result.map(|_|now)
}
