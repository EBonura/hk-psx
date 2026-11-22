//! The port's one PlayerData store: every named field the source keeps in
//! `PlayerData` that this port implements, plus the few port-only values that
//! ride the same record, by a stable id.
//!
//! Until HKS6 the same job was spread over six stores, each with its own card
//! bytes: `persist`'s bools and levels, the script bank's 16-slot reserve, Sly's
//! bools and counters, the charm board, and the NPC conversation cursors
//! (docs/SAVE.md has the inventory). Two of them were indexed by cook order, so
//! a cook that added a field silently put old values on the wrong fields, and the
//! script reserve guarded against that by dropping all of them.
//!
//! Here a field is its id, and the id is its index in `NAMES`. Ids are append
//! only: a field that retires keeps its id and its name gains `port.retired`.
//! On the card a record carries only the fields that differ from their default,
//! as `(id, value)` pairs in id order, so a record written before a field existed
//! reads that field as its default and every other field as it was written.
//!
//! The first 32 ids are `persist`'s PlayerData bits of HKS4 and HKS5, at the same
//! numbers: the cooked pickups name an ability by that bit, and
//! `HK_WORLD_PLAYER` (which routes pin) is still that bitmask. Ids 9 to 15 and 27
//! to 31 were never written by this branch and stay reserved for the branches
//! that use them; id 24 is `shaman`, whose two bits were 24 and 25.
//!
//! `NAMES` is the original field name, so a cooked FSM's
//! `GetPlayerDataBool`/`SetPlayerDataInt` can be bound to a field at compile time
//! (`find`, used by `script.rs`). A name starting `port.` is the port's own and
//! can never match an original one. The names are only read in const context
//! and by tests, so the guest never links them.
//!
//! Every value is an `i32`, the widest the source's ints need, and a bool is 0 or
//! 1. Charms, the shop and the NPC cursors keep their own session state as they
//! did; their fields here are brought up to date from them before a save
//! (`snapshot`) and handed back to them at boot.

pub type Field = usize;

/// The first 32 ids are named where they are used, as `persist::HAS_DASH` and
/// its siblings (checked against `NAMES` at the bottom of this file).
pub const FALSE_KNIGHT_DEFEATED: Field = 0;
pub const FALSE_KNIGHT_FIRST_PLOP: Field = 1;
/// `shaman`, 0 to 3 (game/src/shaman.rs). HKS5 kept it in bits 24 and 25.
pub const SHAMAN: Field = 24;
/// Ids below this were `persist`'s PlayerData bits.
pub const LEGACY_BITS: usize = 32;
/// `fireballLevel`, then `quakeLevel`, `screamLevel` and the port's area title
/// (`persist::level`).
pub const FIREBALL_LEVEL: Field = 32;
/// Id 22, `port.openedMapperShop`, is Cornifer's panel's copy of
/// `openedMapperShop` (`mapper.rs`); id 39 is the original, which the
/// Crossroads_06 FSM writes through the script bank. They are one field in the
/// source; joining them changes `HK_WORLD_PLAYER` bit 22 and `HK_SCRIPT_FLAGS`
/// on routes, so it is left to a deliberate step (docs/SAVE.md). Likewise id 35,
/// `port.areaTitle`, is `title_card.rs`'s area (the title index plus one), not
/// the source's `currentArea` (a map zone, id 37), which the Town's
/// `Area Resetter` FSM writes. Ids 36 to 42 are bound to the cooked script bank
/// by name (`script::FIELD_MAP`).
/// Sly's bools, in the order `data/shop.rs` numbers them (`Flag::Slot`). The
/// harness checks the names against the cooked `FLAG_NAMES`.
pub const SHOP_FLAGS: [Field; 12] = [43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54];
pub const HEART_PIECES: Field = 55;
pub const VESSEL_FRAGMENTS: Field = 56;
pub const MAX_HEALTH_BASE: Field = 57;
pub const MP_RESERVE_MAX: Field = 58;
pub const SIMPLE_KEYS: Field = 59;
pub const RANCID_EGGS: Field = 60;
pub const CHARM_SLOTS: Field = 61;
pub const CAN_OVERCHARM: Field = 62;
/// `gotCharm_1` to `gotCharm_64` and `equippedCharm_N` as the charm board keeps
/// them, one bit per charm (`charms.rs`), in two words each.
pub const PORT_CHARMS_OWNED: Field = 63;
pub const PORT_CHARMS_EQUIPPED: Field = 65;
/// Each talkable NPC's conversation cursor, two bits per NPC (`npc_state.rs`).
pub const PORT_NPC_CURSORS: Field = 67;

