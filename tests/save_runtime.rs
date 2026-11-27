//! The real save record, compiled and executed.
//!
//! `game/src/save.rs` is part of a `no_std` PSX binary and pulls in `psx_mc`
//! for the card I/O, so no cargo target compiles it; this harness does, against
//! stubs. The record is the only thing standing between a player and their save,
//! and every growth of it has to keep loading what the previous guests wrote.
//!
//! Only `encode`/`decode`/`migrate` are exercised here: they are pure functions
//! of the record and never touch the card. HKS6 holds PlayerData as named fields
//! (`game/src/player_data.rs`) instead of six stores at fixed offsets, so the
//! tests that matter most are the ones that load every committed card fixture
//! twice, through the HKS5 guest's own reader (vendored below, unchanged, as the
//! oracle) and through HKS6's migration, and require the same state.
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
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TransportFault { None, TxTimeout, RxTimeout, AckTimeout, AckReleaseTimeout }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TransportTrace { pub exchanges: u16, pub acknowledgements: u16, pub fault: TransportFault,
    pub fault_exchange: u16, pub response_prefix: [u8; 10] }
pub struct SaveIcon { pub palette: [u16; 16], pub pixels: [u8; FRAME_SIZE] }
impl SaveIcon {
    pub const fn new(palette: [u16; 16], pixels: [u8; FRAME_SIZE]) -> Self { Self { palette, pixels } }
}
pub struct HardwareCard;
impl HardwareCard {
    pub fn new(_: Slot) -> Self { panic!("card I/O is not exercised by this harness") }
    pub fn last_trace(&self) -> TransportTrace { unreachable!() }
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
    pub fn delete(&mut self, _: &str) -> Result<()> { unreachable!() }
    pub fn write(&mut self, _: &str, _: &str, _: &[u8]) -> Result<()> { unreachable!() }
    pub fn write_with_icon(&mut self, _: &str, _: &str, _: &[u8], _: &SaveIcon) -> Result<()> { unreachable!() }
    pub fn read(&mut self, _: &str, _: &mut [u8]) -> Result<usize> { unreachable!() }
}

pub mod shade {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct Record { pub present: bool, pub scene: u32, pub position: [i32; 2], pub hp: u16, pub geo_pool: u32 }
}
pub mod input {
    pub fn checkpoint() {}
    pub fn vblanks() -> u32 { 0 }
    pub fn wait_vblanks(_: u32) {}
}

#[path = "../game/src/persist.rs"]
pub mod persist;
#[path = "../game/src/player_data.rs"]
pub mod player_data;
#[path = "../game/src/save.rs"]
pub mod save;
#[path = "../game/src/save_ui.rs"]
pub mod save_ui;

/// The HKS5 guest's record (game/src/save.rs at 5d7d28be), unchanged apart from
/// its comments: the reader every committed fixture was written for, kept here
/// as the oracle the migration is held to.
mod old {
    use crate::persist;
    use crate::save::{SCRIPT_FIELD_SLOTS, SHOP_COUNTERS};
    const MAGIC: [u8; 4] = *b"HKS5";
    const MAGIC_HKS4: [u8; 4] = *b"HKS4";
    pub const LEN_HKS4: usize = 156;
    const HKS4_FIELDS: usize = LEN_HKS4 - 4;
    const ITEMS_AT: usize = HKS4_FIELDS + 4 + persist::LEVELS + 2;
    pub const LEN: usize = 166;
    pub const MAX_LEN: usize = LEN + persist::MAX_ITEMS * 4;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Save {
        pub scene: u32,
        pub seat: [i32; 2],
        pub facing: i32,
        pub region: u32,
        pub geo: u32,
        pub door_hits: u8,
        pub sequence: u32,
        pub shade: crate::shade::Record,
        pub charms_owned: psx_math::Mask64,
        pub charms_equipped: psx_math::Mask64,
        pub charm_notches: u8,
        pub can_overcharm: bool,
        pub npc_conversations: u32,
        pub script_fields: [i32; SCRIPT_FIELD_SLOTS],
        pub script_field_fnv: u32,
        pub shop_slots: u32,
        pub shop_counters: [u8; SHOP_COUNTERS],
        pub player_bools: u32,
        pub player_levels: [u8; persist::LEVELS],
        pub version: u8,
    }
    fn fnv(bytes: &[u8]) -> u32 {
        psx_pack::fnv1a32(bytes)
    }
    impl Save {
        pub fn encode(&self, items: &[u32], out: &mut [u8]) -> usize {
            assert!(items.len() <= persist::MAX_ITEMS && out.len() >= MAX_LEN);
            let len = ITEMS_AT + items.len() * 4 + 4;
            let out = &mut out[..len];
            out.fill(0);
            out[0..4].copy_from_slice(&MAGIC);
            out[4..8].copy_from_slice(&self.scene.to_le_bytes());
            out[8..12].copy_from_slice(&self.seat[0].to_le_bytes());
            out[12..16].copy_from_slice(&self.seat[1].to_le_bytes());
            out[16..20].copy_from_slice(&self.facing.to_le_bytes());
            out[20..24].copy_from_slice(&self.region.to_le_bytes());
            out[24..28].copy_from_slice(&self.geo.to_le_bytes());
            out[28] = self.door_hits;
            out[29] = u8::from(self.shade.present);
            out[30..32].copy_from_slice(&self.shade.hp.to_le_bytes());
            out[32..36].copy_from_slice(&self.shade.geo_pool.to_le_bytes());
            out[36..40].copy_from_slice(&self.shade.scene.to_le_bytes());
            out[40..44].copy_from_slice(&self.shade.position[0].to_le_bytes());
            out[44..48].copy_from_slice(&self.shade.position[1].to_le_bytes());
            out[48..52].copy_from_slice(&self.sequence.to_le_bytes());
            out[52..56].copy_from_slice(&self.charms_owned.low().to_le_bytes());
            out[56..60].copy_from_slice(&self.charms_owned.high().to_le_bytes());
            out[60..64].copy_from_slice(&self.charms_equipped.low().to_le_bytes());
            out[64..68].copy_from_slice(&self.charms_equipped.high().to_le_bytes());
            out[68] = self.charm_notches;
            out[69] = u8::from(self.can_overcharm);
            out[70..74].copy_from_slice(&self.npc_conversations.to_le_bytes());
            for (slot, value) in self.script_fields.iter().enumerate() {
                out[74 + slot * 4..78 + slot * 4].copy_from_slice(&value.to_le_bytes());
            }
            let after = 74 + SCRIPT_FIELD_SLOTS * 4;
            out[after..after + 4].copy_from_slice(&self.script_field_fnv.to_le_bytes());
            out[after + 4..after + 8].copy_from_slice(&self.shop_slots.to_le_bytes());
            out[after + 8..after + 8 + SHOP_COUNTERS].copy_from_slice(&self.shop_counters);
            out[HKS4_FIELDS..HKS4_FIELDS + 4].copy_from_slice(&self.player_bools.to_le_bytes());
            out[HKS4_FIELDS + 4..HKS4_FIELDS + 4 + persist::LEVELS]
                .copy_from_slice(&self.player_levels);
            out[ITEMS_AT - 2..ITEMS_AT].copy_from_slice(&(items.len() as u16).to_le_bytes());
            for (i, item) in items.iter().enumerate() {
                out[ITEMS_AT + i * 4..ITEMS_AT + i * 4 + 4].copy_from_slice(&item.to_le_bytes());
            }
            let sum = fnv(&out[..len - 4]);
            out[len - 4..].copy_from_slice(&sum.to_le_bytes());
            len
        }
        #[cfg_attr(not(test), optimize(size))]
        pub fn decode(bytes: &[u8]) -> Option<(Self, &[u8])> {
            let (len, version) = if bytes.get(0..4)? == MAGIC_HKS4 {
                (LEN_HKS4, 4)
            } else if bytes.get(0..4)? == MAGIC && bytes.len() >= LEN {
                let count = u16::from_le_bytes([bytes[ITEMS_AT - 2], bytes[ITEMS_AT - 1]]) as usize;
                if count > persist::MAX_ITEMS {
                    return None;
                }
                (ITEMS_AT + count * 4 + 4, 5)
            } else {
                return None;
            };
            if bytes.len() < len
                || fnv(&bytes[..len - 4]) != u32::from_le_bytes(bytes[len - 4..len].try_into().ok()?)
            {
                return None;
            }
            let word = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            let mask = |at: usize| {
                psx_math::Mask64::from_words(
                    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
                    u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()),
                )
            };
            let items: &[u8] = if version == 5 {
                &bytes[ITEMS_AT..len - 4]
            } else {
                &[]
            };
            let mut last = None;
            for item in Self::items(items) {
                persist::unpack(item)?;
                if last.is_some_and(|last| item >> 8 <= last) {
                    return None;
                }
                last = Some(item >> 8);
            }
            Some((
                Self {
                    scene: word(4) as u32,
                    seat: [word(8), word(12)],
                    facing: word(16).signum().max(-1),
                    region: word(20) as u32,
                    geo: word(24) as u32,
                    door_hits: bytes[28],
                    shade: crate::shade::Record {
                        present: bytes[29] != 0,
                        hp: u16::from_le_bytes([bytes[30], bytes[31]]),
                        geo_pool: word(32) as u32,
                        scene: word(36) as u32,
                        position: [word(40), word(44)],
                    },
                    sequence: word(48) as u32,
                    charms_owned: mask(52),
                    charms_equipped: mask(60),
                    charm_notches: bytes[68],
                    can_overcharm: bytes[69] != 0,
                    npc_conversations: u32::from_le_bytes(bytes[70..74].try_into().unwrap()),
                    script_fields: core::array::from_fn(|slot| word(74 + slot * 4)),
                    script_field_fnv: word(74 + SCRIPT_FIELD_SLOTS * 4) as u32,
                    shop_slots: word(78 + SCRIPT_FIELD_SLOTS * 4) as u32,
                    shop_counters: core::array::from_fn(|i| bytes[82 + SCRIPT_FIELD_SLOTS * 4 + i]),
                    player_bools: if version == 5 {
                        word(HKS4_FIELDS) as u32
                    } else {
                        0
                    },
                    player_levels: if version == 5 {
                        core::array::from_fn(|i| bytes[HKS4_FIELDS + 4 + i])
                    } else {
                        [0; persist::LEVELS]
                    },
                    version,
                },
                items,
            ))
        }
        pub fn items(raw: &[u8]) -> impl Iterator<Item = u32> + '_ {
            raw.chunks_exact(4)
                .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        }
    }
}

