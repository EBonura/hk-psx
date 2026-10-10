use hk_format::coverage::{
    CoverageCert, CoverageGroup, CoverageValidation, CoverageView, Expected, NO_CERT,
};
use std::ops::{Deref, DerefMut};
#[derive(Clone)]
struct Aligned(Vec<u32>);
impl Deref for Aligned {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.0.as_ptr().cast(), self.0.len() * 4) }
    }
}
impl DerefMut for Aligned {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.0.as_mut_ptr().cast(), self.0.len() * 4) }
    }
}
fn put(b: &mut [u8], p: usize, n: u32) {
    b[p..p + 4].copy_from_slice(&n.to_le_bytes());
}
fn half(b: &mut [u8], p: usize, n: u16) {
    b[p..p + 2].copy_from_slice(&n.to_le_bytes());
}
fn offset(b: &[u8], section: usize) -> usize {
    hk_format::u32_at(b, 32 + section * 8) as usize
}
const OWNER: Expected = Expected {
    scene_id: 7,
    scene_raw_fnv: 0x11223344,
    atlas_fnv: 0x55667788,
    draw_pool_count: 3,
};
fn fixture_with_groups(groups: &[[u16; 4]]) -> Aligned {
    let counts = [2, 3, 3, 1, 1, groups.len()];
    let sizes = [12, 4, 2, 12, 4, 10];
    let mut offsets = [0; 6];
    let mut end = 80;
    for i in 0..6 {
        end = (end + 3) & !3;
        offsets[i] = end;
        end += counts[i] * sizes[i];
    }
    let mut b = Aligned(vec![0; end.div_ceil(4)]);
    b[..8].copy_from_slice(b"HKOCSC01");
    for (i, n) in [
        OWNER.scene_id,
        OWNER.scene_raw_fnv,
        OWNER.atlas_fnv,
        OWNER.draw_pool_count,
        2,
        0,
    ]
    .iter()
    .enumerate()
    {
        put(&mut b, 8 + i * 4, *n);
    }
    for i in 0..6 {
        put(&mut b, 32 + i * 8, offsets[i] as u32);
        put(&mut b, 36 + i * 8, counts[i] as u32);
    }
    let p = offsets[0];
    half(&mut b, p, (-9i16) as u16);
    half(&mut b, p + 2, 7);
    half(&mut b, p + 4, 3);
    half(&mut b, p + 6, 2);
    half(&mut b, p + 12 + 4, 33);
    half(&mut b, p + 12 + 6, 1);
    put(&mut b, p + 12 + 8, 32);
    put(&mut b, offsets[1], 0b111111);
    put(&mut b, offsets[1] + 4, u32::MAX);
    put(&mut b, offsets[1] + 8, 1);
    for (i, n) in [0, NO_CERT, 1].iter().enumerate() {
        half(&mut b, offsets[2] + i * 2, *n);
    }
    half(&mut b, offsets[3] + 4, 2);
    half(&mut b, offsets[3] + 6, 1);
    put(&mut b, offsets[4], 3);
    for (i, members) in groups.iter().enumerate() {
        for (j, id) in members.iter().enumerate() {
            half(&mut b, offsets[5] + i * 10 + 2 + j * 2, *id);
        }
    }
    b
}
fn fixture() -> Aligned {
    fixture_with_groups(&[[0, 2, NO_CERT, NO_CERT]])
}
fn rejected(b: &[u8]) {
    assert!(CoverageView::parse(b, OWNER).is_err());
}

