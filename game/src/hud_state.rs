//! Pure, bounded geometry for a source-derived SOUL orb. No GPU or asset access.
/// Keep the original first row; testing upgrades wrap below it within320px.
pub fn mask_position(index: u16) -> (i16, i16) {
    (56 + (index % 14) as i16 * 16, 14 + (index / 14) as i16 * 16)
}
pub fn geo_y(total_masks: u16) -> i16 {
    38 + total_masks.saturating_sub(1).min(30) as i16 / 14 * 16
}
#[derive(Clone, Copy)]
pub struct SoulSpec {
    pub cut_zero: i32,
    pub cut_per_mp: i32,
    pub height: u16,
    pub focus_cost: u16,
    pub eyes_at: u16,
    pub hide_at: u16,
    pub dim_gain: u8,
    pub source_max: u16,
}
#[derive(Debug, PartialEq, Eq)]
pub struct SoulState {
    pub cut: u16,
    pub gain: u8,
    pub eyes: bool,
    pub fill: bool,
}
impl SoulSpec {
    pub fn state(&self, soul: u16, max_soul: u16) -> SoulState {
        let soul = soul.min(max_soul).min(self.source_max);
        // Cooked geometry is at most32 pixels and source SOUL at most99;
        // Q16 intermediates fit i32, without guest i64 multiply/division.
        let cut = ((self.cut_zero - i32::from(soul) * self.cut_per_mp + 32768) >> 16)
            .clamp(0, i32::from(self.height)) as u16;
        SoulState {
            cut,
            gain: if soul >= self.focus_cost {
                128
            } else {
                self.dim_gain
            },
            eyes: soul >= self.eyes_at,
            fill: soul > self.hide_at && cut < self.height,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expanded_masks_keep_default_layout_and_clear_the_geo_label() {
        assert_eq!(mask_position(5), (136, 14));
        assert_eq!(geo_y(7), 38);
        for i in 0..31 {
            let (x, y) = mask_position(i);
            assert!((56..=264).contains(&x));
            assert!(y + 16 < geo_y(i + 1));
        }
        assert_eq!(mask_position(14), (56, 30));
        assert_eq!(geo_y(31), 70);
    }
    // Synthetic geometry; source gameplay thresholds are independent of UVs.
    fn spec() -> SoulSpec {
        SoulSpec {
            cut_zero: 20 << 16,
            cut_per_mp: 13240,
            height: 21,
            focus_cost: 33,
            eyes_at: 50,
            hide_at: 1,
            dim_gain: 55,
            source_max: 99,
        }
    }
    #[test]
    fn empty_and_focus_eye_thresholds_are_independent() {
        let p = spec();
        assert!(!p.state(0, 99).fill);
        assert!(!p.state(1, 99).fill);
        for (soul, gain, eyes) in [
            (32, 55, false),
            (33, 128, false),
            (49, 128, false),
            (50, 128, true),
            (99, 128, true),
        ] {
            let s = p.state(soul, 99);
            assert_eq!((s.gain, s.eyes, s.fill), (gain, eyes, true));
            assert!(s.cut < p.height);
        }
    }
    #[test]
    fn dynamic_and_source_maxima_both_clamp_all_outputs() {
        let p = spec();
        assert_eq!(p.state(u16::MAX, u16::MAX), p.state(99, 99));
        assert_eq!(p.state(99, 32), p.state(32, 99));
        assert_eq!(p.state(99, 0), p.state(0, 99));
    }
    #[test]
    fn every_input_produces_monotonic_bounded_crop_without_empty_draws() {
        let p = spec();
        let mut previous = p.height;
        for soul in 0..=u16::MAX {
            let s = p.state(soul, 99);
            assert!(s.cut <= previous && s.cut <= p.height);
            if s.fill {
                assert!(p.height - s.cut > 0);
            }
            previous = s.cut;
        }
    }
    #[test]
    fn extreme_surface_positions_hide_or_fill_without_uv_underflow() {
        let mut p = spec();
        p.cut_zero = 40 << 16;
        assert_eq!(p.state(33, 99).cut, p.height);
        assert!(!p.state(33, 99).fill);
        p.cut_zero = -(4 << 16);
        assert_eq!(p.state(33, 99).cut, 0);
        assert!(p.state(33, 99).fill);
    }
}