use persist::Kind;
use player_data::{PlayerData, COUNT};
use save::{Build, Restored, Save, LEN, LEN_HKS4, LEN_HKS5, MAX_LEN, SCRIPT_FIELD_SLOTS, SHOP_COUNTERS};

fn record() -> Save {
    Save {
        scene: 3, seat: [-65536, 720896], facing: -1, region: 7, geo: 41, door_hits: 13,
        shade: shade::Record { present: true, scene: 2, position: [-131072, 65536], hp: 10, geo_pool: 77 },
        sequence: 9,
        version: 6,
    }
}
/// Every field a build writes off its default, each to its own value. The
/// retired ids stay zero: no build writes them, and a load folds them away.
fn everything() -> PlayerData {
    let mut pd = PlayerData::new();
    for id in 0..COUNT {
        if !player_data::NAMES[id].starts_with("port.retired") {
            pd.set(id, (id as i32 + 1) * -7 + 3);
        }
    }
    pd
}
fn live_fields() -> usize {
    player_data::NAMES.iter().filter(|n| !n.starts_with("port.retired")).count()
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
fn encode(save: &Save, pd: &PlayerData, items: &[u32]) -> Vec<u8> {
    let mut out = [0u8; MAX_LEN];
    let len = save.encode(pd, items, &mut out);
    out[..len].to_vec()
}
fn fix(bytes: &mut Vec<u8>) {
    let n = bytes.len();
    let sum = bytes[..n - 4].iter().fold(0x811c9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(16777619));
    bytes[n - 4..].copy_from_slice(&sum.to_le_bytes());
}

// ---------------------------------------------------------------------------
// The running build, as the guest passes it to `save::migrate`.
// ---------------------------------------------------------------------------

/// `SCRIPT_FIELD_NAMES` and `SCRIPT_FIELD_FNV` out of the cooked bank, which is
/// generated (data/ is not committed), so it is read rather than repeated.
fn cooked_bank() -> (Vec<String>, u32) {
    let root = std::path::Path::new(file!()).parent().unwrap().parent().unwrap().to_path_buf();
    let text = std::fs::read_to_string(root.join("data/scripts.rs"))
        .expect("data/scripts.rs: cook the scripts before running this harness");
    let fnv = text.split("SCRIPT_FIELD_FNV:u32=").nth(1).unwrap().split(';').next().unwrap().parse().unwrap();
    let names = text.split("SCRIPT_FIELD_NAMES:&[&str]=&[").nth(1).unwrap().split(']').next().unwrap();
    (names.split(',').map(|n| n.trim_matches('"').to_string()).collect(), fnv)
}
fn shop_flag_names() -> Vec<String> {
    let root = std::path::Path::new(file!()).parent().unwrap().parent().unwrap().to_path_buf();
    let text = std::fs::read_to_string(root.join("data/shop.rs")).expect("data/shop.rs");
    let names = text.split("FLAG_NAMES:[&str;FLAG_COUNT]=[").nth(1).unwrap().split(']').next().unwrap();
    names.split(',').map(|n| n.trim_matches('"').to_string()).collect()
}
const FALSE_KNIGHT_SCENE: usize = 11;
const STARTING_MASKS: u8 = 5;
const SOUL_PER_VESSEL: u16 = 33;
/// `title_card::AREA_IDS` out of the generated table (host/title_cards.py).
fn area_ids() -> Vec<u8> {
    let root = std::path::Path::new(file!()).parent().unwrap().parent().unwrap().to_path_buf();
    let text = std::fs::read_to_string(root.join("data/title_cards.rs")).expect("data/title_cards.rs");
    let ids = text.split("pub const AREA_IDS:").nth(1).expect("AREA_IDS: run host/title_cards.py")
        .split("=[").nth(1).unwrap().split(']').next().unwrap();
    ids.split(',').map(|n| n.parse().unwrap()).collect()
}
struct Bank { fields: Vec<usize>, names: Vec<String>, fnv: u32, area_ids: Vec<u8> }
fn bank() -> Bank {
    let (names, fnv) = cooked_bank();
    let fields = names.iter().map(|n| player_data::find(n)).collect::<Vec<_>>();
    assert!(fields.iter().all(|&f| f < COUNT), "a cooked script field has no player_data row: {names:?}");
    Bank { fields, names, fnv, area_ids: area_ids() }
}
fn build(bank: &Bank) -> Build<'_> {
    Build {
        script_fields: &bank.fields,
        script_fnv: bank.fnv,
        false_knight_scene: Some(FALSE_KNIGHT_SCENE),
        starting_masks: STARTING_MASKS,
        soul_per_vessel: SOUL_PER_VESSEL,
        area_ids: &bank.area_ids,
    }
}
/// A record through the HKS6 loader: decode, then migrate into fresh stores.
fn load(bytes: &[u8], bank: &Bank) -> Option<(Save, PlayerData, persist::Store, Restored)> {
    let (save, player, items) = Save::decode(bytes)?;
    let (mut pd, mut store) = (PlayerData::new(), persist::Store::new());
    let restored = save::migrate(&save, player, items, &build(bank), &mut pd, &mut store);
    Some((save, pd, store, restored))
}

