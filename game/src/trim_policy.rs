//! When the scenery's exact trimming (occluder holes, tile runs, alpha scissors,
//! flat cores) is worth its CPU time. Trimming saves GPU pixels and spends CPU
//! cycles, so a CPU-bound view is faster without it and a GPU-bound one with it.
//! A frame past two vblanks tells which one it is by its GPU tail: how many
//! scanlines the flip trailed the CPU's last kick.

/// A tail under this many lines means the CPU was the limit (the last chunk's own
/// drawing is about 15 to 23 lines).
pub const OFF_TAIL: u32 = 60;
/// A tail of at least this many lines means the GPU was the limit.
pub const ON_TAIL: u32 = 140;
/// Slow, short-tailed frames among the last `WINDOW` presented frames that turn trimming off. One such frame is
/// no verdict: a CD service or an SPU refill landing in a frame makes one, and
/// so do the cold caches of the first frames of a view. Switching off on it
/// left a GPU-bound view untrimmed for good, since an untrimmed GPU-bound
/// frame's own tail seldom reaches `ON_TAIL` (secret-c21: 84 late frames of 216
/// on one build and 51 on another, by when a single frame happened to run
/// long).
pub const OFF_FRAMES: u32 = 3;
/// How many presented frames the count looks back over. It used to be "in a row", with every frame inside the budget
/// restarting it, which is bistable at the two-vblank edge: a CPU-bound view whose frames alternate between two and three
/// vblanks never gets three slow ones together, keeps trimming, and the trimming's CPU keeps it on the edge (Crossroads_37
/// standing: 836 frames of two vblanks with trimming mostly off, 572 with it on, on builds that differ by 568 bytes of
/// code). A window sees the three slow frames of such a spell, and a long-tailed (GPU-bound) slow frame or a loading gap
/// still clears it.
pub const WINDOW: u32 = 12;
/// The frame lengths in vblanks that count as slow: past the 30 fps bar, and not
/// a loading gap.
const SLOW_VBLANKS: core::ops::RangeInclusive<u32> = 3..=8;

/// What a presented frame says about trimming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    TrimOff,
    TrimOn,
}

/// The verdict of one presented frame that took `vblanks` with a GPU `tail`, while
/// trimming was `trimming`. `history` has one bit per recent presented frame (the newest in bit 0), set for a slow
/// short-tailed one; it belongs to the caller, and restarts at zero whenever the view is bound.
pub fn verdict(trimming: bool, history: &mut u16, vblanks: u32, tail: u32) -> Verdict {
    if vblanks > *SLOW_VBLANKS.end() {
        *history = 0;
        return Verdict::Keep;
    }
    if !SLOW_VBLANKS.contains(&vblanks) {
        // Inside the budget: it is no evidence either way, and it ages the window.
        *history = (*history << 1) & ((1 << WINDOW) - 1);
        return Verdict::Keep;
    }
    if !trimming {
        *history = 0;
        return if tail >= ON_TAIL {
            Verdict::TrimOn
        } else {
            Verdict::Keep
        };
    }
    if tail >= OFF_TAIL {
        *history = 0;
        return Verdict::Keep;
    }
    *history = ((*history << 1) | 1) & ((1 << WINDOW) - 1);
    if history.count_ones() < OFF_FRAMES {
        return Verdict::Keep;
    }
    *history = 0;
    Verdict::TrimOff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(trimming: bool, frames: &[(u32, u32)]) -> (bool, Vec<Verdict>) {
        let (mut on, mut history, mut out) = (trimming, 0u16, Vec::new());
        for &(vblanks, tail) in frames {
            let v = verdict(on, &mut history, vblanks, tail);
            match v {
                Verdict::TrimOff => on = false,
                Verdict::TrimOn => on = true,
                Verdict::Keep => {}
            }
            out.push(v);
        }
        (on, out)
    }

    #[test]
    fn two_slow_short_tailed_frames_do_not_switch_trimming_off() {
        let (on, _) = run(true, &[(3, 20), (2, 10), (3, 80), (2, 10)]);
        assert!(on);
    }

    #[test]
    fn a_long_tail_or_a_loading_gap_restarts_the_count() {
        let (on, _) = run(true, &[(3, 20), (3, 20), (3, 70), (3, 20), (3, 20)]);
        assert!(on);
        let (on, _) = run(true, &[(3, 20), (3, 20), (9, 0), (3, 20)]);
        assert!(on);
    }

    #[test]
    fn three_slow_short_tailed_frames_in_a_row_switch_it_off() {
        let (on, verdicts) = run(true, &[(3, 20), (4, 30), (3, 59)]);
        assert!(!on);
        assert_eq!(verdicts[2], Verdict::TrimOff);
        assert_eq!(&verdicts[..2], &[Verdict::Keep, Verdict::Keep]);
    }

    #[test]
    fn slow_frames_between_frames_inside_the_budget_still_count() {
        // The two-vblank edge: slow and in-budget frames alternate and the old count never reached three.
        let (on, _) = run(true, &[(3, 20), (2, 5), (3, 20), (2, 5), (3, 20)]);
        assert!(!on);
    }

    #[test]
    fn slow_frames_far_apart_do_not_add_up() {
        let mut frames = vec![(3, 20)];
        for _ in 0..WINDOW {
            frames.push((2, 5));
        }
        frames.push((3, 20));
        for _ in 0..WINDOW {
            frames.push((2, 5));
        }
        frames.push((3, 20));
        let (on, _) = run(true, &frames);
        assert!(on);
    }

    #[test]
    fn a_long_tail_switches_it_back_on_and_the_band_between_holds() {
        let (on, v) = run(false, &[(3, 60), (3, 139), (2, 300), (9, 300)]);
        assert!(!on && v.iter().all(|&v| v == Verdict::Keep));
        let (on, v) = run(false, &[(3, 140)]);
        assert!(on && v[0] == Verdict::TrimOn);
    }

    #[test]
    fn loading_gaps_are_not_slow_frames() {
        let (on, _) = run(true, &[(9, 0), (423, 0), (9, 0), (9, 0)]);
        assert!(on);
        let mut history = 3;
        assert_eq!(verdict(true, &mut history, 20, 0), Verdict::Keep);
        assert_eq!(history, 0);
    }
}
