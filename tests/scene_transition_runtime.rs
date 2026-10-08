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
    Admit(usize), End(u32), FadeStart(u16, u16), FadeBlack,
}
struct Hardware {
    events: Vec<Event>, tick: u32, queued: Option<(bool, u8)>, display: bool,
    dma: bool, render_owned: bool, input_owned: bool, framebuffer_clear: bool,
    arena_ready: bool, atlases_ready: bool, vram_ready: bool,
    // Exit fade: armed by the frame side, then run from load checkpoints.
    // While it is armed or running the screen shows a frozen copy of the
    // last frame (no scene or texture byte); once black it shows nothing.
    fade_armed: bool, fade_left: u32, fade_black: bool,
    // GPUSTAT bit 24: psx-rt applies a queued word only once a GP0(1Fh)
    // after the last arm has raised it.
    armed: bool, signalled: bool,
}
impl Hardware {
    fn new(tick: u32) -> Self {
        Self {events: Vec::with_capacity(64), tick, queued: None, display: true,
            dma: false, render_owned: true, input_owned: false,
            framebuffer_clear: false, arena_ready: true, atlases_ready: true,
            vram_ready: true, fade_armed: false, fade_left: 0, fade_black: false,
            armed: false, signalled: false}
    }
    /// Nothing on screen depends on the scene arena, the atlases or VRAM.
    fn scene_hidden(&self) -> bool { !self.display || self.fade_armed || self.fade_left > 0 || self.fade_black }
    fn record(&mut self, event: Event) {
        assert!(self.events.len() < 64, "unexpected unbounded device traffic");
        self.events.push(event);
    }
}
static HW: Mutex<Option<Hardware>> = Mutex::new(None);
// The fake devices are one global: tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());
fn serial() -> std::sync::MutexGuard<'static, ()> { SERIAL.lock().unwrap_or_else(|e| e.into_inner()) }
fn device<T>(f: impl FnOnce(&mut Hardware) -> T) -> T {
    f(HW.lock().unwrap().as_mut().unwrap())
}
fn reset(tick: u32) { *HW.lock().unwrap() = Some(Hardware::new(tick)); }