#[test]
fn the_bank_and_the_shop_name_fields_the_store_has_in_the_order_it_expects() {
    // `script::FIELD_MAP` binds by name at compile time in the guest; this is
    // the same binding, and the shop's slots are bound by position, so their
    // names are checked against the cook's here.
    let _ = bank();
    let names: Vec<_> = player_data::SHOP_FLAGS.iter().map(|&f| player_data::NAMES[f].to_string()).collect();
    assert_eq!(names, shop_flag_names());
}

#[test]
fn the_names_are_unique_and_the_port_s_own_cannot_match_a_source_one() {
    for (i, a) in player_data::NAMES.iter().enumerate() {
        assert_eq!(player_data::find(a), i, "{a} is listed twice");
        assert!(!a.is_empty() && !a.contains(' '));
    }
    assert_eq!(player_data::find("noSuchField"), COUNT);
    assert_eq!(player_data::NAMES[player_data::SHAMAN], "shaman");
    assert_eq!(player_data::NAMES[player_data::FIREBALL_LEVEL], "fireballLevel");
    assert_eq!(player_data::NAMES[player_data::MAX_HEALTH_BASE], "maxHealthBase");
    assert_eq!(player_data::NAMES[player_data::PORT_NPC_CURSORS], "port.npcCursors");
}

// ---------------------------------------------------------------------------
// HKS6 itself.
// ---------------------------------------------------------------------------

#[test]
fn every_field_survives_the_round_trip() {
    // A header field written to the wrong offset reads back as a different
    // field's value, and the record checksum cannot see it.
    let save = record();
    let pd = everything();
    let store = world();
    let bytes = encode(&save, &pd, store.items());
    assert_eq!(bytes.len(), LEN + live_fields() * 6 + store.items().len() * 4);
    let (back, loaded, items, restored) = load(&bytes, &bank()).expect("a record this module just wrote must decode");
    assert_eq!(back.scene, save.scene);
    assert_eq!(back.seat, save.seat);
    assert_eq!(back.facing, save.facing);
    assert_eq!(back.region, save.region);
    assert_eq!(back.geo, save.geo);
    assert_eq!(back.door_hits, save.door_hits);
    assert_eq!(back.sequence, save.sequence);
    assert_eq!(back.version, 6);
    let s = back.shade;
    assert_eq!((s.present, s.scene, s.position, s.hp, s.geo_pool), (true, 2, [-131072, 65536], 10, 77));
    assert_eq!(loaded, pd);
    assert_eq!(items.items(), store.items());
    assert_eq!(restored, Restored { items: store.items().len(), scripts: true });
    assert_eq!(items.get(Kind::GeoRock, 3, 2), Some(0), "a depleted rock is an item, not an absence");
}

#[test]
fn a_new_game_is_the_shortest_record_and_the_longest_fits_the_buffer() {
    let empty = encode(&record(), &PlayerData::new(), &[]);
    assert_eq!(empty.len(), LEN, "a field at its default is not written");
    let mut full = persist::Store::new();
    for local in 0..persist::MAX_ITEMS {
        assert!(full.set(Kind::Breakable, 1, local, 1));
    }
    let bytes = encode(&record(), &everything(), full.items());
    assert_eq!(bytes.len(), MAX_LEN - (COUNT - live_fields()) * 6);
    let (_, pd, store, _) = load(&bytes, &bank()).unwrap();
    assert_eq!((pd, store.items().len()), (everything(), persist::MAX_ITEMS));
    // The longest any build could have written: an HKS6 record from before the
    // merge, with the retired ids set too. It fits, and loads folded.
    let mut all = everything();
    for id in 0..COUNT {
        all.set(id, (id as i32 + 1) * -7 + 3);
    }
    let bytes = encode(&record(), &all, full.items());
    assert_eq!(bytes.len(), MAX_LEN);
    let (_, pd, _, _) = load(&bytes, &bank()).unwrap();
    assert_eq!(pd.get(player_data::find("port.retired.openedMapperShop")), 0);
    assert_eq!(pd.get(player_data::find("port.retired.areaTitle")), 0);
    // The psx-mc container ahead of the payload is 16 bytes, and a file owns
    // one 8,192-byte block whose first two frames are its title and icon.
    assert!(16 + MAX_LEN <= 8192 - 256);
}

#[test]
fn a_field_the_record_does_not_mention_reads_as_its_default_and_no_other_moves() {
    // The property HKS6 exists for. A record written before a field joined the
    // table simply lacks its entry; drop each entry in turn and every other
    // field must come back exactly as written, which the script reserve's
    // identity check could never do (one new field reset all of them).
    let pd = everything();
    let bytes = encode(&record(), &pd, &[]);
    let live = live_fields();
    for entry in 0..live {
        let mut older = bytes.clone();
        let at = 54 + entry * 6;
        let id = u16::from_le_bytes([older[at], older[at + 1]]) as usize;
        older.drain(at..at + 6);
        let count = (live - 1) as u16;
        older[52..54].copy_from_slice(&count.to_le_bytes());
        fix(&mut older);
        let (_, back, _, _) = load(&older, &bank()).expect("a record missing a field still loads");
        for other in 0..COUNT {
            let want = if other == id { player_data::default(id) } else { pd.get(other) };
            assert_eq!(back.get(other), want, "dropping {} moved {}", player_data::NAMES[id], player_data::NAMES[other]);
        }
    }
}

#[test]
fn a_section_no_build_could_write_is_refused() {
    // Unknown ids (a newer build's fields: a save at the next bench would lose
    // them for good), repeats, disorder and a count past the table.
    let mut pd = PlayerData::new();
    pd.set(2, 1);
    pd.set(40, 1);
    let good = encode(&record(), &pd, &[]);
    assert!(Save::decode(&good).is_some());
    let entry = |bytes: &mut Vec<u8>, i: usize, id: u16| bytes[54 + i * 6..56 + i * 6].copy_from_slice(&id.to_le_bytes());
    for (i, id) in [(1, COUNT as u16), (1, 2), (0, 41), (1, u16::MAX)] {
        let mut bad = good.clone();
        entry(&mut bad, i, id);
        fix(&mut bad);
        assert!(Save::decode(&bad).is_none(), "entry {i} as id {id} must not decode");
    }
    let mut over = good.clone();
    over[52..54].copy_from_slice(&((COUNT + 1) as u16).to_le_bytes());
    fix(&mut over);
    assert!(Save::decode(&over).is_none());
    let mut short = good.clone();
    short[52..54].copy_from_slice(&3u16.to_le_bytes());
    fix(&mut short);
    assert!(Save::decode(&short).is_none(), "a count that runs into the item list must not decode");
}

