//! The spec pool across scene swaps, on the real chunks of data/scene-specs (skipped on a checkout that has not
//! cooked): the seam between the outgoing and the incoming scene. A scene's specs are the pool's only while the scene
//! is loaded; loading another replaces them, loading the first again restores them byte for byte; a bad chunk leaves an
//! empty pool; and a reference taken for the outgoing scene reads the incoming scene's data afterwards, which is why
//! the world rebuilds its `placed_spec` table whenever the admitted region changes.
use hk_sim::wire::SpecPool;
use hk_sim::ActorSpec;
use std::path::PathBuf;

type Pool = SpecPool<24, 24>;

fn chunk(id: usize) -> Option<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../data/scene-specs/scene_{id}.hkspec"));
    std::fs::read(path).ok()
}

/// The pool as a raw pointer, the way the guest holds its static: `load` takes `&'static mut`, `specs` `&'static`.
fn fresh() -> *mut Pool {
    Box::leak(Box::new(Pool::new()))
}
fn load(pool: *mut Pool, id: usize, bytes: &[u8]) -> bool {
    unsafe { (*pool).load(id, bytes) }
}
fn specs(pool: *mut Pool, id: usize) -> &'static [ActorSpec] {
    unsafe { (*pool).specs(id) }
}

fn dump(specs: &[ActorSpec]) -> Vec<String> {
    specs.iter().map(|s| format!("{s:?}")).collect()
}

fn scenes() -> Vec<(usize, Vec<u8>)> {
    (0..65).filter_map(|id| chunk(id).map(|c| (id, c))).collect()
}

#[test]
fn a_swap_replaces_the_scene_and_swapping_back_restores_it() {
    let scenes = scenes();
    if scenes.len() < 2 {
        return;
    }
    // The ring A -> B -> A over every neighbouring pair, then the reverse order.
    let mut order: Vec<usize> = (0..scenes.len()).collect();
    order.extend((0..scenes.len()).rev());
    let pool = fresh();
    let mut first_seen: Vec<Option<Vec<String>>> = vec![None; scenes.len()];
    for &k in &order {
        let (id, bytes) = &scenes[k];
        assert!(load(pool, *id, bytes), "scene {id} loads");
        let now = dump(specs(pool, *id));
        match &first_seen[k] {
            None => first_seen[k] = Some(now),
            Some(before) => assert_eq!(*before, now, "scene {id} after a round trip"),
        }
        // Nobody else's specs are visible while this scene is loaded.
        for (other, _) in scenes.iter().filter(|(o, _)| o != id) {
            assert!(specs(pool, *other).is_empty(), "scene {other} visible while {id} is loaded");
        }
        // Corpse references land in the pool's own corpse list.
        for spec in specs(pool, *id) {
            if let Some(corpse) = spec.corpse {
                let c = corpse as *const _ as usize;
                let base = pool as usize;
                assert!(c >= base && c < base + std::mem::size_of::<Pool>(), "scene {id}: corpse outside the pool");
            }
        }
    }
}

#[test]
fn a_reference_to_the_outgoing_scene_reads_the_incoming_one() {
    let scenes: Vec<_> = scenes().into_iter().filter(|(_, b)| hk_sim::wire::read_chunk_head(b).is_some_and(|h| h.specs > 0)).collect();
    if scenes.len() < 2 {
        return;
    }
    let pool = fresh();
    let (a, b) = (&scenes[0], &scenes[scenes.len() - 1]);
    assert!(load(pool, a.0, &a.1));
    let first: &'static ActorSpec = &specs(pool, a.0)[0];
    assert!(load(pool, b.0, &b.1));
    // The stale reference now shows the incoming scene's first spec: never a dangling read, never the old one.
    let incoming = format!("{:?}", specs(pool, b.0)[0]);
    assert_eq!(format!("{first:?}"), incoming);
}

#[test]
fn a_bad_chunk_leaves_an_empty_pool() {
    let scenes = scenes();
    if scenes.len() < 2 {
        return;
    }
    let pool = fresh();
    let (a, b) = (&scenes[0], &scenes[1]);
    assert!(load(pool, a.0, &a.1));
    // Another scene's chunk under this scene's id.
    assert!(!load(pool, a.0, &b.1));
    assert!(specs(pool, a.0).is_empty());
    // A flipped byte in the payload.
    let mut bad = a.1.clone();
    bad[hk_sim::wire::CHUNK_HEAD] ^= 1;
    assert!(!load(pool, a.0, &bad));
    assert!(specs(pool, a.0).is_empty());
    // A pool too small for the chunk.
    let tiny: &'static mut SpecPool<1, 1> = Box::leak(Box::new(SpecPool::new()));
    if hk_sim::wire::read_chunk_head(&a.1).is_some_and(|h| h.specs > 1) {
        assert!(!tiny.load(a.0, &a.1));
    }
}
