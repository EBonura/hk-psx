#[allow(clippy::all, unexpected_cfgs, dead_code)] // game source, linted with the game
#[path = "../../../game/src/room_decode.rs"]
mod room_decode;
fn main() {
    let capacity: usize = std::env::var("HK_ROOM_ARENA_BYTES")
        .map(|v| v.parse().expect("arena byte capacity"))
        .unwrap_or(768 * 1024);
    for name in std::env::args().skip(1) {
        let source = std::fs::read(&name).unwrap();
        let mut expected = vec![0; capacity];
        expected[..source.len()].copy_from_slice(&source);
        let n = psx_pack::decompress_hlzc_in_place(&mut expected, source.len()).unwrap();
        for budget in [1, 31, 1024, 2048, 8192] {
            let mut arena = vec![0; capacity];
            arena[..source.len()].copy_from_slice(&source);
            let mut decoder = room_decode::Decoder::new(
                source.len(),
                psx_pack::fnv1a32(&source),
                n,
                psx_pack::fnv1a32(&expected[..n]),
            );
            let mut calls = 0;
            loop {
                calls += 1;
                assert!(calls < 4_000_000);
                if let Some(len) = decoder
                    .step(&mut arena, budget)
                    .expect("incremental decoder")
                {
                    assert_eq!(len, n);
                    assert_eq!(&arena[..n], &expected[..n]);
                    break;
                }
            }
        }
        println!("{} {}", name, n);
    }
}