#[test]
fn capacity_160_preserves_every_item_on_reload_and_kind_reset() {
    assert_eq!(persist::MAX_ITEMS, 192);
    let mut full = persist::Store::new();
    for local in 0..80 {
        assert!(full.set(Kind::Breakable, 1, local, 1));
        assert!(full.set(Kind::SoulTotem, 2, local, 0));
    }
    assert_eq!(full.items().len(), 160);
    let bytes = encode(&record(), &PlayerData::new(), full.items());
    assert_eq!(bytes.len(), LEN + 640);
    let (_, _, mut reload, restored) = load(&bytes, &bank()).unwrap();
    assert_eq!(restored.items, 160);
    assert_eq!(reload.items(), full.items());
    reload.clear_kind(Kind::SoulTotem);
    assert_eq!(reload.items().len(), 80);
    for local in 0..80 {
        assert_eq!(reload.get(Kind::Breakable, 1, local), Some(1));
        assert_eq!(reload.get(Kind::SoulTotem, 2, local), None);
    }
    assert_eq!(reload.overflow, 0);
}

#[test]
fn post_rest_kill_and_combined_185_item_record_survive_cold_load_then_only_semi_reset() {
    let mut world = persist::Store::new();
    // The current-source authored fixture's 138 non-actor keys, including nine
    // totems, plus 45 reset-lifetime actor states, one permanent actor state
    // and the Crossroads_22 arena completion key.
    for local in 0..129 { assert!(world.set(Kind::Breakable, 1, local, 1)); }
    for local in 0..9 { assert!(world.set(Kind::SoulTotem, 2, local, 0)); }
    for local in 16..61 { assert!(world.set(Kind::Enemy, 3, local, 2)); }
    assert!(world.set(Kind::Enemy, 3, 61, 1));
    assert!(world.set(Kind::BattleScene, 21, 0, 1));
    assert_eq!(world.items().len(), 185);
    let bytes = encode(&record(), &everything(), world.items());
    assert_eq!(&bytes[..4], b"HKS6");
    assert!(16 + bytes.len() <= 8192 - 256);
    let (_, _, mut cold, _) = load(&bytes, &bank()).unwrap();
    assert_eq!(cold.items(), world.items());
    cold.clear_semi_actor_deaths();
    assert_eq!(cold.items().len(), 140);
    assert_eq!(cold.get(Kind::BattleScene, 21, 0), Some(1));
    assert_eq!(cold.get(Kind::Enemy,3,61), Some(1));
    // Rest happened before a projectile killed another enemy while Save was
    // pending. Confirming Save must capture it without a second reset.
    assert!(cold.set(Kind::Enemy,3,16,2));
    let post_rest = encode(&record(), &PlayerData::new(), cold.items());
    let (_, _, mut loaded, _) = load(&post_rest, &bank()).unwrap();
    assert_eq!(loaded.get(Kind::Enemy,3,16), Some(2));
    loaded.clear_semi_actor_deaths();
    assert_eq!(loaded.get(Kind::Enemy,3,16), None);
    assert_eq!(loaded.get(Kind::Enemy,3,61), Some(1));
    assert_eq!(loaded.get(Kind::SoulTotem,2,0), Some(0));
    assert_eq!(loaded.overflow,0);
}

#[test]
fn a_rest_or_a_death_refills_the_totems_and_returns_only_the_semi_persistent_enemies() {
    // `GameManager.PlayerDead` and the bench both call `ResetSemiPersistentItems`.
    let mut s = persist::Store::new();
    // A Blocker's death, a permanent enemy death and two a reset forgets, two
    // soul totems, a broken wall and a won arena.
    assert!(s.set(Kind::Enemy, 2, 1, 1));
    assert!(s.set(Kind::Enemy, 2, 16, 1));
    assert!(s.set(Kind::Enemy, 2, 17, 2));
    assert!(s.set(Kind::Enemy, 6, 18, 2));
    assert!(s.set(Kind::SoulTotem, 2, 0, 3));
    assert!(s.set(Kind::SoulTotem, 44, 1, 4));
    assert!(s.set(Kind::Breakable, 2, 9, 1));
    assert!(s.set(Kind::BattleScene, 21, 0, 1));
    s.reset_semi_persistent();
    assert_eq!(s.all(Kind::Enemy).collect::<Vec<_>>(), [(2, 1, 1), (2, 16, 1)]);
    assert_eq!(s.count(Kind::SoulTotem), 0, "the totems refill");
    assert_eq!(s.get(Kind::Breakable, 2, 9), Some(1));
    assert_eq!(s.get(Kind::BattleScene, 21, 0), Some(1));
    assert_eq!(s.items().len(), 4);
}

#[test]
fn no_two_header_fields_share_an_offset() {
    // The round trip passes even if two fields overlap, as long as the last
    // writer wins consistently. Changing one field at a time and watching
    // exactly one byte range move is what catches an overlap.
    let pd = PlayerData::new();
    let base = encode(&record(), &pd, &[]);
    let mut seen: Vec<(usize, usize)> = Vec::new();
    let mut mutate: Vec<(&str, Box<dyn Fn(&mut Save)>)> = Vec::new();
    mutate.push(("scene", Box::new(|s: &mut Save| s.scene ^= 0x5555_5555)));
    mutate.push(("seat", Box::new(|s: &mut Save| s.seat[0] ^= 0x5555_5555)));
    mutate.push(("region", Box::new(|s: &mut Save| s.region ^= 0x5555_5555)));
    mutate.push(("geo", Box::new(|s: &mut Save| s.geo ^= 0x5555_5555)));
    mutate.push(("door_hits", Box::new(|s: &mut Save| s.door_hits ^= 0x55)));
    mutate.push(("sequence", Box::new(|s: &mut Save| s.sequence ^= 0x5555_5555)));
    mutate.push(("shade.hp", Box::new(|s: &mut Save| s.shade.hp ^= 0x5555)));
    mutate.push(("shade.geo_pool", Box::new(|s: &mut Save| s.shade.geo_pool ^= 0x5555_5555)));
    mutate.push(("shade.scene", Box::new(|s: &mut Save| s.shade.scene ^= 0x5555_5555)));
    mutate.push(("shade.position", Box::new(|s: &mut Save| s.shade.position[1] ^= 0x5555_5555)));
    for (name, change) in mutate {
        let mut save = record();
        change(&mut save);
        let bytes = encode(&save, &pd, &[]);
        let moved: Vec<usize> = (0..LEN - 4).filter(|&i| bytes[i] != base[i]).collect();
        assert!(!moved.is_empty(), "{name} changed no byte of the record");
        let span = (moved[0], moved[moved.len() - 1]);
        assert!(span.1 < 52, "{name} lands outside the header");
        assert!(!seen.contains(&span), "{name} writes the same bytes as another field: {span:?}");
        seen.push(span);
    }
}

#[test]
fn a_shorter_or_differently_tagged_record_is_refused() {
    let store = world();
    let bytes = encode(&record(), &everything(), store.items());
    assert!(Save::decode(&bytes[..bytes.len() - 1]).is_none(), "a truncated record must not decode");
    assert!(Save::decode(&bytes[..LEN - 1]).is_none());
    for older in [*b"HKS1", *b"HKS2", *b"HKS3", *b"HKS7"] {
        let mut stale = bytes.clone();
        stale[0..4].copy_from_slice(&older);
        assert!(Save::decode(&stale).is_none(), "an unknown tag must not decode");
    }
}

