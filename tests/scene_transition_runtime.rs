//! Run the production scene-gate orchestrator against bounded fake devices.
//! Device preconditions fail at the offending operation, independently of the
//! expected trace. Scene decoding itself is covered by the format/cache tests.
extern crate self as psx_rt;
extern crate self as psx_hw;
extern crate self as psx_gpu;

use std::sync::Mutex;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Begin(u32), Queue(bool), VBlank(u32), Applied(bool), Sync,
    ReleaseRender, ReleaseVram, Clear, Reclaim(usize), Upload,
    Admit(usize), End(u32),
}
struct Hardware {
    events: Vec<Event>, tick: u32, queued: Option<(bool, u8)>, display: bool,
    dma: bool, render_owned: bool, input_owned: bool, framebuffer_clear: bool,
    arena_ready: bool, atlases_ready: bool, vram_ready: bool,
}
impl Hardware {
    fn new(tick: u32) -> Self {
        Self {events: Vec::with_capacity(64), tick, queued: None, display: true,
            dma: false, render_owned: true, input_owned: false,
            framebuffer_clear: false, arena_ready: true, atlases_ready: true,
            vram_ready: true}
    }
    fn record(&mut self, event: Event) {
        assert!(self.events.len() < 64, "unexpected unbounded device traffic");
        self.events.push(event);
    }
}
static HW: Mutex<Option<Hardware>> = Mutex::new(None);
fn device<T>(f: impl FnOnce(&mut Hardware) -> T) -> T {
    f(HW.lock().unwrap().as_mut().unwrap())
}
fn reset(tick: u32) { *HW.lock().unwrap() = Some(Hardware::new(tick)); }

pub mod gpu { pub mod gp1 {
    pub fn display_enable(enabled: bool) -> u32 { 0x03000000 | u32::from(!enabled) }
}}
pub mod interrupts {
    pub fn gp1_queue_pending() -> bool { super::device(|h| h.queued.is_some()) }
    pub fn queue_gp1_at_vblank(command: u32) {
        assert!(command == 0x03000000 || command == 0x03000001);
        let enabled = command == 0x03000000;
        super::device(|h| {
            assert!(h.input_owned && h.queued.is_none());
            if enabled {
                assert!(!h.display && h.framebuffer_clear);
                assert!(h.arena_ready && h.atlases_ready && h.vram_ready);
                assert!(!h.render_owned && !h.dma);
            }
            h.queued = Some((enabled, 2)); // Completion requires two checkpoints.
            h.record(super::Event::Queue(enabled));
        });
    }
}
pub fn draw_sync() {
    device(|h| { assert!(!h.display && h.queued.is_none()); h.dma = false; h.record(Event::Sync); });
}
pub fn fill_rect(x: u16, y: u16, w: u16, h: u16, r: u8, g: u8, b: u8) {
    assert_eq!((x,y,w,h,r,g,b), (0,0,320,480,0,0,0));
    device(|h| {
        assert!(!h.display && h.queued.is_none() && !h.dma);
        assert!(!h.render_owned && !h.vram_ready);
        h.framebuffer_clear = true; h.dma = true; h.record(Event::Clear);
    });
}
mod render {
    pub fn dma_pending() -> bool { super::device(|h| h.dma) }
    pub fn release_scene() {
        super::device(|h| {
            assert!(!h.display && h.queued.is_none() && !h.dma && h.render_owned);
            h.render_owned = false; h.record(super::Event::ReleaseRender);
        });
    }
}
mod input {
    pub fn begin_scene_load(clock: u32) {
        super::device(|h| {
            assert!(!h.input_owned); assert_eq!(h.tick, clock);
            h.input_owned = true; h.record(super::Event::Begin(clock));
        });
    }
    pub fn checkpoint() {
        super::device(|h| {
            assert!(h.input_owned);
            h.tick = h.tick.wrapping_add(1); h.record(super::Event::VBlank(h.tick));
            if let Some((enabled, remaining)) = h.queued {
                if remaining > 1 { h.queued = Some((enabled, remaining - 1)); }
                else {
                    h.queued = None; h.display = enabled;
                    h.record(super::Event::Applied(enabled));
                }
            }
        });
    }
    pub fn end_scene_load() -> u32 {
        super::device(|h| {
            assert!(h.input_owned && h.queued.is_none() && !h.dma);
            h.input_owned = false; h.record(super::Event::End(h.tick)); h.tick
        })
    }
}
mod disc {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum LoadError { Read, Upload }
    pub struct Cache { pub error: Option<LoadError> }
    impl Cache {
        pub fn select(&mut self, region: usize) -> Result<(), LoadError> {
            super::device(|h| {
                assert!(!h.display && h.queued.is_none() && !h.dma && h.input_owned);
                assert!(!h.render_owned && !h.vram_ready && h.framebuffer_clear);
                h.arena_ready = false; h.atlases_ready = false;
                h.record(super::Event::Reclaim(region));
            });
            // Emulate cooperative CD/decode service beyond FIFO capacity. The
            // real sampler's separate lifecycle tests verify sample accounting.
            for _ in 0..24 { super::input::checkpoint(); }
            if self.error == Some(LoadError::Read) { return Err(LoadError::Read); }
            super::device(|h| {
                assert!(!h.display && !h.render_owned && !h.vram_ready);
                h.record(super::Event::Upload);
            });
            if self.error == Some(LoadError::Upload) { return Err(LoadError::Upload); }
            super::device(|h| {h.arena_ready = true; h.atlases_ready = true;});
            Ok(())
        }
        pub fn atlases_ready(&self) -> bool { super::device(|h| h.atlases_ready) }
    }
}
mod vram_cache {
    pub struct Cache;
    impl Cache {
        pub fn release(&mut self) {
            super::device(|h| {
                assert!(!h.display && h.queued.is_none() && !h.render_owned && !h.dma);
                h.vram_ready = false; h.record(super::Event::ReleaseVram);
            });
        }
        pub fn admit_scene(&mut self, region: usize, uploaded: bool) {
            super::device(|h| {
                assert!(!h.display && h.arena_ready && h.atlases_ready && uploaded);
                assert!(!h.render_owned && !h.vram_ready && h.input_owned);
                h.vram_ready = true; h.record(super::Event::Admit(region));
            });
        }
    }
}
#[path = "../game/src/scene_transition.rs"] mod scene_transition;

