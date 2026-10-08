//! Animated tk2d scenery: torches, waterfalls, idle lift chains, glow bugs.
//!
//! host/cook.py cooks every unique frame of such an object's playing clip as a
//! scenery draw at the object's authored transform, so it sorts with the
//! scenery around it at its own depth; this hides all but the frame the clip
//! is on. Unity starts every auto-playing animator on the scene's first frame,
//! so one scene clock drives them all and objects sharing a clip stay in step,
//! as they do in the original.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/decor.rs"));

/// Decor groups shown, summed over drawn frames.
#[no_mangle] pub static mut HK_DECOR_GROUPS: u32 = 0;

/// Show one frame of every decor group of `view`, `ticks` (60 Hz) after its
/// scene started. Runs after the view's visibility reset, beside props::apply.
#[inline(never)]
pub fn apply(view: usize, ticks: u32) {
    let Ok(at) = VIEWS.binary_search_by_key(&(view as u16), |&(slot, _, _)| slot) else { return };
    let (_, first, count) = VIEWS[at];
    for &(draw, draws, fps, wrap, loop_start, step, steps) in &GROUPS[first as usize..(first + count) as usize] {
        let shown = STEPS[step as usize + frame(ticks, fps, wrap, loop_start, steps)] as usize;
        for k in 0..draws as usize {
            if k != shown { crate::render::set_visible(draw as usize + k, false); }
        }
    }
    unsafe { HK_DECOR_GROUPS = HK_DECOR_GROUPS.wrapping_add(count as u32) }
}

/// The clip step at an age, as tk2d plays loop (0), loop section (1) and once
/// (2) clips. Ages are clamped well past any clip so the multiply stays 32-bit.
fn frame(ticks: u32, fps_x256: u16, wrap: u8, loop_start: u8, steps: u8) -> usize {
    let steps = steps as u32;
    let time = (ticks % (1 << 20)) * fps_x256 as u32 / (60 * 256);
    let loop_start = loop_start as u32;
    let index = match wrap {
        0 => time % steps,
        1 if time >= steps && loop_start < steps => loop_start + (time - loop_start) % (steps - loop_start),
        _ => time.min(steps - 1),
    };
    index as usize
}