#[test]
fn a_single_flipped_bit_anywhere_is_refused() {
    let store = world();
    let bytes = encode(&record(), &everything(), store.items());
    let len = bytes.len();
    for byte in [0usize, 24, 51, 52, 54, 57, 60, 200, 400, len - 9, len - 5] {
        let mut corrupt = bytes.clone();
        corrupt[byte] ^= 1;
        assert!(Save::decode(&corrupt).is_none(), "a flipped bit at {byte} must not decode");
    }
}

#[test]
fn a_well_checksummed_list_no_build_wrote_is_refused() {
    let a = persist::pack(Kind::Breakable, 0, 1, 1).unwrap();
    let b = persist::pack(Kind::Breakable, 0, 2, 1).unwrap();
    for items in [vec![b, a], vec![a, a | 2], vec![0xC000_0000]] {
        let mut bytes = encode(&record(), &PlayerData::new(), &[a, b]);
        for (i, item) in items.iter().enumerate() {
            bytes[LEN - 4 + i * 4..LEN + i * 4].copy_from_slice(&item.to_le_bytes());
        }
        let count = items.len() as u16;
        bytes[LEN - 6..LEN - 4].copy_from_slice(&count.to_le_bytes());
        bytes.truncate(LEN + items.len() * 4);
        fix(&mut bytes);
        assert!(Save::decode(&bytes).is_none(), "{items:x?} must not decode");
    }
    let mut over = encode(&record(), &PlayerData::new(), &[]);
    over[LEN - 6..LEN - 4].copy_from_slice(&((persist::MAX_ITEMS + 1) as u16).to_le_bytes());
    fix(&mut over);
    assert!(Save::decode(&over).is_none(), "a count past the store must not decode");
}

#[test]
fn the_length_is_exactly_what_the_fields_need() {
    // HKS4: 74 bytes of fields, the script reserve and its identity, the shop
    // bools and counters, the checksum. HKS5 added the PlayerData bools and
    // levels and the item count. HKS6: the 52-byte header, the field count, the
    // item count, the checksum; and at most one six-byte entry per field.
    assert_eq!(LEN_HKS4, 74 + SCRIPT_FIELD_SLOTS * 4 + 4 + 4 + SHOP_COUNTERS + 4);
    assert_eq!(LEN_HKS5, LEN_HKS4 - 4 + 4 + persist::LEVELS + 2 + 4);
    assert_eq!(LEN_HKS5, old::LEN);
    assert_eq!(LEN, 52 + 2 + 2 + 4);
    assert_eq!(MAX_LEN, LEN + COUNT * 6 + persist::MAX_ITEMS * 4);
}

// ---------------------------------------------------------------------------
// Migration: what the HKS5 guest restored from an HKS4 or HKS5 record is what
// HKS6 restores from it.
// ---------------------------------------------------------------------------

/// What the HKS5 guest's boot made of a record, in the shape each owner was
/// handed it (main.rs and persist.rs at 5d7d28be): the charm board and Sly's
/// half as recorded, the conversation word, the script reserve taken only for a
/// matching bank (an HKS4 record's top two slots then moved to the False
/// Knight), `persist`'s bits and levels, and the SceneData items.
#[derive(Debug, PartialEq)]
struct Booted {
    header: (u32, [i32; 2], i32, u32, u32, u8, u32, shade::Record),
    charms: (psx_math::Mask64, psx_math::Mask64, u8, bool),
    shop: Option<(u32, [u8; 6])>,
    npc: u32,
    scripts: Vec<i32>,
    scripts_taken: bool,
    player_bits: u32,
    levels: [u8; 4],
    items: Vec<u32>,
}
fn old_boot(bytes: &[u8], bank: &Bank) -> Option<Booted> {
    let (s, items) = old::Save::decode(bytes)?;
    let taken = s.script_field_fnv == bank.fnv;
    let mut reserve = if taken { s.script_fields } else { [0; SCRIPT_FIELD_SLOTS] };
    let mut player = s.player_bools;
    let mut store = persist::Store::new();
    for item in old::Save::items(items) {
        let (k, sc, l, v) = persist::unpack(item).unwrap();
        store.set(k, sc, l, v);
    }
    if s.version == 4 && taken {
        let arena = reserve[SCRIPT_FIELD_SLOTS - 1] != 0;
        let first_plop = reserve[SCRIPT_FIELD_SLOTS - 2] != 0;
        reserve[SCRIPT_FIELD_SLOTS - 1] = 0;
        reserve[SCRIPT_FIELD_SLOTS - 2] = 0;
        if first_plop { player |= 1 << persist::FALSE_KNIGHT_FIRST_PLOP; }
        if arena {
            player |= 1 << persist::FALSE_KNIGHT_DEFEATED;
            store.set(Kind::BattleScene, FALSE_KNIGHT_SCENE, 0, 1);
        }
    }
    // `shop::State::restore` refuses slot bits past the cooked flags, so such a
    // record left the shelf at `State::new`, whose record is all zero.
    let named = (1u32 << player_data::SHOP_FLAGS.len()) - 1;
    Some(Booted {
        header: (s.scene, s.seat, s.facing, s.region, s.geo, s.door_hits, s.sequence, s.shade),
        charms: (s.charms_owned, s.charms_equipped, s.charm_notches, s.can_overcharm),
        shop: Some(if s.shop_slots & !named == 0 { (s.shop_slots, s.shop_counters) } else { (0, [0; 6]) }),
        npc: s.npc_conversations,
        scripts: reserve[..bank.fields.len()].to_vec(),
        scripts_taken: taken,
        player_bits: player,
        levels: s.player_levels,
        items: store.items().to_vec(),
    })
}
/// The same, read off HKS6's stores the way `main` and `persist` now hand them
/// out.
fn new_boot(bytes: &[u8], bank: &Bank) -> Option<(Booted, PlayerData, persist::Store)> {
    let (s, pd, store, restored) = load(bytes, bank)?;
    let booted = Booted {
        header: (s.scene, s.seat, s.facing, s.region, s.geo, s.door_hits, s.sequence, s.shade),
        charms: pd.charms(),
        shop: pd.shop(STARTING_MASKS, SOUL_PER_VESSEL),
        npc: pd.get(player_data::PORT_NPC_CURSORS) as u32,
        scripts: bank.fields.iter().map(|&f| pd.get(f)).collect(),
        scripts_taken: restored.scripts,
        player_bits: pd.legacy_bits(),
        levels: core::array::from_fn(|i| pd.get(player_data::FIREBALL_LEVEL + i) as u8),
        items: store.items().to_vec(),
    };
    assert_eq!(restored.items, store.items().len());
    Some((booted, pd, store))
}
/// The one deliberate difference from the HKS5 guest: the two fields the port
/// kept apart from the source's are merged. `openedMapperShop` is the script's
/// value or Cornifer's bit 22, and `HK_WORLD_PLAYER` bit 22 then reads it; the
/// title card's area (HKS5 level 3, a title index plus one) becomes that area's
/// `areaID` in `currentArea` and stops being a level. Returns the HKS5 state
/// with that applied, and the names of what it changed.
fn merged(old: &Booted, bank: &Bank) -> (Booted, Vec<String>) {
    let mut b = Booted { scripts: old.scripts.clone(), items: old.items.clone(), shop: old.shop, ..*old };
    let slot = |name: &str| bank.names.iter().position(|n| n == name);
    let mut changed = Vec::new();
    if let Some(om) = slot("openedMapperShop") {
        if b.scripts[om] == 0 && b.player_bits >> 22 & 1 != 0 {
            b.scripts[om] = 1;
            changed.push("openedMapperShop 0 -> 1 (Cornifer's bit 22)".to_string());
        }
        if b.scripts[om] != 0 && b.player_bits >> 22 & 1 == 0 {
            b.player_bits |= 1 << 22;
            changed.push("HK_WORLD_PLAYER bit 22 0 -> 1 (the script's openedMapperShop)".to_string());
        }
    }
    let title = b.levels[3] as usize;
    if let (Some(ca), Some(&id)) = (slot("currentArea"), title.checked_sub(1).and_then(|t| bank.area_ids.get(t))) {
        if id != 0 && b.scripts[ca] != i32::from(id) {
            changed.push(format!("currentArea {} -> {id} (title card area {})", b.scripts[ca], title - 1));
            b.scripts[ca] = i32::from(id);
        }
    }
    if b.levels[3] != 0 {
        changed.push(format!("port.areaTitle {} -> retired", b.levels[3]));
        b.levels[3] = 0;
    }
    (b, changed)
}
/// Both readers on one record, and the HKS6 record the next bench writes from
/// what was loaded, which must load back to the same stores. Returns what the
/// merge changed against the HKS5 guest; nothing else may differ.
fn same_state_both_ways(bytes: &[u8], bank: &Bank, what: &str) -> Vec<String> {
    let old = old_boot(bytes, bank).unwrap_or_else(|| panic!("{what}: the HKS5 reader refuses it"));
    let (want, changed) = merged(&old, bank);
    let (got, pd, store) = new_boot(bytes, bank).unwrap_or_else(|| panic!("{what}: HKS6 refuses it"));
    assert_eq!(got, want, "{what}");
    let (save, _, _) = Save::decode(bytes).unwrap();
    let again = encode(&save, &pd, store.items());
    let (_, pd2, store2, _) = load(&again, bank).unwrap();
    assert_eq!((pd2, store2.items()), (pd, store.items()), "{what}: the HKS6 resave");
    changed
}