pub const COUNT: usize = 68;
pub const NAMES: [&str; COUNT] = [
    "falseKnightDefeated",
    "falseKnightFirstPlop",
    "hasDash",
    "hasWalljump",
    "hasDoubleJump",
    "hasSuperDash",
    "hasShadowDash",
    "hasDreamNail",
    "hasCityKey",
    "port.reserved9",
    "port.reserved10",
    "port.reserved11",
    "port.reserved12",
    "port.reserved13",
    "port.reserved14",
    "port.reserved15",
    "hasMap",
    "mapCrossroads",
    "hasQuill",
    "hasPinBench",
    "metCornifer",
    "corniferIntroduced",
    "port.openedMapperShop",
    "metIselda",
    "shaman",
    "port.retired.shamanHigh",
    "hornet1Defeated",
    "port.reserved27",
    "port.reserved28",
    "port.reserved29",
    "port.reserved30",
    "port.reserved31",
    "fireballLevel",
    "quakeLevel",
    "screamLevel",
    "port.areaTitle",
    "seenFocusTablet",
    "currentArea",
    "corn_crossroadsLeft",
    "openedMapperShop",
    "quirrelLeftEggTemple",
    "hornet_f19",
    "hornetGreenpath",
    "gaveSlykey",
    "hasSlykey",
    "slyShellFrag1",
    "slyShellFrag2",
    "slyVesselFrag1",
    "slySimpleKey",
    "hasLantern",
    "slyRancidEgg",
    "slyShellFrag3",
    "slyShellFrag4",
    "slyVesselFrag2",
    "hasWhiteKey",
    "heartPieces",
    "vesselFragments",
    "maxHealthBase",
    "MPReserveMax",
    "simpleKeys",
    "rancidEggs",
    "charmSlots",
    "canOvercharm",
    "port.gotCharm_1to32",
    "port.gotCharm_33to64",
    "port.equippedCharm_1to32",
    "port.equippedCharm_33to64",
    "port.npcCursors",
];
/// A new game's value. Matches what the owners start on: `shop::State::new`'s
/// five masks and `charms::State::new`'s three notches (checked where both are
/// in scope, at the bottom of this file).
pub const fn default(field: Field) -> i32 {
    match field {
        MAX_HEALTH_BASE => 5,
        CHARM_SLOTS => 3,
        _ => 0,
    }
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}
/// The field with this original name, or `COUNT` when the port has none. For
/// const context: a cooked FSM field binds here at compile time.
pub const fn find(name: &str) -> Field {
    let mut i = 0;
    while i < COUNT {
        if same(NAMES[i], name) {
            return i;
        }
        i += 1;
    }
    COUNT
}

