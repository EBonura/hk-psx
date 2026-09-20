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
//! a shop purchase survive a quit.
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

#[path = "../game/src/save.rs"]
pub mod save;

use save::{Save, LEN, SCRIPT_FIELD_SLOTS, SHOP_COUNTERS};

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
    }
}

#[test]
fn every_field_survives_the_round_trip() {
    // The encode and decode offsets are hand-maintained and were extended twice
    // in one day. A field written to the wrong offset reads back as a different
    // field's value, and the record checksum cannot see it: the bytes are
    // intact, they are just in the wrong places.
    let save = record();
    let bytes = save.encode();
    assert_eq!(bytes.len(), LEN);
    let back = Save::decode(&bytes).expect("a record this module just wrote must decode");
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
    let s = back.shade;
    assert_eq!((s.present, s.scene, s.position, s.hp, s.geo_pool),
               (true, 2, [-131072, 65536], 10, 77));
}

#[test]
fn no_two_fields_share_an_offset() {
    // The round trip above passes even if two fields overlap, as long as the
    // last writer wins consistently. Changing one field at a time and watching
    // exactly one byte range move is what catches an overlap.
    let base = record().encode();
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
    for slot in 0..SCRIPT_FIELD_SLOTS {
        mutate.push(("script_field", Box::new(move |s: &mut Save| s.script_fields[slot] ^= 0x5555_5555)));
    }
    for slot in 0..SHOP_COUNTERS {
        mutate.push(("shop_counter", Box::new(move |s: &mut Save| s.shop_counters[slot] ^= 0x55)));
    }
    for (name, change) in mutate {
        let mut save = record();
        change(&mut save);
        let bytes = save.encode();
        // The trailing checksum moves for every change, so it is not a span.
        let moved: Vec<usize> = (0..LEN - 4).filter(|&i| bytes[i] != base[i]).collect();
        assert!(!moved.is_empty(), "{name} changed no byte of the record");
        let span = (moved[0], moved[moved.len() - 1]);
        assert!(!seen.contains(&span), "{name} writes the same bytes as another field: {span:?}");
        seen.push(span);
    }
}

#[test]
fn a_shorter_or_differently_tagged_record_is_refused() {
    // Every committed card fixture was written by an older guest, so this is
    // the behaviour that invalidates them, and it has to be a refusal rather
    // than a partial decode into invented fields. tools/migrate_cards.py is
    // what carries the fixtures forward.
    let bytes = record().encode();
    assert!(Save::decode(&bytes[..LEN - 1]).is_none(), "a truncated record must not decode");
    for older in [*b"HKS1", *b"HKS2", *b"HKS3"] {
        let mut stale = bytes;
        stale[0..4].copy_from_slice(&older);
        assert!(Save::decode(&stale).is_none(), "an older tag must not decode");
    }
}

#[test]
fn a_single_flipped_bit_anywhere_is_refused() {
    // The checksum covers the whole record, so corruption in the newest fields
    // has to be caught as surely as corruption in the oldest.
    let bytes = record().encode();
    for byte in [0usize, 24, 52, 70, 74, LEN - 19, LEN - 14, LEN - 9, LEN - 5] {
        let mut corrupt = bytes;
        corrupt[byte] ^= 1;
        assert!(Save::decode(&corrupt).is_none(), "a flipped bit at {byte} must not decode");
    }
}

#[test]
fn the_length_is_exactly_what_the_fields_need() {
    // A reserve that drifts from the record length is how a slot ends up
    // overlapping the checksum. 74 bytes of fields before the reserve, then the
    // reserve, then the identity word, then the shop bools and their counters,
    // then the checksum.
    assert_eq!(LEN, 74 + SCRIPT_FIELD_SLOTS * 4 + 4 + 4 + SHOP_COUNTERS + 4);
}
