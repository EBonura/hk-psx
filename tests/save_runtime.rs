//! The real save record, compiled and executed.
//!
//! `game/src/save.rs` carries a `#[cfg(test)] mod tests` that nothing in this
//! repository compiled: the guest is a `no_std` PSX binary and no cargo target
//! path-includes the file, because it pulls in `psx_mc` for the card I/O. Its
//! assertions were documentation. That is a bad place for a gap. The record is
//! the only thing standing between a player and their save, it grew twice in one
//! day (HKS1 to HKS2 for the charm board and the conversation cursors, HKS2 to
//! HKS3 for the script bank's PlayerData), and every growth silently
//! invalidates every committed card fixture.
//!
//! Only `encode`/`decode` are exercised here, which is the whole of what the
//! gap covered: they are pure functions of the record and never touch the card.
//! HKS3 to HKS4 added Sly's half of PlayerData, which is the growth that made
//! a shop purchase survive a quit, and HKS4 to HKS5 the port's own PlayerData
//! and SceneData (`game/src/persist.rs`), which is what keeps a beaten boss and
//! a broken wall that way. HKS5 is the first format an older one still loads
//! into, so the HKS4 half of `decode` is exercised here too.
//! The `psx_mc` surface below exists solely so the module compiles, and the
//! stubs panic rather than pretend, so a future test that strays into card I/O
//! fails loudly instead of passing against a fiction.
#![allow(dead_code)]
// `save.rs` writes `psx_mc::Slot` and `psx_mc::Error` as extern-crate paths, so
// the stubs have to live at this crate's root under that alias, the way
// tests/menu_runtime.rs aliases the GPU and pad crates.
extern crate self as psx_mc;

pub const FRAME_SIZE: usize = 128;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Mirrors psx_mc::Error exactly. save.rs matches on it exhaustively, so a
// variant missing here is a compile error rather than a silent gap.
pub enum Error { NoCard, Protocol, BadChecksum, OutOfRange, NotFormatted, NotFound, NoSpace,
    Exists, Corrupt, BufferTooSmall, BadContainer, Compression, BadName }
pub type Result<T> = core::result::Result<T, Error>;
pub enum Slot { One, Two }
pub trait Block {
    fn read_frame(&mut self, frame: u16, out: &mut [u8; FRAME_SIZE]) -> Result<()>;
    fn write_frame(&mut self, frame: u16, data: &[u8; FRAME_SIZE]) -> Result<()>;
}
pub struct HardwareCard;
impl HardwareCard {
    pub fn new(_: Slot) -> Self { panic!("card I/O is not exercised by this harness") }
}
impl Block for HardwareCard {
    fn read_frame(&mut self, _: u16, _: &mut [u8; FRAME_SIZE]) -> Result<()> { unreachable!() }
    fn write_frame(&mut self, _: u16, _: &[u8; FRAME_SIZE]) -> Result<()> { unreachable!() }
}
pub struct Card<B>(core::marker::PhantomData<B>);
impl<B: Block> Card<B> {
    pub fn new(_: B) -> Self { panic!("card I/O is not exercised by this harness") }
    pub fn is_formatted(&mut self) -> Result<bool> { unreachable!() }
    pub fn format(&mut self) -> Result<()> { unreachable!() }
    pub fn write(&mut self, _: &str, _: &str, _: &[u8]) -> Result<()> { unreachable!() }
    pub fn read(&mut self, _: &str, _: &mut [u8]) -> Result<usize> { unreachable!() }
}

pub mod shade {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct Record { pub present: bool, pub scene: u32, pub position: [i32; 2], pub hp: u16, pub geo_pool: u32 }
}
pub mod input {
    pub fn checkpoint() {}
}

#[path = "../game/src/persist.rs"]
pub mod persist;
#[path = "../game/src/save.rs"]
pub mod save;

use persist::Kind;
use save::{Save, LEN, LEN_HKS4, MAX_LEN, SCRIPT_FIELD_SLOTS, SHOP_COUNTERS};