/// On the card: a `u16` count, then `count` entries of a `u16` id and an `i32`
/// value, little endian, ids strictly increasing, defaults left out.
pub const ENTRY: usize = 6;
pub const MAX_SECTION: usize = 2 + COUNT * ENTRY;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerData {
    values: [i32; COUNT],
}
impl PlayerData {
    pub const fn new() -> Self {
        let mut values = [0; COUNT];
        let mut i = 0;
        while i < COUNT {
            values[i] = default(i);
            i += 1;
        }
        Self { values }
    }
    /// Back to a new game's values, in place.
    #[inline(never)]
    pub fn reset(&mut self) {
        self.values = [0; COUNT];
        self.values[MAX_HEALTH_BASE] = default(MAX_HEALTH_BASE);
        self.values[CHARM_SLOTS] = default(CHARM_SLOTS);
    }
    pub fn get(&self, field: Field) -> i32 {
        self.values[field]
    }
    pub fn set(&mut self, field: Field, value: i32) {
        self.values[field] = value;
    }
    pub fn flag(&self, field: Field) -> bool {
        self.values[field] != 0
    }
    pub fn put(&mut self, field: Field, on: bool) {
        self.values[field] = i32::from(on);
    }
    /// `persist`'s HKS4/HKS5 bitmask, as `HK_WORLD_PLAYER` still publishes it.
    pub fn legacy_bits(&self) -> u32 {
        let mut bits = 0;
        for id in 0..LEGACY_BITS {
            if id != SHAMAN && self.values[id] != 0 {
                bits |= 1 << id;
            }
        }
        bits | (self.values[SHAMAN] as u32 & 3) << SHAMAN
    }
    /// The bits back onto their fields: every bit its own bool, except 24 and 25,
    /// which are `shaman`'s value.
    pub fn set_legacy_bits(&mut self, bits: u32) {
        for id in 0..LEGACY_BITS {
            self.values[id] = (bits >> id & 1) as i32;
        }
        self.values[SHAMAN] = (bits >> SHAMAN & 3) as i32;
        self.values[SHAMAN + 1] = 0;
    }
    /// The card section, into `out`; returns its length.
    pub fn encode(&self, out: &mut [u8]) -> usize {
        let mut at = 2;
        let mut count = 0u16;
        for id in 0..COUNT {
            let value = self.values[id];
            if value == default(id) {
                continue;
            }
            out[at..at + 2].copy_from_slice(&(id as u16).to_le_bytes());
            out[at + 2..at + 6].copy_from_slice(&value.to_le_bytes());
            at += ENTRY;
            count += 1;
        }
        out[..2].copy_from_slice(&count.to_le_bytes());
        at
    }
    /// The section's length when it is well formed: ids in range and strictly
    /// increasing. A checksum proves the bytes arrived, not that a build wrote
    /// them, so a section no build could write is refused like the item list is.
    /// An id past `COUNT` is a field from a newer build: refused rather than
    /// dropped, because a save at the next bench would lose it for good.
    pub fn check(bytes: &[u8]) -> Option<usize> {
        let count = u16::from_le_bytes([*bytes.first()?, *bytes.get(1)?]) as usize;
        if count > COUNT {
            return None;
        }
        let len = 2 + count * ENTRY;
        let body = bytes.get(2..len)?;
        let mut last = None;
        for entry in body.chunks_exact(ENTRY) {
            let id = u16::from_le_bytes([entry[0], entry[1]]) as usize;
            if id >= COUNT || last.is_some_and(|last| id <= last) {
                return None;
            }
            last = Some(id);
        }
        Some(len)
    }
    /// A section `check` accepted, onto a new game's values.
    pub fn decode(&mut self, bytes: &[u8]) {
        self.reset();
        let count = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        for entry in bytes[2..2 + count * ENTRY].chunks_exact(ENTRY) {
            let id = u16::from_le_bytes([entry[0], entry[1]]) as usize;
            self.values[id] = i32::from_le_bytes([entry[2], entry[3], entry[4], entry[5]]);
        }
    }

    /// The charm board's record (`charms::State::record`) onto its fields.
    pub fn put_charms(&mut self, board: (psx_math::Mask64, psx_math::Mask64, u8, bool)) {
        let (owned, equipped, notches, can_overcharm) = board;
        self.values[PORT_CHARMS_OWNED] = owned.low() as i32;
        self.values[PORT_CHARMS_OWNED + 1] = owned.high() as i32;
        self.values[PORT_CHARMS_EQUIPPED] = equipped.low() as i32;
        self.values[PORT_CHARMS_EQUIPPED + 1] = equipped.high() as i32;
        self.values[CHARM_SLOTS] = i32::from(notches);
        self.put(CAN_OVERCHARM, can_overcharm);
    }
    /// And back, for `charms::boot`. A notch count no board could hold reads as
    /// zero, which `charms::State::restore` refuses like any other bad board.
    pub fn charms(&self) -> (psx_math::Mask64, psx_math::Mask64, u8, bool) {
        let word = |f: Field| self.values[f] as u32;
        (
            psx_math::Mask64::from_words(word(PORT_CHARMS_OWNED), word(PORT_CHARMS_OWNED + 1)),
            psx_math::Mask64::from_words(
                word(PORT_CHARMS_EQUIPPED),
                word(PORT_CHARMS_EQUIPPED + 1),
            ),
            u8::try_from(self.values[CHARM_SLOTS]).unwrap_or(0),
            self.flag(CAN_OVERCHARM),
        )
    }
    /// Sly's record (`shop::State::record`: the slot bits, then the shards, the
    /// fragments, the masks above `starting_masks`, the vessels, the keys and the
    /// eggs) onto its fields. `maxHealthBase` and `MPReserveMax` take the source's
    /// own units, masks and SOUL.
    pub fn put_shop(&mut self, shop: (u32, [u8; 6]), starting_masks: u8, soul_per_vessel: u16) {
        let (slots, [shards, fragments, extra_masks, vessels, keys, eggs]) = shop;
        for (slot, &field) in SHOP_FLAGS.iter().enumerate() {
            self.values[field] = (slots >> slot & 1) as i32;
        }
        self.values[HEART_PIECES] = i32::from(shards);
        self.values[VESSEL_FRAGMENTS] = i32::from(fragments);
        self.values[MAX_HEALTH_BASE] = i32::from(starting_masks) + i32::from(extra_masks);
        self.values[MP_RESERVE_MAX] = i32::from(vessels) * i32::from(soul_per_vessel);
        self.values[SIMPLE_KEYS] = i32::from(keys);
        self.values[RANCID_EGGS] = i32::from(eggs);
    }
    /// And back, for `shop::boot`, or None for values the record's bytes cannot
    /// hold (a reserve that is not whole vessels, fewer masks than a new game),
    /// which `shop::boot` treats as the contradictory record it is.
    pub fn shop(&self, starting_masks: u8, soul_per_vessel: u16) -> Option<(u32, [u8; 6])> {
        let byte = |f: Field| u8::try_from(self.values[f]).ok();
        let mut slots = 0u32;
        for (slot, &field) in SHOP_FLAGS.iter().enumerate() {
            slots |= u32::from(self.flag(field)) << slot;
        }
        let extra = u8::try_from(self.values[MAX_HEALTH_BASE] - i32::from(starting_masks)).ok()?;
        let reserve = self.values[MP_RESERVE_MAX];
        let per = i32::from(soul_per_vessel);
        if reserve < 0 || reserve % per != 0 {
            return None;
        }
        let vessels = u8::try_from(reserve / per).ok()?;
        Some((
            slots,
            [
                byte(HEART_PIECES)?,
                byte(VESSEL_FRAGMENTS)?,
                extra,
                vessels,
                byte(SIMPLE_KEYS)?,
                byte(RANCID_EGGS)?,
            ],
        ))
    }
}