fn old_record(version: u8) -> old::Save {
    old::Save {
        scene: 3, seat: [-65536, 720896], facing: -1, region: 7, geo: 41, door_hits: 13,
        shade: shade::Record { present: true, scene: 2, position: [-131072, 65536], hp: 10, geo_pool: 77 },
        sequence: 9,
        charms_owned: psx_math::Mask64::from_words(0x9abc_def0, 0x1234_5678),
        charms_equipped: psx_math::Mask64::from_words(0x0f0f_0f0f, 0x0f0f_0f0f),
        charm_notches: 7, can_overcharm: true,
        npc_conversations: 0xdead_beef,
        script_fields: core::array::from_fn(|i| (i as i32 + 1) * -3),
        script_field_fnv: 0,
        shop_slots: 0x0000_0d6b,
        shop_counters: [3, 2, 4, 1, 1, 1],
        player_bools: 0x0f00_81ff,
        player_levels: [1, 2, 3, 4],
        version,
    }
}
fn old_encode(save: &old::Save, items: &[u32]) -> Vec<u8> {
    let mut out = [0u8; old::MAX_LEN];
    let len = save.encode(items, &mut out);
    out[..len].to_vec()
}
/// An HKS4 record as the HKS4 guest wrote it: HKS5's first 152 bytes, then its
/// checksum.
fn hks4(save: &old::Save) -> Vec<u8> {
    let mut bytes = old_encode(save, &[])[..LEN_HKS4].to_vec();
    bytes[0..4].copy_from_slice(b"HKS4");
    fix(&mut bytes);
    bytes
}

#[test]
fn hks5_and_hks4_records_load_to_what_the_hks5_guest_restored() {
    let bank = bank();
    let world = world();
    for fnv in [0, bank.fnv, bank.fnv ^ 1] {
        let mut s = old_record(5);
        s.script_field_fnv = fnv;
        same_state_both_ways(&old_encode(&s, world.items()), &bank, &format!("HKS5 fnv {fnv}"));
        same_state_both_ways(&old_encode(&s, &[]), &bank, &format!("HKS5 empty world fnv {fnv}"));
        same_state_both_ways(&hks4(&s), &bank, &format!("HKS4 fnv {fnv}"));
        // The False Knight in HKS4's top two slots, with and without the bank.
        for (plop, arena) in [(1, 0), (0, 1), (1, 1)] {
            s.script_fields[SCRIPT_FIELD_SLOTS - 2] = plop;
            s.script_fields[SCRIPT_FIELD_SLOTS - 1] = arena;
            same_state_both_ways(&hks4(&s), &bank, &format!("HKS4 False Knight {plop}{arena} fnv {fnv}"));
        }
    }
    // Every legacy bit, shaman's two included, comes back as the same bitmask.
    for bit in 0..32 {
        let mut s = old_record(5);
        s.player_bools = 1 << bit;
        same_state_both_ways(&old_encode(&s, &[]), &bank, &format!("bit {bit}"));
    }
    // Records the owners refuse are refused the same way: shop bits past the
    // cooked flags, a board that equips what it does not own, no notches.
    let mut s = old_record(5);
    s.shop_slots |= 1 << 20;
    same_state_both_ways(&old_encode(&s, &[]), &bank, "shop bits past the flags");
    s.charms_equipped = psx_math::Mask64::from_words(u32::MAX, u32::MAX);
    s.charm_notches = 0;
    same_state_both_ways(&old_encode(&s, &[]), &bank, "a board charms::restore refuses");
    // And a corrupt one is corrupt to both.
    let mut bad = old_encode(&old_record(5), &[]);
    bad[70] ^= 1;
    assert!(old_boot(&bad, &bank).is_none() && Save::decode(&bad).is_none());
}

/// Every file in a card image that holds a record, as (name, payload). The
/// psx-mc container ahead of the payload (crates/psx-mc/src/lib.rs): "PMC1",
/// flags (bit 0 LZSS), three reserved, raw_len, stored_len.
fn card_records(image: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for block in 1..16 {
        let entry = &image[block * 128..block * 128 + 128];
        if entry[0] != 0x51 {
            continue;
        }
        let name = String::from_utf8_lossy(&entry[10..30]).trim_end_matches('\0').to_string();
        let at = block * 8192 + 256;
        assert_eq!(&image[at..at + 4], b"PMC1", "{name}: no container where psx-mc writes it");
        assert_eq!(image[at + 4] & 1, 0, "{name}: a compressed payload");
        let raw = u32::from_le_bytes(image[at + 8..at + 12].try_into().unwrap()) as usize;
        let stored = u32::from_le_bytes(image[at + 12..at + 16].try_into().unwrap()) as usize;
        assert_eq!(raw, stored, "{name}");
        out.push((name, image[at + 16..at + 16 + raw].to_vec()));
    }
    out
}