#[test]
fn production_scene_handoff_waits_for_blackout_and_keeps_failed_replacement_revoked() {
    let mut vram = vram_cache::Cache;
    reset(100);
    assert_eq!(scene_transition::load(&mut disc::Cache {error: None}, &mut vram, 7, 100), Ok(128));
    device(|h| {
        assert_eq!(&h.events[..10], &[
            Event::Begin(100), Event::Queue(false), Event::VBlank(101),
            Event::VBlank(102), Event::Applied(false), Event::Sync,
            Event::ReleaseRender, Event::ReleaseVram, Event::Clear, Event::Sync,
        ]);
        assert_eq!(h.events[10], Event::Reclaim(7));
        assert_eq!(&h.events[h.events.len()-7..], &[
            Event::Upload, Event::Admit(7), Event::Queue(true),
            Event::VBlank(127), Event::VBlank(128), Event::Applied(true), Event::End(128),
        ]);
        assert!(h.display && h.arena_ready && h.atlases_ready && h.vram_ready);
        assert!(!h.input_owned && !h.render_owned && !h.dma && h.queued.is_none());
    });
    unsafe {
        assert_eq!(scene_transition::HK_SCENE_GATE_LOADS, 1);
        assert_eq!(scene_transition::HK_SCENE_GATE_TICKS, 28);
    }
    for error in [disc::LoadError::Read, disc::LoadError::Upload] {
        reset(u32::MAX - 10);
        assert_eq!(scene_transition::load(&mut disc::Cache {error: Some(error)}, &mut vram, 3, u32::MAX - 10), Err(error));
        device(|h| {
            assert!(!h.display && !h.vram_ready && !h.arena_ready && !h.atlases_ready);
            assert!(!h.input_owned && !h.render_owned && !h.dma && h.queued.is_none());
            assert!(!h.events.iter().any(|event| matches!(event, Event::Queue(true) | Event::Admit(_))));
            assert_eq!(h.events.last(), Some(&Event::End(15)));
        });
    }
    unsafe {
        assert_eq!(scene_transition::HK_SCENE_GATE_LOADS, 3);
        assert_eq!(scene_transition::HK_SCENE_GATE_TICKS, 80);
    }
    for dma in [true, false] {
        reset(200);
        device(|h| {if dma {h.dma = true;} else {h.queued = Some((true, 1));}});
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scene_transition::load(&mut disc::Cache {error: None}, &mut vram, 1, 200)
        }));
        assert!(rejected.is_err());
        device(|h| {
            assert!(h.events.is_empty() && h.display && h.render_owned);
            assert!(!h.input_owned && h.vram_ready && h.arena_ready && h.atlases_ready);
        });
    }
    unsafe { assert_eq!(scene_transition::HK_SCENE_GATE_LOADS, 3); }
}