#[cfg(not(test))]
static mut STORE: PlayerData = PlayerData::new();
/// The one store. A static for the reason `persist::store` is one: its writers
/// sit several layers inside the frame.
#[cfg(not(test))]
pub fn store() -> &'static mut PlayerData {
    unsafe { &mut *(&raw mut STORE) }
}
/// Bring the fields whose owners keep their own session state up to date: the
/// charm board, Sly's half and the NPC cursors. Before every save.
#[cfg(not(test))]
#[inline(never)]
pub fn snapshot() {
    let pd = store();
    pd.put_charms(crate::charms::record());
    pd.put_shop(crate::shop::record(), crate::shop::STARTING_MASKS, crate::shop::SOUL_PER_VESSEL);
    pd.set(PORT_NPC_CURSORS, crate::dialogue::met_bits() as i32);
}
#[cfg(not(test))]
const _: () = {
    use crate::persist as p;
    let named = [
        (p::FALSE_KNIGHT_DEFEATED, "falseKnightDefeated"),
        (p::FALSE_KNIGHT_FIRST_PLOP, "falseKnightFirstPlop"),
        (p::HAS_DASH, "hasDash"),
        (p::HAS_WALLJUMP, "hasWalljump"),
        (p::HAS_DOUBLE_JUMP, "hasDoubleJump"),
        (p::HAS_SUPER_DASH, "hasSuperDash"),
        (p::HAS_SHADOW_DASH, "hasShadowDash"),
        (p::HAS_DREAM_NAIL, "hasDreamNail"),
        (p::HAS_CITY_KEY, "hasCityKey"),
        (p::HAS_MAP, "hasMap"),
        (p::MAP_CROSSROADS, "mapCrossroads"),
        (p::HAS_QUILL, "hasQuill"),
        (p::HAS_PIN_BENCH, "hasPinBench"),
        (p::MET_CORNIFER, "metCornifer"),
        (p::CORNIFER_INTRODUCED, "corniferIntroduced"),
        (p::OPENED_MAPPER_SHOP, "port.openedMapperShop"),
        (p::MET_ISELDA, "metIselda"),
        (p::HORNET_1_DEFEATED, "hornet1Defeated"),
        (FIREBALL_LEVEL as u32 + p::FIREBALL_LEVEL as u32, "fireballLevel"),
        (FIREBALL_LEVEL as u32 + p::QUAKE_LEVEL as u32, "quakeLevel"),
        (FIREBALL_LEVEL as u32 + p::SCREAM_LEVEL as u32, "screamLevel"),
        (FIREBALL_LEVEL as u32 + p::CURRENT_AREA as u32, "port.areaTitle"),
    ];
    let mut i = 0;
    while i < named.len() {
        assert!(same(NAMES[named[i].0 as usize], named[i].1), "a persist id names another field");
        i += 1;
    }
    assert!(p::LEVELS == 4);
    assert!(default(MAX_HEALTH_BASE) == crate::shop::STARTING_MASKS as i32);
    assert!(default(CHARM_SLOTS) == crate::charms::STARTING_NOTCHES as i32);
    assert!(SHOP_FLAGS.len() == crate::shop::FLAG_COUNT);
};
