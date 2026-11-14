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
/// Slow, short-tailed frames among the last `WINDOW` presented frames that turn trimming off.
///
/// Derived from kings-death, cornifer-map, baldur-spell, grub-jar, secret-c04, secret-c13, secret-c21 and c37-stand
/// (docs/REPIN-LOG.md, P2): per route, mean frame time with trimming always on, always off and under the live policy,
/// at three tape phases, and the share of presented frames that were slow and short-tailed with trimming on.
/// Trimming off paid on c37-stand (240 against 286 on main, 232 against 281 on the port) and on secret-c04 (-1.5%);
/// it cost 4% on kings-death, 6 to 7% on cornifer-map, 7% on baldur-spell, 11% on grub-jar, 16% on secret-c13 and
/// 2.5% on secret-c21. A single slow short-tailed frame, or three in twelve, is no evidence: kings-death has 8% of
/// them, cornifer-map 12%, secret-c21 18%, c37-stand 35%. In a window of 32 frames, 10 of them were slow and short-tailed
/// in 52% of c37-stand's windows and in at most 22% of any other route's (secret-c21), 10% of kings-death's and 7%
/// of cornifer-map's. The old rule (three in twelve, and a long-tailed slow frame clearing the count) turned trimming off
/// 1.7 to 3.9 times in 100 frames on every one of these routes with it held on, and left the port at the OFF level on
/// kings-death and cornifer-map at some tape phases.
pub const OFF_FRAMES: u32 = 10;
/// Slow frames (any tail) among the last `WINDOW` frames, with trimming off, that turn it back on: half the window.
/// Trimming off is right only where it takes the slow frames away. The share of presented frames past two vblanks,
/// with trimming on then off: c37-stand 0.67 then 0.30 and secret-c04 0.14 then 0.09 (off helps); kings-death 0.18
/// then 0.73, cornifer-map 0.80 then 0.97, baldur-spell 0.84 then 0.98, grub-jar 0.63 then 0.93, secret-c13 0.48 then
/// 0.95, secret-c21 0.94 then 0.94 (off hurts). Half the window sits between 0.30 and 0.73: a wrong off is undone after
/// about 16 slow frames (under a second at 30 fps); a right one is undone by chance in about 1% of windows. Before this
/// an off verdict stood until one slow frame had a tail of `ON_TAIL`, and cornifer-map stayed off for 300 frames at
/// the full cost of off (+6%).
pub const ON_FRAMES: u32 = 16;
/// How many presented frames the count looks back over. A long-tailed slow frame no longer clears the count (it
/// ages it like any other frame): clearing made the verdict depend on whichever frame came last, the bistability at the
/// two-vblank edge that `trim_policy`'s earlier windows only moved (Crossroads_37 standing, builds 568 bytes apart).
pub const WINDOW: u32 = 32;
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
pub fn verdict(trimming: bool, history: &mut u32, vblanks: u32, tail: u32) -> Verdict {
    if vblanks > *SLOW_VBLANKS.end() {
        *history = 0;
        return Verdict::Keep;
    }
    if !SLOW_VBLANKS.contains(&vblanks) {
        // Inside the budget: it is no evidence either way, and it ages the window.
        *history <<= 1;
        return Verdict::Keep;
    }
    if !trimming {
        // Off, every slow frame is a bit, whatever its tail: trimming off is only right where it takes the slow
        // frames away (see `ON_FRAMES`).
        *history = (*history << 1) | 1;
        if tail >= ON_TAIL || history.count_ones() >= ON_FRAMES {
            *history = 0;
            return Verdict::TrimOn;
        }
        return Verdict::Keep;
    }
    if tail >= OFF_TAIL {
        *history <<= 1;
        return Verdict::Keep;
    }
    *history = (*history << 1) | 1;
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
        let (mut on, mut history, mut out) = (trimming, 0u32, Vec::new());
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
    fn fewer_than_the_count_in_the_window_do_not_switch_trimming_off() {
        let mut frames = Vec::new();
        for _ in 0..OFF_FRAMES - 1 {
            frames.push((3, 20));
        }
        // The rest of the window: in budget, and a long-tailed slow frame that ages the count.
        for _ in 0..WINDOW - OFF_FRAMES + 1 {
            frames.push((2, 5));
        }
        frames.push((3, 300));
        let (on, _) = run(true, &frames);
        assert!(on);
    }

    #[test]
    fn the_count_of_slow_short_tailed_frames_in_the_window_switches_it_off() {
        let frames = vec![(3, 20); OFF_FRAMES as usize];
        let (on, verdicts) = run(true, &frames);
        assert!(!on);
        assert_eq!(*verdicts.last().unwrap(), Verdict::TrimOff);
        assert!(verdicts[..verdicts.len() - 1].iter().all(|&v| v == Verdict::Keep));
    }

    #[test]
    fn long_tailed_slow_frames_between_them_do_not_clear_the_count() {
        // A CPU-bound view whose slow frames alternate with GPU-looking ones: the old rule, which cleared on every
        // long tail, never reached its count here and the verdict hung on whichever frame came last.
        let mut frames = Vec::new();
        for _ in 0..OFF_FRAMES {
            frames.push((3, 20));
            frames.push((3, 300));
        }
        let (_, verdicts) = run(true, &frames);
        assert!(verdicts.contains(&Verdict::TrimOff));
    }

    #[test]
    fn a_route_with_one_slow_short_tailed_frame_in_a_dozen_never_switches_it_off() {
        // The measured shares with trimming on: kings-death 8%, cornifer-map 12%.
        let mut frames = Vec::new();
        for _ in 0..200 {
            frames.push((3, 20));
            for _ in 0..11 {
                frames.push((2, 5));
            }
        }
        let (on, verdicts) = run(true, &frames);
        assert!(on && verdicts.iter().all(|&v| v == Verdict::Keep));
    }

    #[test]
    fn slow_frames_far_apart_do_not_add_up() {
        let mut frames = Vec::new();
        for _ in 0..OFF_FRAMES - 1 {
            frames.push((3, 20));
            for _ in 0..WINDOW {
                frames.push((2, 5));
            }
        }
        let (on, _) = run(true, &frames);
        assert!(on);
    }

    #[test]
    fn a_loading_gap_restarts_the_count() {
        let mut frames = vec![(3, 20); OFF_FRAMES as usize - 1];
        frames.push((9, 0));
        frames.push((3, 20));
        let (on, _) = run(true, &frames);
        assert!(on);
    }

    #[test]
    fn off_with_the_slow_frames_still_there_goes_back_on() {
        // Measured with trimming off: cornifer-map and kings-death keep 73 to 97% of their frames slow.
        let frames = vec![(3, 100); ON_FRAMES as usize];
        let (on, verdicts) = run(false, &frames);
        assert!(on);
        assert_eq!(*verdicts.last().unwrap(), Verdict::TrimOn);
        assert!(verdicts[..verdicts.len() - 1].iter().all(|&v| v == Verdict::Keep));
    }

    #[test]
    fn off_stays_off_where_it_takes_the_slow_frames_away() {
        // c37-stand with trimming off: 30% of the frames slow.
        let mut frames = Vec::new();
        for _ in 0..300 {
            frames.extend([(3, 100), (2, 5), (2, 5), (3, 100), (2, 5), (2, 5), (2, 5), (3, 100), (2, 5), (2, 5)]);
        }
        let (on, verdicts) = run(false, &frames);
        assert!(!on && verdicts.iter().all(|&v| v == Verdict::Keep));
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
        let mut history = 3u32;
        assert_eq!(verdict(true, &mut history, 20, 0), Verdict::Keep);
        assert_eq!(history, 0);
    }
}
