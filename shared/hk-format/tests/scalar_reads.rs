//! Alignment, exact-value and rejection parity for safe little-endian fields.
use hk_format as candidate;
// Preserve the previous scalar reader as an independent regression reference.
mod original {
    pub fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }
    pub fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }
    pub fn i32_at(b: &[u8], i: usize) -> i32 {
        u32_at(b, i) as i32
    }
}
#[repr(align(16))]
struct Aligned([u8; 32]);
#[test]
fn all_u16_values_at_all_word_alignments() {
    let mut bytes = Aligned([0; 32]);
    for value in 0..=u16::MAX {
        for base in 0..4 {
            bytes.0[base..base + 2].copy_from_slice(&value.to_le_bytes());
            assert_eq!(candidate::u16_at(&bytes.0[base..], 0), value);
            assert_eq!(
                candidate::u16_at(&bytes.0, base),
                original::u16_at(&bytes.0, base)
            );
        }
    }
}
#[test]
fn deterministic_u32_and_signed_values_at_all_word_alignments() {
    let mut bytes = Aligned([0; 32]);
    let mut seed = 0x83ea0b15u32;
    for iteration in 0..100_000 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let value = match iteration {
            0 => 0,
            1 => u32::MAX,
            2 => i32::MAX as u32,
            3 => i32::MIN as u32,
            _ => seed,
        };
        for base in 0..4 {
            for offset in 0..4 {
                bytes.0[base + offset..base + offset + 4].copy_from_slice(&value.to_le_bytes());
                let slice = &bytes.0[base..];
                assert_eq!(candidate::u32_at(slice, offset), value);
                assert_eq!(candidate::i32_at(slice, offset), value as i32);
                assert_eq!(
                    candidate::u32_at(slice, offset),
                    original::u32_at(slice, offset)
                );
                assert_eq!(
                    candidate::i32_at(slice, offset),
                    original::i32_at(slice, offset)
                );
            }
        }
    }
}
#[test]
fn truncated_empty_boundary_and_overflow_offsets_have_identical_rejection() {
    let data = Aligned(core::array::from_fn(|i| (i * 71) as u8));
    for base in 0..4 {
        for length in 0..=16 {
            let bytes = &data.0[base..base + length];
            for offset in
                (0..=18).chain([usize::MAX, usize::MAX - 1, usize::MAX - 2, usize::MAX - 3])
            {
                for width in [2, 4] {
                    let expected = offset <= length && length - offset >= width;
                    let a = std::panic::catch_unwind(|| {
                        if width == 2 {
                            candidate::u16_at(bytes, offset) as u32
                        } else {
                            candidate::u32_at(bytes, offset)
                        }
                    });
                    let b = std::panic::catch_unwind(|| {
                        if width == 2 {
                            original::u16_at(bytes, offset) as u32
                        } else {
                            original::u32_at(bytes, offset)
                        }
                    });
                    assert_eq!(a.is_ok(), expected);
                    assert_eq!(b.is_ok(), expected);
                    if expected {
                        assert_eq!(a.unwrap(), b.unwrap());
                    }
                    let signed = std::panic::catch_unwind(|| candidate::i32_at(bytes, offset));
                    assert_eq!(signed.is_ok(), offset <= length && length - offset >= 4);
                }
            }
        }
    }
}