/// What the merge changes in the committed fixtures, and nothing else: the two
/// copies of fungus1-31-bench.mcd, the only fixtures written after the script
/// bank named these fields. (Title indices: 0 Crossroads, areaID 2; 2
/// Greenpath, areaID 3.)
const MERGE_CHANGES: &[&str] = &[
    "fungus1-31-bench.mcd BASLUS-00000HKPSX01: HK_WORLD_PLAYER bit 22 0 -> 1 (the script's openedMapperShop)",
    "fungus1-31-bench.mcd BASLUS-00000HKPSX01: currentArea -1 -> 3 (title card area 2)",
    "fungus1-31-bench.mcd BASLUS-00000HKPSX01: port.areaTitle 3 -> retired",
    "fungus1-31-bench.mcd BASLUS-00000HKPSX00: HK_WORLD_PLAYER bit 22 0 -> 1 (the script's openedMapperShop)",
    "fungus1-31-bench.mcd BASLUS-00000HKPSX00: currentArea -1 -> 2 (title card area 0)",
    "fungus1-31-bench.mcd BASLUS-00000HKPSX00: port.areaTitle 1 -> retired",
];

#[test]
fn every_committed_fixture_card_loads_to_the_same_state_as_before() {
    // tools/cards/*.mcd are what the test walks boot from, written by the HKS4
    // and HKS5 guests. Each record has to load through HKS6's migration to
    // exactly what the HKS5 guest gave every owner, and then survive the HKS6
    // resave the next bench makes.
    let bank = bank();
    let root = std::path::Path::new(file!()).parent().unwrap().parent().unwrap().join("tools/cards");
    let mut cards: Vec<_> = std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "mcd")).collect();
    cards.sort();
    let (mut records, mut versions, mut taken) = (0, [0; 7], 0);
    let mut changes = Vec::new();
    for card in &cards {
        let image = std::fs::read(card).unwrap();
        for (name, payload) in card_records(&image) {
            let what = format!("{} {name}", card.file_name().unwrap().to_string_lossy());
            if !name.starts_with("BASLUS-00000HKPSX") || name.ends_with("MS") {
                continue;
            }
            if payload.starts_with(b"HKS6") {
                // A record tools/seed_card.py wrote as HKS6 (for a field HKS5 has no place for): no older
                // guest reads it, so it is checked against its own resave only.
                let (_, pd, store, _) = load(&payload, &bank).unwrap_or_else(|| panic!("{what}: HKS6 refuses it"));
                let (save, _, _) = Save::decode(&payload).unwrap();
                let again = encode(&save, &pd, store.items());
                assert_eq!(again, payload, "{what}: not what an HKS6 bench writes for the same state");
                versions[6] += 1;
                records += 1;
                continue;
            }
            for change in same_state_both_ways(&payload, &bank, &what) {
                changes.push(format!("{what}: {change}"));
            }
            let (save, _, _) = Save::decode(&payload).unwrap();
            versions[save.version as usize] += 1;
            taken += usize::from(new_boot(&payload, &bank).unwrap().0.scripts_taken);
            records += 1;
        }
    }
    eprintln!("{} cards, {records} records (HKS4 {}, HKS5 {}, seeded HKS6 {}), {taken} with this bank's script fields",
        cards.len(), versions[4], versions[5], versions[6]);
    assert!(cards.len() >= 50 && records >= cards.len());
    assert!(versions[4] > 0 && versions[5] > 0, "both older formats are covered");
    // Every fixture field the openedMapperShop and currentArea merge changes.
    // Nothing else may differ from the HKS5 guest (same_state_both_ways).
    for change in &changes {
        eprintln!("merge: {change}");
    }
    assert_eq!(changes, MERGE_CHANGES);
}

#[test]
fn the_charm_and_shop_owners_round_trip_through_their_fields() {
    let mut pd = PlayerData::new();
    let board = (psx_math::Mask64::from_words(0x8000_0001, 0xff), psx_math::Mask64::from_words(1, 0), 4, true);
    pd.put_charms(board);
    assert_eq!(pd.charms(), board);
    let shop = (0b1010_0101_0011u32, [3, 2, 4, 1, 7, 9]);
    pd.put_shop(shop, STARTING_MASKS, SOUL_PER_VESSEL);
    assert_eq!(pd.shop(STARTING_MASKS, SOUL_PER_VESSEL), Some(shop));
    assert_eq!(pd.get(player_data::MAX_HEALTH_BASE), 9, "maxHealthBase in masks");
    assert_eq!(pd.get(player_data::MP_RESERVE_MAX), 33, "MPReserveMax in SOUL");
    // A new game is what the owners start on.
    let fresh = PlayerData::new();
    assert_eq!(fresh.shop(STARTING_MASKS, SOUL_PER_VESSEL), Some((0, [0; 6])));
    assert_eq!(fresh.charms().2, 3);
    // Values the shop's record cannot hold are refused, not wrapped.
    for (field, value) in [(player_data::MP_RESERVE_MAX, 34), (player_data::MAX_HEALTH_BASE, 4),
                           (player_data::HEART_PIECES, 256), (player_data::RANCID_EGGS, -1)] {
        let mut odd = PlayerData::new();
        odd.set(field, value);
        assert_eq!(odd.shop(STARTING_MASKS, SOUL_PER_VESSEL), None, "{}", player_data::NAMES[field]);
    }
    let mut reset = everything();
    reset.reset();
    assert_eq!(reset, PlayerData::new());
}

// ---------------------------------------------------------------------------
// What the player sees of a card operation (game/src/save_ui.rs, save.rs).
// ---------------------------------------------------------------------------
use save::Fault;
use save_ui::{Edges, Ui};

const ALL_FAULTS: [Fault; 7] = [Fault::NoCard, Fault::Full, Fault::Corrupt, Fault::Unformatted,
    Fault::Other, Fault::Comms, Fault::Write];

fn press(ui: &mut Ui, e: Edges) -> Option<bool> { ui.step(e) }
const CROSS: Edges = Edges { up: false, down: false, cross: true, circle: false };
const CIRCLE: Edges = Edges { up: false, down: false, cross: false, circle: true };
const DOWN: Edges = Edges { up: false, down: true, cross: false, circle: false };

#[test]
fn every_card_fault_says_what_happened_and_the_numbers_are_stable() {
    // HK_SAVE_FAULT publishes discriminant + 1 and the replay pins read it, so
    // a new kind goes on the end and the old ones never move.
    assert_eq!(ALL_FAULTS.map(|f| f as u32), [0, 1, 2, 3, 4, 5, 6]);
    for f in ALL_FAULTS {
        assert!(!f.message().is_empty(), "{f:?} has no message");
        // The panel is 296 pixels wide; the font averages under 6.5 per glyph.
        assert!(f.message().len() <= 40 && f.hint().len() <= 44, "{f:?} would not fit its box");
        assert!(f.message().bytes().chain(f.hint().bytes()).all(|b| (32..=126).contains(&b)));
    }
    let messages: Vec<_> = ALL_FAULTS.iter().map(|f| f.message()).collect();
    for (i, a) in messages.iter().enumerate() {
        assert!(!messages[i + 1..].contains(a), "two faults share the message {a:?}");
    }
    // The formatting question is its own prompt, so it has no hint of its own.
    assert_eq!(Fault::Unformatted.hint(), "");
    for f in ALL_FAULTS.iter().filter(|f| **f != Fault::Unformatted) {
        assert!(!f.hint().is_empty());
    }
}

