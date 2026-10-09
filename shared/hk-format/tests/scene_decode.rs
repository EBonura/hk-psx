#[path = "../../../game/src/room_decode.rs"]
mod room_decode;
use room_decode::{Decoder, Error};
fn put(b: &mut [u8], at: usize, n: u32) {
    b[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn word(b: &mut [u8], at: usize, n: u16) {
    b[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn fixture() -> Vec<u8> {
    let counts = [2usize, 2, 2, 1, 1, 2, 1, 1, 24];
    let sizes = [32, 88, 20, 16, 32, 32, 32768, 24, 80, 32];
    let mut offsets = [0usize; 10];
    let mut end = 128;
    for i in 0..10 {
        offsets[i] = end;
        end += sizes[i];
    }
    let mut b = vec![0; end];
    b[..8].copy_from_slice(b"HKSCNE01");
    put(&mut b, 8, 7);
    for (i, n) in counts.iter().enumerate() {
        put(&mut b, 12 + i * 4, *n as u32);
    }
    put(&mut b, 48, end as u32);
    put(&mut b, 52, 1);
    for (i, o) in offsets.iter().enumerate() {
        put(&mut b, 64 + i * 4, *o as u32);
    }
    let t = offsets[0];
    word(&mut b, t + 6, 1);
    word(&mut b, t + 8, 1);
    word(&mut b, t + 16, 65535);
    word(&mut b, t + 22, 1);
    word(&mut b, t + 24, 1);
    put(&mut b, t + 28, 20);
    for i in 0..2 {
        put(&mut b, offsets[1] + i * 44 + 4, 65536);
    }
    put(&mut b, offsets[2], 1);
    put(&mut b, offsets[3] + 4, 1);
    put(&mut b, offsets[3] + 8, 65536);
    put(&mut b, offsets[4] + 8, 65536);
    put(&mut b, offsets[4] + 16 + 8, 131072);
    b[offsets[7]] = 1;
    b[offsets[7] + 20] = 1;
    for i in 0..2 {
        let p = offsets[8] + i * 40;
        put(&mut b, p, 1 + i as u32 * 2);
        for s in 0..4 {
            put(&mut b, p + 4 + s * 4, 1);
            put(&mut b, p + 20 + s * 4, (offsets[9] + i * 16 + s * 4) as u32);
        }
        word(&mut b, offsets[9] + i * 16, i as u16);
        word(&mut b, offsets[9] + i * 16 + 12, i as u16);
    }
    b
}

fn framed(raw: &[u8]) -> Vec<u8> {
    let mut out = b"HLZC".to_vec();
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out.push(0xf0);
    let mut n = raw.len() - 15;
    while n >= 255 {
        out.push(255);
        n -= 255;
    }
    out.push(n as u8);
    out.extend_from_slice(raw);
    out
}
fn finish(mut decoder: Decoder, arena: &mut [u8], budget: usize) -> Result<usize, Error> {
    for _ in 0..1_000_000 {
        if let Some(n) = decoder.step(arena, budget)? {
            return Ok(n);
        }
    }
    panic!("scene decoder failed to progress")
}
#[test]
fn scene_raw_and_hlzc_use_scene_validation_and_identical_bounded_bytes() {
    let raw = fixture();
    hk_format::Scene::parse(&raw).unwrap();
    for source in [raw.clone(), framed(&raw)] {
        for budget in [1, 31, 1024, 8192] {
            let mut arena = vec![0xa5; raw.len() + source.len() + 64];
            arena[..source.len()].copy_from_slice(&source);
            let d = Decoder::new_scene(
                source.len(),
                psx_pack::fnv1a32(&source),
                raw.len(),
                psx_pack::fnv1a32(&raw),
            );
            assert_eq!(finish(d, &mut arena, budget), Ok(raw.len()));
            assert_eq!(&arena[..raw.len()], raw);
            // Payload type is explicit: a room decoder must reject a scene.
            let mut input = raw.clone();
            let d = Decoder::new(
                raw.len(),
                psx_pack::fnv1a32(&raw),
                raw.len(),
                psx_pack::fnv1a32(&raw),
            );
            assert_eq!(finish(d, &mut input, budget), Err(Error::RoomFormat));
        }
    }
}
#[test]
fn scene_checksums_and_late_indirect_references_never_publish() {
    let raw = fixture();
    let hash = psx_pack::fnv1a32(&raw);
    for (stored, decoded) in [(hash ^ 1, hash), (hash, hash ^ 1)] {
        let mut arena = raw.clone();
        let d = Decoder::new_scene(raw.len(), stored, raw.len(), decoded);
        assert_eq!(finish(d, &mut arena, 31), Err(Error::Checksum));
    }
    let mut bad = raw.clone();
    let end = bad.len();
    bad[end - 4..end - 2].copy_from_slice(&u16::MAX.to_le_bytes());
    let h = psx_pack::fnv1a32(&bad);
    let d = Decoder::new_scene(bad.len(), h, bad.len(), h);
    assert_eq!(finish(d, &mut bad, 1), Err(Error::RoomFormat));
}
#[test]
fn sequential_scene_suffix_decode_preserves_admitted_prefix() {
    let first = fixture();
    let mut second = fixture();
    put(&mut second, 8, 8);
    let source = framed(&second);
    let offset = (first.len() + 3) & !3;
    let mut arena = vec![0xa5; offset + source.len() + second.len() + 64];
    arena[..first.len()].copy_from_slice(&first);
    let prefix = arena[..offset].to_vec();
    arena[offset..offset + source.len()].copy_from_slice(&source);
    let d = Decoder::new_scene(
        source.len(),
        psx_pack::fnv1a32(&source),
        second.len(),
        psx_pack::fnv1a32(&second),
    );
    assert_eq!(finish(d, &mut arena[offset..], 1), Ok(second.len()));
    assert_eq!(&arena[..offset], prefix);
    assert_eq!(&arena[offset..offset + second.len()], second);
    let scene = unsafe { hk_format::Scene::validated_view(&arena[..first.len()]) };
    assert_eq!(scene.id(), 7);
    assert_eq!(scene.room_by_chunk(3).unwrap().edge(0)[2], 131072);
}
