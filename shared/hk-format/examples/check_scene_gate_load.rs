//! Host proof of repeated scene replacement using the actual guest decoder.
//! No CD/GPU timing claim: uploads are copied into a bounded physical atlas model.
#[allow(clippy::all, unexpected_cfgs, dead_code)] // game source, linted with the game
#[path = "../../../game/src/room_decode.rs"]
mod room_decode;
struct Blob {
    stored: Vec<u8>,
    raw: Vec<u8>,
}
struct Atlas {
    kind: usize,
    first: usize,
    count: usize,
    blob: Blob,
}
struct Bank {
    id: usize,
    scene: Blob,
    atlases: Vec<Atlas>,
    meta: Option<Blob>,
}
fn blob(stored: &str, raw: &str) -> Blob {
    Blob {
        stored: std::fs::read(stored).expect("stored file"),
        raw: std::fs::read(raw).expect("raw file"),
    }
}
fn decode(arena: &mut [u8], b: &Blob, budget: usize, scene: bool) {
    assert!(!b.stored.is_empty() && !b.raw.is_empty());
    assert!(
        b.stored.len().div_ceil(2048) * 2048 <= arena.len(),
        "sector-rounded read exceeds arena"
    );
    assert!(b.raw.len() <= arena.len(), "raw payload exceeds arena");
    arena[..b.stored.len()].copy_from_slice(&b.stored);
    // CD reads write whole sectors. Poison padding to expose reliance on stale bytes.
    arena[b.stored.len()..b.stored.len().div_ceil(2048) * 2048].fill(0x7b);
    let args = (
        b.stored.len(),
        psx_pack::fnv1a32(&b.stored),
        b.raw.len(),
        psx_pack::fnv1a32(&b.raw),
    );
    let mut d = if scene {
        room_decode::Decoder::new_scene(args.0, args.1, args.2, args.3)
    } else {
        room_decode::Decoder::new_bytes(args.0, args.1, args.2, args.3)
    };
    let mut calls = 0;
    loop {
        calls += 1;
        assert!(calls < 32_000_000, "decoder failed to progress");
        if let Some(n) = d.step(arena, budget).expect("guest decoder") {
            assert_eq!(n, b.raw.len());
            break;
        }
    }
    assert_eq!(
        &arena[..b.raw.len()],
        b.raw.as_slice(),
        "decoded payload differs"
    );
}
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let capacity: usize = a[0].parse().unwrap();
    assert!(capacity > 0 && capacity.is_multiple_of(4));
    let mut banks: Vec<Bank> = Vec::new();
    let mut i = 1;
    while i < a.len() {
        match a[i].as_str() {
            "--scene" => {
                banks.push(Bank {
                    id: a[i + 1].parse().unwrap(),
                    scene: blob(&a[i + 2], &a[i + 3]),
                    atlases: vec![],
                    meta: None,
                });
                i += 4;
            }
            "--meta" => {
                banks.last_mut().expect("meta follows scene").meta =
                    Some(blob(&a[i + 1], &a[i + 2]));
                i += 3;
            }
            "--atlas" => {
                banks
                    .last_mut()
                    .expect("atlas follows scene")
                    .atlases
                    .push(Atlas {
                        kind: a[i + 1].parse().unwrap(),
                        first: a[i + 2].parse().unwrap(),
                        count: a[i + 3].parse().unwrap(),
                        blob: blob(&a[i + 4], &a[i + 5]),
                    });
                i += 6;
            }
            _ => panic!("unknown argument"),
        }
    }
    assert!(!banks.is_empty());
    let mut ids = std::collections::HashSet::new();
    for b in &banks {
        assert!(ids.insert(b.id), "duplicate scene");
        assert_eq!(
            hk_format::Scene::parse(&b.scene.raw)
                .expect("scene format")
                .id(),
            b.id
        );
        let mut next = [0usize; 2];
        for a in &b.atlases {
            assert!(a.kind < 2 && a.count > 0);
            assert_eq!(a.first, next[a.kind], "atlas gap or overlap");
            next[a.kind] += a.count;
            assert!(next[a.kind] <= [20, 1248][a.kind], "physical atlas limit");
            assert_eq!(a.blob.raw.len(), a.count * [32768, 32][a.kind]);
        }
        let scene = hk_format::Scene::parse(&b.scene.raw).unwrap();
        assert_eq!(
            next,
            [scene.page_count(), scene.palette_count()],
            "atlas inventory differs from scene"
        );
    }
    for budget in [1, 31, 1024, 2048, 8192] {
        let mut arena = vec![0xa5; capacity];
        let mut vram = [vec![0xcc; 20 * 32768], vec![0xcc; 1248 * 32]];
        // Reverse and repeat: the largest bank overwrites a smaller bank too.
        let route = (0..banks.len())
            .chain((0..banks.len()).rev())
            .chain(0..banks.len());
        for index in route {
            let b = &banks[index];
            // The outgoing room must have no surviving references before this point.
            // Only successful completion below admits a new Scene view.
            for a in &b.atlases {
                decode(&mut arena, &a.blob, budget, false);
                let unit = [32768, 32][a.kind];
                let start = a.first * unit;
                let end = start + a.count * unit;
                let prefix = vram[a.kind][..start].to_vec();
                let suffix = vram[a.kind][end..].to_vec();
                vram[a.kind][start..end].copy_from_slice(&arena[..a.blob.raw.len()]);
                assert_eq!(&vram[a.kind][..start], prefix);
                assert_eq!(&vram[a.kind][end..], suffix);
            }
            decode(&mut arena, &b.scene, budget, true);
            assert_eq!(
                hk_format::Scene::parse(&arena[..b.scene.raw.len()])
                    .unwrap()
                    .id(),
                b.id
            );
            if let Some(meta) = &b.meta {
                // The guest stages the world bank last, inside the arena tail
                // beyond the resident scene, and reads it back during gameplay.
                let tail = (b.scene.raw.len() + 3) & !3;
                assert!(
                    tail + meta.raw.len() <= capacity,
                    "metadata bank exceeds the arena tail"
                );
                decode(&mut arena[tail..], meta, budget, false);
                assert_eq!(
                    &arena[..b.scene.raw.len()],
                    b.scene.raw.as_slice(),
                    "metadata decode changed the resident scene"
                );
                assert_eq!(
                    &arena[tail..tail + meta.raw.len()],
                    meta.raw.as_slice(),
                    "metadata bank differs after staging"
                );
            }
            for a in &b.atlases {
                let start = a.first * [32768, 32][a.kind];
                assert_eq!(
                    &vram[a.kind][start..start + a.blob.raw.len()],
                    a.blob.raw.as_slice(),
                    "scene decode changed uploaded data"
                );
            }
        }
        println!(
            "PASS budget={budget}, {} scene replacements, capacity={capacity}",
            banks.len() * 3
        );
    }
}