#[test]
fn a_card_that_answers_and_then_misbehaves_is_not_reported_as_absent() {
    use psx_mc::Error::*;
    // A read: protocol or checksum trouble is the card talking, not silence.
    assert_eq!(Fault::of(NoCard), Fault::NoCard);
    assert_eq!(Fault::of(Protocol), Fault::Comms);
    assert_eq!(Fault::of(BadChecksum), Fault::Corrupt);
    assert_eq!(Fault::of(NoSpace), Fault::Full);
    assert_eq!(Fault::of(NotFormatted), Fault::Unformatted);
    assert_eq!(Fault::of(Corrupt), Fault::Corrupt);
    // A write: the card refusing or losing a frame is a write error.
    assert_eq!(Fault::of_write(Protocol), Fault::Write);
    assert_eq!(Fault::of_write(BadChecksum), Fault::Write);
    assert_eq!(Fault::of_write(NoCard), Fault::NoCard);
    assert_eq!(Fault::of_write(NoSpace), Fault::Full);
    assert_eq!(Fault::of_write(NotFormatted), Fault::Unformatted);
}

#[test]
fn the_card_manager_title_is_double_width_shift_jis() {
    let mut out = [0xaa; 64];
    save::sjis_title("HOLLOW KNIGHT PSX", &mut out);
    // A..Z are 0x8260.., the double-width space 0x8140.
    let want: [u8; 34] = [0x82, 0x67, 0x82, 0x6e, 0x82, 0x6b, 0x82, 0x6b, 0x82, 0x6e, 0x82, 0x76, 0x81, 0x40,
        0x82, 0x6a, 0x82, 0x6d, 0x82, 0x68, 0x82, 0x66, 0x82, 0x67, 0x82, 0x73, 0x81, 0x40,
        0x82, 0x6f, 0x82, 0x72, 0x82, 0x77];
    assert_eq!(&out[..34], &want);
    assert!(out[34..].iter().all(|&b| b == 0), "the rest of the 64 bytes is zero padding");
    let mut digits = [0u8; 8];
    save::sjis_title("Ab 9", &mut digits);
    assert_eq!(digits, [0x82, 0x60, 0x82, 0x82, 0x81, 0x40, 0x82, 0x58]);
}

#[test]
fn the_save_icon_is_a_sixteen_by_sixteen_mask_the_manager_can_draw() {
    let icon = &save::ICON;
    assert_eq!(icon.pixels.len(), 128);
    // Palette index 0 is the transparent background, 1 the white mask.
    assert_eq!(icon.palette[0], 0);
    assert_eq!(icon.palette[1], 0x7fff);
    let pixel = |x: usize, y: usize| {
        let byte = icon.pixels[y * 8 + x / 2];
        if x % 2 == 0 { byte & 15 } else { byte >> 4 }
    };
    let mut lit = 0;
    for y in 0..16 {
        for x in 0..16 {
            assert!(pixel(x, y) <= 1, "only the two drawn colours are used");
            lit += pixel(x, y) as u32;
            // The Knight's mask is symmetric, left to right.
            assert_eq!(pixel(x, y), pixel(15 - x, y), "asymmetric at {x},{y}");
        }
    }
    assert!(lit > 60 && lit < 200, "{lit} lit pixels");
    assert!((0..16).all(|x| pixel(x, 0) == 0 && pixel(x, 15) == 0), "a clear border row top and bottom");
}

#[test]
fn the_seated_prompt_saves_on_yes_and_leaves_on_no_or_circle() {
    let mut ui = Ui::Idle;
    ui.ask();
    assert_eq!(ui, Ui::Ask(0));
    assert!(ui.modal() && ui.reads_pad());
    assert_eq!(press(&mut ui, CROSS), Some(false));
    assert_eq!(ui, Ui::Saving { frames: 0, format: false });
    let mut ui = Ui::Idle;
    ui.ask();
    assert_eq!(press(&mut ui, DOWN), None);
    assert_eq!(ui, Ui::Ask(1));
    assert_eq!(press(&mut ui, CROSS), None, "No writes nothing");
    assert_eq!(ui, Ui::Idle);
    ui.ask();
    assert_eq!(press(&mut ui, CIRCLE), None);
    assert_eq!(ui, Ui::Idle);
}

#[test]
fn the_notice_is_up_for_several_frames_before_the_write_begins() {
    let mut ui = Ui::Ask(0);
    press(&mut ui, CROSS);
    assert!(ui.modal() && !ui.reads_pad(), "a write in progress holds the Knight and ignores the pad");
    assert_eq!(press(&mut ui, CROSS), None);
    assert!(matches!(ui, Ui::Saving { .. }), "input while saving changes nothing");
    let mut presented = 0;
    while !ui.armed() {
        presented += 1;
        assert!(presented < 20);
    }
    // The write starts only after the notice has been drawn NOTICE_FRAMES times.
    assert_eq!(presented, save_ui::NOTICE_FRAMES as u32);
    assert!(!Ui::Idle.armed());
}

#[test]
fn a_successful_save_says_so_and_then_clears() {
    let mut ui = Ui::Saving { frames: 9, format: false };
    ui.finish(Ok(()));
    assert_eq!(ui, Ui::Saved(save_ui::SAVED_TICKS));
    assert!(!ui.modal(), "the Knight is free again while 'Game saved' shows");
    for _ in 0..save_ui::SAVED_TICKS {
        assert_ne!(ui, Ui::Idle);
        ui.step(Edges::default());
    }
    assert_eq!(ui, Ui::Idle);
}

#[test]
fn no_card_error_is_ever_silent() {
    for fault in ALL_FAULTS {
        let mut ui = Ui::Saving { frames: 9, format: false };
        ui.finish(Err(fault));
        match fault {
            // The card can be formatted, so the player is asked rather than told.
            Fault::Unformatted => assert_eq!(ui, Ui::Format(0)),
            _ => assert_eq!(ui, Ui::Failed(fault)),
        }
        assert!(ui.modal() && ui.reads_pad(), "{fault:?} must stay up until the player answers");
        assert_ne!(ui.code(), 0);
        // Ticks alone never dismiss it.
        for _ in 0..1000 {
            ui.step(Edges::default());
        }
        assert!(ui.modal(), "{fault:?} was dismissed by the passage of time");
    }
}

#[test]
fn a_failure_closes_on_cross_or_circle_and_the_format_question_can_be_answered() {
    let mut ui = Ui::Failed(Fault::NoCard);
    assert_eq!(press(&mut ui, CROSS), None);
    assert_eq!(ui, Ui::Idle);
    let mut ui = Ui::Failed(Fault::Full);
    press(&mut ui, CIRCLE);
    assert_eq!(ui, Ui::Idle);
    // "Format it and save?" Yes asks for a write that may format the card.
    let mut ui = Ui::Format(0);
    assert_eq!(press(&mut ui, CROSS), Some(true));
    assert_eq!(ui, Ui::Saving { frames: 0, format: true });
    // No leaves the card as it was.
    let mut ui = Ui::Format(0);
    press(&mut ui, DOWN);
    assert_eq!(ui, Ui::Format(1));
    assert_eq!(press(&mut ui, CROSS), None);
    assert_eq!(ui, Ui::Idle);
}

#[test]
fn the_telemetry_code_distinguishes_every_phase() {
    let codes = [Ui::Idle, Ui::Ask(0), Ui::Format(0), Ui::Saving { frames: 0, format: false },
        Ui::Saved(1), Ui::Failed(Fault::Write)].map(|u| u.code());
    assert_eq!(codes, [0, 1, 2, 3, 4, 5]);
}