fn record() -> Save {
    Save {
        scene: 3, seat: [-65536, 720896], facing: -1, region: 7, geo: 41, door_hits: 13,
        shade: shade::Record { present: true, scene: 2, position: [-131072, 65536], hp: 10, geo_pool: 77 },
        sequence: 9,
        charms_owned: 0x1234_5678_9abc_def0, charms_equipped: 0x0f0f_0f0f_0f0f_0f0f,
        charm_notches: 7, can_overcharm: true,
        npc_conversations: 0xdead_beef,
        script_fields: core::array::from_fn(|i| (i as i32 + 1) * -3),
        script_field_fnv: 814_805_480,
        shop_slots: 0x0000_0d6b,
        shop_counters: [3, 2, 4, 1, 1, 1],
        player_bools: 0x0000_0083,
        player_levels: [1, 0, 0, 0],
        version: 5,
    }
}
/// A world with one of every kind, in the store's own order.
fn world() -> persist::Store {
    let mut store = persist::Store::new();
    assert!(store.set(Kind::BattleScene, 11, 0, 1));
    assert!(store.set(Kind::Breakable, 0, 45, 1));
    assert!(store.set(Kind::GeoRock, 3, 2, 0));
    assert!(store.set(Kind::SecretMask, 28, 1, 1));
    assert!(store.set(Kind::Cocoon, 0, 0, 1));
    store
}
fn encode(save: &Save, items: &[u32]) -> Vec<u8> {
    let mut out = [0u8; MAX_LEN];
    let len = save.encode(items, &mut out);
    out[..len].to_vec()
}

#[test]
fn every_field_survives_the_round_trip() {
    // The encode and decode offsets are hand-maintained and were extended in
    // four steps. A field written to the wrong offset reads back as a different
    // field's value, and the record checksum cannot see it: the bytes are
    // intact, they are just in the wrong places.
    let save = record();
    let store = world();
    let bytes = encode(&save, store.items());
    assert_eq!(bytes.len(), LEN + store.items().len() * 4);
    let (back, items) = Save::decode(&bytes).expect("a record this module just wrote must decode");
    assert_eq!(back.scene, save.scene);
    assert_eq!(back.seat, save.seat);
    assert_eq!(back.facing, save.facing);
    assert_eq!(back.region, save.region);
    assert_eq!(back.geo, save.geo);
    assert_eq!(back.door_hits, save.door_hits);
    assert_eq!(back.sequence, save.sequence);
    assert_eq!(back.charms_owned, save.charms_owned);
    assert_eq!(back.charms_equipped, save.charms_equipped);
    assert_eq!(back.charm_notches, save.charm_notches);
    assert_eq!(back.can_overcharm, save.can_overcharm);
    assert_eq!(back.npc_conversations, save.npc_conversations);
    assert_eq!(back.script_fields, save.script_fields);
    assert_eq!(back.script_field_fnv, save.script_field_fnv);
    assert_eq!(back.shop_slots, save.shop_slots);
    assert_eq!(back.shop_counters, save.shop_counters);
    assert_eq!(back.player_bools, save.player_bools);
    assert_eq!(back.player_levels, save.player_levels);
    assert_eq!(back.version, 5);
    let s = back.shade;
    assert_eq!((s.present, s.scene, s.position, s.hp, s.geo_pool),
               (true, 2, [-131072, 65536], 10, 77));
    assert_eq!(Save::items(items).collect::<Vec<_>>(), store.items());
    let mut reloaded = persist::Store::new();
    for item in Save::items(items) {
        let (kind, scene, local, value) = persist::unpack(item).unwrap();
        assert!(reloaded.set(kind, scene, local, value));
    }
    assert_eq!(reloaded.items(), store.items());
    assert_eq!(reloaded.get(Kind::GeoRock, 3, 2), Some(0), "a depleted rock is an item, not an absence");
}

#[test]
fn an_empty_world_is_the_shortest_record_and_a_full_one_fits_the_buffer() {
    let empty = encode(&record(), &[]);
    assert_eq!(empty.len(), LEN);
    let mut full = persist::Store::new();
    for local in 0..persist::MAX_ITEMS {
        assert!(full.set(Kind::Breakable, 1, local, 1));
    }
    let bytes = encode(&record(), full.items());
    assert_eq!(bytes.len(), MAX_LEN);
    assert_eq!(Save::decode(&bytes).map(|(_, items)| items.len()), Some(persist::MAX_ITEMS * 4));
    // The psx-mc container ahead of the payload is 16 bytes, and a file owns
    // one 8,192-byte block whose first two frames are its title and icon.
    assert!(16 + MAX_LEN <= 8192 - 256);
}