#[test]
fn typed_views_borrow_checked_layout_and_resolve_scene_local_ids() {
    let b = fixture();
    let v = CoverageView::parse(&b, OWNER).unwrap();
    assert_eq!(std::mem::size_of::<CoverageCert>(), 12);
    assert_eq!(std::mem::size_of::<CoverageGroup>(), 10);
    assert_eq!(
        std::mem::size_of::<CoverageView>(),
        std::mem::size_of::<&[u8]>()
    );
    assert_eq!(v.scene_id(), 7);
    assert_eq!(v.byte_len(), b.len());
    assert_eq!(v.grid_shift(), 2);
    assert_eq!(v.draw_pool_count(), 3);
    assert_eq!(v.pool_map(), &[0, NO_CERT, 1]);
    assert_eq!(v.draw_certificate(0), Some(0));
    assert_eq!(v.draw_certificate(1), None);
    assert_eq!(v.draw_certificate(2), Some(1));
    assert_eq!(v.draw_certificate(3), None);
    assert_eq!(
        v.tile_certs()[0],
        CoverageCert {
            gx: -9,
            gy: 7,
            width: 3,
            height: 2,
            offset: 0
        }
    );
    assert_eq!(v.tile_cert(1).unwrap().offset, 32);
    assert!(v.tile_cert(NO_CERT).is_none());
    assert_eq!(v.tile_bits(), &[63, u32::MAX, 1]);
    assert_eq!(v.group_bits(), &[3]);
    assert_eq!(v.group_cert(0).unwrap().width, 2);
    assert!(v.group_cert(1).is_none());
    assert_eq!(
        v.groups(),
        &[CoverageGroup {
            certificate: 0,
            members: [0, 2, NO_CERT, NO_CERT]
        }]
    );
    assert_eq!(
        v.pool_map().as_ptr().cast::<u8>(),
        b[offset(&b, 2)..].as_ptr()
    );
    let trusted = unsafe { CoverageView::validated_view(&b) };
    assert_eq!(trusted.tile_certs(), v.tile_certs());
    assert_eq!(trusted.groups(), v.groups());
}
#[test]
fn header_rejects_wrong_owners_limits_offsets_padding_alignment_and_truncation() {
    let b = fixture();
    for expected in [
        Expected {
            scene_id: 8,
            ..OWNER
        },
        Expected {
            scene_raw_fnv: 0,
            ..OWNER
        },
        Expected {
            atlas_fnv: 0,
            ..OWNER
        },
        Expected {
            draw_pool_count: 4,
            ..OWNER
        },
    ] {
        assert!(CoverageValidation::new(&b, expected).is_err());
    }
    for p in [0, 8, 12, 16, 20, 24, 28] {
        let mut bad = b.clone();
        bad[p] ^= 1;
        rejected(&bad);
    }
    for section in 0..6 {
        for value in [0, 79, 81, offset(&b, section) as u32 + 4, u32::MAX] {
            let mut bad = b.clone();
            put(&mut bad, 32 + section * 8, value);
            rejected(&bad);
        }
        let mut bad = b.clone();
        put(&mut bad, 36 + section * 8, u32::MAX);
        rejected(&bad);
    }
    for end in 0..b.len() {
        rejected(&b[..end]);
    }
    let mut extra = b.clone();
    extra.0.push(0);
    rejected(&extra);
    let mut bad = b.clone();
    bad[offset(&b, 2) + 6] = 1;
    rejected(&bad); // map -> cert alignment
    let mut bad = b.clone();
    let end = bad.len();
    bad[end - 1] = 1;
    rejected(&bad); // group tail
    let mut unaligned = Aligned(vec![0; b.0.len() + 1]);
    unaligned[1..1 + b.len()].copy_from_slice(&b);
    rejected(&unaligned[1..1 + b.len()]);
    let mut bad = b.clone();
    put(&mut bad, 20, 65536);
    assert!(CoverageValidation::new(
        &bad,
        Expected {
            draw_pool_count: 65536,
            ..OWNER
        }
    )
    .is_err());
}
#[test]
fn certificates_require_nonempty_shapes_canonical_word_offsets_and_zero_unused_bits() {
    let b = fixture();
    for section in [0, 3] {
        for field in [4, 6] {
            let mut bad = b.clone();
            half(&mut bad, offset(&b, section) + field, 0);
            rejected(&bad);
        }
        for start in [1, 32, u32::MAX] {
            let mut bad = b.clone();
            put(&mut bad, offset(&b, section) + 8, start);
            rejected(&bad);
        }
        let mut bad = b.clone();
        half(&mut bad, offset(&b, section) + 4, u16::MAX);
        half(&mut bad, offset(&b, section) + 6, u16::MAX);
        rejected(&bad);
    }
    for start in [0, 31, 64, u32::MAX] {
        let mut bad = b.clone();
        put(&mut bad, offset(&b, 0) + 20, start);
        rejected(&bad);
    }
    for (p, n) in [
        (offset(&b, 1), 64),
        (offset(&b, 1) + 8, 2),
        (offset(&b, 4), 4),
    ] {
        let mut bad = b.clone();
        put(&mut bad, p, n);
        rejected(&bad);
    }
    // Internal full words have no unused bits; all combinations are legal.
    let mut valid = b.clone();
    put(&mut valid, offset(&b, 1) + 4, 0x81234567);
    CoverageView::parse(&valid, OWNER).unwrap();
    // Shrinking the second descriptor leaves a spare bitmap word, rejected at
    // phase completion rather than accepting unowned data at the end.
    let mut bad = b.clone();
    half(&mut bad, offset(&b, 0) + 16, 1);
    put(&mut bad, offset(&b, 1) + 4, 0);
    rejected(&bad);
}
#[test]
fn all_map_group_references_membership_order_and_duplicates_are_checked() {
    let b = fixture();
    let mut bad = b.clone();
    half(&mut bad, offset(&b, 2), 2);
    rejected(&bad);
    let mut bad = b.clone();
    half(&mut bad, offset(&b, 5), 1);
    rejected(&bad);
    let mut bad = b.clone();
    half(&mut bad, offset(&b, 5), NO_CERT);
    rejected(&bad);
    for members in [
        [NO_CERT; 4],
        [0, NO_CERT, NO_CERT, NO_CERT],
        [0, NO_CERT, 2, NO_CERT],
        [0, 0, NO_CERT, NO_CERT],
        [2, 0, NO_CERT, NO_CERT],
        [0, 3, NO_CERT, NO_CERT],
    ] {
        rejected(&fixture_with_groups(&[members]));
    }
    let group = [0, 2, NO_CERT, NO_CERT];
    rejected(&fixture_with_groups(&[group, group]));
    rejected(&fixture_with_groups(&[
        [0, 2, NO_CERT, NO_CERT],
        [0, 1, NO_CERT, NO_CERT],
    ]));
    // Canonical tuple order places a shorter prefix before its extension.
    let valid = fixture_with_groups(&[
        [0, 1, NO_CERT, NO_CERT],
        [0, 1, 2, NO_CERT],
        [0, 2, NO_CERT, NO_CERT],
    ]);
    assert_eq!(
        CoverageView::parse(&valid, OWNER).unwrap().groups().len(),
        3
    );
}
#[test]
fn validator_is_bounded_does_not_publish_early_and_latches_failed_admission() {
    let b = fixture();
    let mut check = CoverageValidation::new(&b, OWNER).unwrap();
    assert!(std::mem::size_of_val(&check) <= 64);
    assert!(!std::mem::needs_drop::<CoverageValidation>());
    assert!(!check.step(0).unwrap());
    assert!(CoverageValidation::new(&b, OWNER)
        .unwrap()
        .finish()
        .is_err());
    // 2 tile certs +3 map entries +1 group cert +1 group +4 phase transitions.
    for _ in 0..10 {
        assert!(!check.step(1).unwrap());
    }
    assert!(check.step(1).unwrap());
    assert!(check.step(0).unwrap());
    check.finish().unwrap();
    let mut bad = b.clone();
    half(&mut bad, offset(&b, 2) + 4, 2);
    let mut check = CoverageValidation::new(&bad, OWNER).unwrap();
    assert!(!check.step(5).unwrap());
    assert!(check.step(1).is_err());
    assert!(check.step(usize::MAX).is_err());
    assert!(check.finish().is_err());
}
#[test]
fn no_certificates_is_a_valid_explicit_owner_with_a_full_no_certificate_pool_map() {
    let mut b = Aligned(vec![0; 22]);
    b[..8].copy_from_slice(b"HKOCSC01");
    for (i, n) in [
        OWNER.scene_id,
        OWNER.scene_raw_fnv,
        OWNER.atlas_fnv,
        3,
        2,
        0,
    ]
    .iter()
    .enumerate()
    {
        put(&mut b, 8 + i * 4, *n);
    }
    for (section, p) in [80, 80, 80, 88, 88, 88].iter().enumerate() {
        put(&mut b, 32 + section * 8, *p);
    }
    put(&mut b, 36 + 2 * 8, 3);
    for p in [80, 82, 84] {
        half(&mut b, p, NO_CERT);
    }
    let v = CoverageView::parse(&b, OWNER).unwrap();
    assert!(v.tile_certs().is_empty() && v.groups().is_empty());
    assert_eq!(v.pool_map(), &[NO_CERT; 3]);
}