pub mod gpu { pub mod gp1 {
    pub fn display_enable(enabled: bool) -> u32 { 0x03000000 | u32::from(!enabled) }
}}
pub mod interrupts {
    pub fn vblank_count() -> u32 { super::device(|h| h.tick) }
    pub fn gp1_queue_pending() -> bool { super::device(|h| h.queued.is_some()) }
    pub fn queue_gp1_at_vblank(command: u32) {
        assert!(command == 0x03000000 || command == 0x03000001);
        let enabled = command == 0x03000000;
        super::device(|h| {
            assert!(h.signalled, "queued without a GP0(1Fh) after the arm: psx-rt never applies it");
            h.signalled = false;
            assert!(h.input_owned && h.queued.is_none());
            if enabled {
                assert!((!h.display || h.fade_black) && h.framebuffer_clear);
                assert!(h.arena_ready && h.atlases_ready && h.vram_ready);
                assert!(!h.render_owned && !h.dma);
            }
            h.queued = Some((enabled, 2)); // Completion requires two checkpoints.
            h.record(super::Event::Queue(enabled));
        });
    }
}
pub fn arm_draw_done() { device(|h| { h.armed = true; h.signalled = false; }); }
pub fn signal_draw_done() { device(|h| { assert!(h.armed, "GP0(1Fh) without an arm"); h.armed = false; h.signalled = true; }); }
pub fn draw_sync() {
    device(|h| { assert!(h.scene_hidden() && h.queued.is_none()); h.dma = false; h.record(Event::Sync); });
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
            assert!(h.scene_hidden() && h.queued.is_none() && !h.dma && h.render_owned);
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
            if h.fade_left > 0 {
                h.fade_left -= 1;
                if h.fade_left == 0 {
                    h.fade_black = true; h.framebuffer_clear = true;
                    h.record(super::Event::FadeBlack);
                }
            }
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
mod music { pub fn begin_gate(_scene: usize) {} }
mod gate_probe {
    pub const PREP: u8 = 0; pub const DISPLAY: u8 = 0;
    pub fn load_start(_: u32) {} pub fn set(_: u8) {} pub fn note(_: usize, _: u32) {}
    pub fn display_on() {}
}
mod exit_fade {
    pub fn armed() -> bool { super::device(|h| h.fade_armed) }
    pub fn start(shown: u16, other: u16) {
        super::device(|h| {
            assert!(h.fade_armed && h.input_owned && !h.dma && !h.render_owned && !h.vram_ready);
            h.fade_armed = false; h.fade_left = 14; h.record(super::Event::FadeStart(shown, other));
        });
    }
    pub fn active() -> bool { super::device(|h| h.fade_left > 0) }
}
mod world { pub fn scene_of(region: usize) -> usize { region } }
mod disc {
    // The load owns the drive for its whole length, inside the input handoff.
    pub fn begin_load() { super::device(|h| assert!(h.input_owned)); }
    pub fn end_load() { super::device(|h| assert!(h.input_owned)); }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum LoadError { Read, Upload }
    pub struct Cache { pub error: Option<LoadError> }
    impl Cache {
        pub fn select(&mut self, region: usize) -> Result<(), LoadError> {
            super::device(|h| {
                assert!(h.scene_hidden() && h.queued.is_none() && !h.dma && h.input_owned);
                assert!(!h.render_owned && !h.vram_ready && (h.framebuffer_clear || h.fade_left > 0));
                h.arena_ready = false; h.atlases_ready = false;
                h.record(super::Event::Reclaim(region));
            });
            // Emulate cooperative CD/decode service beyond FIFO capacity. The
            // real sampler's separate lifecycle tests verify sample accounting.
            for _ in 0..24 { super::input::checkpoint(); }
            if self.error == Some(LoadError::Read) { return Err(LoadError::Read); }
            super::device(|h| {
                assert!(h.scene_hidden() && !h.render_owned && !h.vram_ready);
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
                assert!(h.scene_hidden() && h.queued.is_none() && !h.render_owned && !h.dma);
                h.vram_ready = false; h.record(super::Event::ReleaseVram);
            });
        }
        pub fn admit_scene(&mut self, region: usize, uploaded: bool) {
            super::device(|h| {
                assert!((!h.display || h.fade_black) && h.fade_left == 0);
                assert!(h.arena_ready && h.atlases_ready && uploaded);
                assert!(!h.render_owned && !h.vram_ready && h.input_owned);
                h.vram_ready = true; h.record(super::Event::Admit(region));
            });
        }
    }
}
#[path = "../game/src/scene_transition.rs"] mod scene_transition;

#[test]
fn production_scene_handoff_waits_for_blackout_and_keeps_failed_replacement_revoked() {
    let _serial = serial();
    let (loads, ticks) = unsafe { (scene_transition::HK_SCENE_GATE_LOADS, scene_transition::HK_SCENE_GATE_TICKS) };
    let mut vram = vram_cache::Cache;
    reset(100);
    assert_eq!(scene_transition::load(&mut disc::Cache {error: None}, &mut vram, 7, 100, 0, 256), Ok(128));
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
        assert_eq!(scene_transition::HK_SCENE_GATE_LOADS, loads + 1);
        assert_eq!(scene_transition::HK_SCENE_GATE_TICKS, ticks + 28);
    }
    for error in [disc::LoadError::Read, disc::LoadError::Upload] {
        reset(u32::MAX - 10);
        assert_eq!(scene_transition::load(&mut disc::Cache {error: Some(error)}, &mut vram, 3, u32::MAX - 10, 0, 256), Err(error));
        device(|h| {
            assert!(!h.display && !h.vram_ready && !h.arena_ready && !h.atlases_ready);
            assert!(!h.input_owned && !h.render_owned && !h.dma && h.queued.is_none());
            assert!(!h.events.iter().any(|event| matches!(event, Event::Queue(true) | Event::Admit(_))));
            assert_eq!(h.events.last(), Some(&Event::End(15)));
        });
    }
    unsafe {
        assert_eq!(scene_transition::HK_SCENE_GATE_LOADS, loads + 3);
        assert_eq!(scene_transition::HK_SCENE_GATE_TICKS, ticks + 80);
    }
    for dma in [true, false] {
        reset(200);
        device(|h| {if dma {h.dma = true;} else {h.queued = Some((true, 1));}});
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scene_transition::load(&mut disc::Cache {error: None}, &mut vram, 1, 200, 0, 256)
        }));
        assert!(rejected.is_err());
        device(|h| {
            assert!(h.events.is_empty() && h.display && h.render_owned);
            assert!(!h.input_owned && h.vram_ready && h.arena_ready && h.atlases_ready);
        });
    }
    unsafe { assert_eq!(scene_transition::HK_SCENE_GATE_LOADS, loads + 3); }
}

#[test]
fn an_exit_fade_finishes_from_the_last_frame_before_the_new_scene_is_shown() {
    let _serial = serial();
    let mut vram = vram_cache::Cache;
    reset(300);
    device(|h| h.fade_armed = true);
    let now = scene_transition::load(&mut disc::Cache {error: None}, &mut vram, 9, 300, 256, 0).unwrap();
    device(|h| {
        // No blackout is queued: the fade keeps the frozen frame on screen and
        // the load starts at once, with no framebuffer clear of its own.
        assert_eq!(&h.events[..4], &[Event::Begin(300), Event::Sync, Event::ReleaseRender, Event::ReleaseVram]);
        assert_eq!(h.events[4], Event::FadeStart(256, 0));
        assert_eq!(h.events[5], Event::Reclaim(9));
        assert!(!h.events.iter().any(|e| matches!(e, Event::Queue(false) | Event::Clear)));
        // The scene is admitted only after the fade has reached black.
        let black = h.events.iter().position(|e| *e == Event::FadeBlack).unwrap();
        let admit = h.events.iter().position(|e| *e == Event::Admit(9)).unwrap();
        assert!(black < admit);
        assert_eq!(h.events.last(), Some(&Event::End(now)));
        assert!(h.display && h.vram_ready && !h.input_owned && h.queued.is_none());
    });
}