#[test]
fn no_two_fields_share_an_offset() {
    // The round trip above passes even if two fields overlap, as long as the
    // last writer wins consistently. Changing one field at a time and watching
    // exactly one byte range move is what catches an overlap.
    let base = encode(&record(), &[]);
    let mut seen: Vec<(usize, usize)> = Vec::new();
    let mut mutate: Vec<(&str, Box<dyn Fn(&mut Save)>)> = Vec::new();
    mutate.push(("scene", Box::new(|s: &mut Save| s.scene ^= 0x5555_5555)));
    mutate.push(("seat", Box::new(|s: &mut Save| s.seat[0] ^= 0x5555_5555)));
    mutate.push(("region", Box::new(|s: &mut Save| s.region ^= 0x5555_5555)));
    mutate.push(("geo", Box::new(|s: &mut Save| s.geo ^= 0x5555_5555)));
    mutate.push(("door_hits", Box::new(|s: &mut Save| s.door_hits ^= 0x55)));
    mutate.push(("sequence", Box::new(|s: &mut Save| s.sequence ^= 0x5555_5555)));
    mutate.push(("charms_owned", Box::new(|s: &mut Save| s.charms_owned ^= 0x5555_5555_5555_5555)));
    mutate.push(("charms_equipped", Box::new(|s: &mut Save| s.charms_equipped ^= 0x5555_5555_5555_5555)));
    mutate.push(("charm_notches", Box::new(|s: &mut Save| s.charm_notches ^= 0x55)));
    mutate.push(("npc_conversations", Box::new(|s: &mut Save| s.npc_conversations ^= 0x5555_5555)));
    mutate.push(("script_field_fnv", Box::new(|s: &mut Save| s.script_field_fnv ^= 0x5555_5555)));
    mutate.push(("shop_slots", Box::new(|s: &mut Save| s.shop_slots ^= 0x0000_0555)));
    mutate.push(("player_bools", Box::new(|s: &mut Save| s.player_bools ^= 0x5555_5555)));
    for slot in 0..SCRIPT_FIELD_SLOTS {
        mutate.push(("script_field", Box::new(move |s: &mut Save| s.script_fields[slot] ^= 0x5555_5555)));
    }
    for slot in 0..SHOP_COUNTERS {
        mutate.push(("shop_counter", Box::new(move |s: &mut Save| s.shop_counters[slot] ^= 0x55)));
    }
    for slot in 0..persist::LEVELS {
        mutate.push(("player_level", Box::new(move |s: &mut Save| s.player_levels[slot] ^= 0x55)));
    }
    for (name, change) in mutate {
        let mut save = record();
        change(&mut save);
        let bytes = encode(&save, &[]);
        // The trailing checksum moves for every change, so it is not a span.
        let moved: Vec<usize> = (0..LEN - 4).filter(|&i| bytes[i] != base[i]).collect();
        assert!(!moved.is_empty(), "{name} changed no byte of the record");
        let span = (moved[0], moved[moved.len() - 1]);
        assert!(!seen.contains(&span), "{name} writes the same bytes as another field: {span:?}");
        seen.push(span);
    }
}

/// An HKS4 record as the previous guest wrote it: its 152 bytes of fields are
/// HKS5's first 152, then its checksum.
fn hks4(save: &Save) -> Vec<u8> {
    let mut bytes = encode(save, &[])[..LEN_HKS4].to_vec();
    bytes[0..4].copy_from_slice(b"HKS4");
    let sum = bytes[..LEN_HKS4 - 4].iter().fold(0x811c9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(16777619));
    bytes[LEN_HKS4 - 4..].copy_from_slice(&sum.to_le_bytes());
    bytes
}

