use super::command;

// Frozen color operations from scenery's gain calculation and quad_words.
// This includes UV/CLUT/TPAGE modifications so tests explicitly verify that
// replacing only words[0] cannot alter other material packet fields.
fn legacy(template: [u32; 9], tint: (u8, u8, u8), gain: u8, opacity: u16) -> [u32; 9] {
    let gain = gain as u16;
    let tint = (
        (tint.0 as u16 * gain / 128) as u8,
        (tint.1 as u16 * gain / 128) as u8,
        (tint.2 as u16 * gain / 128) as u8,
    );
    let mut words = template;
    words[0] = (template[0] & 0xff00_0000)
        | tint.0 as u32
        | ((tint.1 as u32) << 8)
        | ((tint.2 as u32) << 16);
    if opacity & 256 != 0 {
        words[4] &= !(3 << 21);
        words[0] = (words[0] & !255) | 127;
    }
    let opacity = opacity & 255;
    if opacity < 128 {
        let a = opacity as u32;
        words[0] = (words[0] & 0xff00_0000) | a | (a << 8) | (a << 16);
        words[2] = (words[2] & 0xffff) | (0x7814 << 16);
        words[4] = (words[4] & !(3 << 21)) | (2 << 21);
    }
    words
}
#[test]
fn every_gain_channel_and_material_matches_original_rounding() {
    // Exhaust independent channel inputs, all gain bytes (including values
    // beyond the live clamp), and command-byte patterns including real0x2e.
    for high in [0x00, 0x2c, 0x2e, 0x3e, 0x80, 0xff] {
        let template = [
            (high << 24) | 0x713a9b,
            0,
            0x2468_abcd,
            0,
            0xffff_9876,
            0,
            0,
            0,
            0,
        ];
        for gain in 0..=255u8 {
            for value in 0..=255u8 {
                for tint in [(value, 73, 191), (211, value, 19), (37, 143, value)] {
                    for opacity in [128, 255, 128 | 256, 255 | 256] {
                        assert_eq!(
                            command(template[0], tint, gain, opacity),
                            legacy(template, tint, gain, opacity)[0]
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn every_opacity_matches_and_preserves_noncolor_packet_words() {
    let mut seed = 0x3a71938bu32;
    let mut random = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for opacity in 0..=511u16 {
        for _ in 0..512 {
            let template = core::array::from_fn(|_| random());
            let rgb = random();
            let gain = random() as u8;
            let tint = (rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8);
            let expected = legacy(template, tint, gain, opacity);
            let actual =
                super::with_material(template, command(0, tint, gain, opacity), opacity, 0x7814);
            assert_eq!(actual, expected);
        }
    }
}
#[test]
fn opaque_nonblack_zero_tint_and_repeated_live_changes_are_exact() {
    let mut seed = 0x5391952bu32;
    for _ in 0..100000 {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let tint = (seed as u8, (seed >> 8) as u8, (seed >> 16) as u8);
        for gain in [0, 1, 31, 64, 127, 128] {
            let c = command(0x2e123456, tint, gain, 128);
            let zero = ((tint.0 as u16 * gain as u16 / 128) as u8
                | (tint.1 as u16 * gain as u16 / 128) as u8
                | (tint.2 as u16 * gain as u16 / 128) as u8)
                == 0;
            assert_eq!(c & 0xffffff == 0, zero);
            for opacity in [0, 1, 64, 127, 128, 128 | 256, 31 | 256, 128] {
                assert_eq!(
                    command(0x2e123456, tint, gain, opacity),
                    legacy([0x2e123456; 9], tint, gain, opacity)[0]
                );
                let old_core = opacity & 255 == 128 && (opacity & 256 != 0 || zero);
                let cached_zero = opacity == 128 && command(0, tint, gain, opacity) == 0;
                let new_core = opacity & 255 == 128 && (opacity & 256 != 0 || cached_zero);
                assert_eq!(new_core, old_core);
            }
        }
    }
}