#[test]
fn an_hks4_record_still_loads_with_the_authored_world() {
    // Every committed fixture and every card Manny has carries HKS4. Nothing
    // HKS5 adds was ever recorded by it, so the honest decode is its own fields
    // and an untouched world, which is what loading it did before.
    let save = record();
    let old = hks4(&save);
    let (back, items) = Save::decode(&old).expect("HKS4 must still load");
    assert_eq!(back.version, 4);
    assert!(items.is_empty());
    assert_eq!((back.player_bools, back.player_levels), (0, [0; persist::LEVELS]));
    assert_eq!((back.scene, back.geo, back.door_hits, back.sequence, back.shop_counters, back.script_fields),
               (save.scene, save.geo, save.door_hits, save.sequence, save.shop_counters, save.script_fields));
    // And it is still checksummed like one.
    let mut corrupt = hks4(&save);
    corrupt[70] ^= 1;
    assert!(Save::decode(&corrupt).is_none());
}

#[test]
fn a_shorter_or_differently_tagged_record_is_refused() {
    // HKS1 to HKS3 never load: their fields would half-decode into invented
    // ones. tools/migrate_cards.py is what carries a fixture forward.
    let store = world();
    let bytes = encode(&record(), store.items());
    assert!(Save::decode(&bytes[..bytes.len() - 1]).is_none(), "a truncated record must not decode");
    assert!(Save::decode(&bytes[..LEN - 1]).is_none());
    for older in [*b"HKS1", *b"HKS2", *b"HKS3"] {
        let mut stale = bytes.clone();
        stale[0..4].copy_from_slice(&older);
        assert!(Save::decode(&stale).is_none(), "an older tag must not decode");
    }
}

#[test]
fn a_single_flipped_bit_anywhere_is_refused() {
    // The checksum covers the whole record, so corruption in the newest fields
    // has to be caught as surely as corruption in the oldest.
    let store = world();
    let bytes = encode(&record(), store.items());
    let len = bytes.len();
    for byte in [0usize, 24, 52, 70, 74, 151, 152, 156, 160, 162, len - 9, len - 5] {
        let mut corrupt = bytes.clone();
        corrupt[byte] ^= 1;
        assert!(Save::decode(&corrupt).is_none(), "a flipped bit at {byte} must not decode");
    }
}

#[test]
fn a_well_checksummed_list_no_build_wrote_is_refused() {
    // A checksum proves the bytes arrived; it cannot prove a writer made them.
    // Unknown kinds, duplicates and disorder would each install something the
    // store's own invariants forbid, so they are corruption too.
    let fix = |bytes: &mut Vec<u8>| {
        let n = bytes.len();
        let sum = bytes[..n - 4].iter().fold(0x811c9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(16777619));
        bytes[n - 4..].copy_from_slice(&sum.to_le_bytes());
    };
    let a = persist::pack(Kind::Breakable, 0, 1, 1).unwrap();
    let b = persist::pack(Kind::Breakable, 0, 2, 1).unwrap();
    for items in [vec![b, a], vec![a, a | 2], vec![0xC000_0000]] {
        let mut bytes = encode(&record(), &[a, b]);
        for (i, item) in items.iter().enumerate() {
            bytes[LEN - 4 + i * 4..LEN + i * 4].copy_from_slice(&item.to_le_bytes());
        }
        let count = items.len() as u16;
        bytes[LEN - 6..LEN - 4].copy_from_slice(&count.to_le_bytes());
        bytes.truncate(LEN + items.len() * 4);
        fix(&mut bytes);
        assert!(Save::decode(&bytes).is_none(), "{items:x?} must not decode");
    }
    let mut over = encode(&record(), &[]);
    over[LEN - 6..LEN - 4].copy_from_slice(&((persist::MAX_ITEMS + 1) as u16).to_le_bytes());
    fix(&mut over);
    assert!(Save::decode(&over).is_none(), "a count past the store must not decode");
}

#[test]
fn the_length_is_exactly_what_the_fields_need() {
    // A reserve that drifts from the record length is how a slot ends up
    // overlapping the checksum. 74 bytes of fields before the reserve, then the
    // reserve, then the identity word, then the shop bools and their counters,
    // which is where HKS4 ended; then the PlayerData bools and levels, the item
    // count, and the checksum.
    assert_eq!(LEN_HKS4, 74 + SCRIPT_FIELD_SLOTS * 4 + 4 + 4 + SHOP_COUNTERS + 4);
    assert_eq!(LEN, LEN_HKS4 - 4 + 4 + persist::LEVELS + 2 + 4);
    assert_eq!(MAX_LEN, LEN + persist::MAX_ITEMS * 4);
}
