//! The port's SceneData and PlayerData: the world state that outlives a scene
//! load and rides the bench save record (`save.rs`, HKS5 onward).
//!
//! The source keeps two stores and this mirrors both. `PlayerData` is one flat
//! set of named fields; the few the port implements and nothing else writes are
//! the bits and levels below, each named after its source field. `SceneData`
//! is a list of persistent items keyed by scene and object: every
//! `PersistentBoolItem` (a broken wall, an opened cocoon, a revealed secret, an
//! arena's `Activated`) and every `GeoRock`'s `hitsLeft`. The list holds only
//! the items that differ from their authored state, sorted, so a fresh game is
//! an empty list and the record grows with what the player did rather than with
//! the size of the world.
//!
//! An item is one `u32`: kind in the top four bits, the catalogue scene id in
//! the next ten, the object's cooked state index in the next ten, and the value
//! in the low eight. Sorting the words sorts by (kind, scene, object), which is
//! what makes a lookup a binary search and the encoding canonical. The scene and
//! object ids are the cook's stable ones (`docs/STATE_IDENTITY.md`: breakables
//! keep `scene * 128 + state_index`, rocks `scene * 16 + state`), not the 64-bit
//! BLAKE2b ids of `hk_sim::persistent`, which the guest has no table for and
//! which would cost three times the card bytes per item.
//!
//! Two ways in. Breakables and Geo rocks already have session stores
//! (`world::State::broken`, `geo::World`'s rock pool), so those are copied into
//! the list when the player saves and back out when a save loads. Secret masks,
//! the Lifeblood cocoon and arena gates have no store that outlives their scene,
//! so their owners write here the moment the source's own bool flips and read
//! here when their scene is seated.

/// Items one record can carry. Measured against the admitted world rather than
/// guessed: 8 persistent breakables, 35 Geo rocks, 9 one-way secret masks, one
/// cocoon and one arena is 54, so a player who does everything this disc offers
/// uses under half. A full store refuses the next item and counts it in
/// `HK_WORLD_OVERFLOW`, which a route pins at zero; it never evicts one.
pub const MAX_ITEMS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// `Breakable` with a full `PersistentBoolItem` (not `dontSave`, not
    /// `semiPersistent`); value 1 is broken.
    Breakable = 1,
    /// `GeoRock`, value `hitsLeft`. Present once hit at all; 0 is depleted.
    GeoRock = 2,
    /// A one-way reveal mask (`Secret Mask`), value 1 is uncovered. Local id is
    /// the scene's reveal controller index.
    SecretMask = 3,
    /// `HealthCocoon`, value 1 is opened. Local id 0; one cocoon per scene.
    Cocoon = 4,
    /// A `Battle Scene`'s `Activated`, value 1 is won. Local id 0.
    BattleScene = 5,
    /// An enemy's own `PersistentBoolItem`, value 1 is dead. Local id is the
    /// owning table's index (`blocker_terrain::SOURCES` for the Blockers).
    Enemy = 6,
    /// A grub jar's `PersistentBoolItem`, value 1 is broken and its grub
    /// freed. Local id is the jar's index in its scene (host/hk-cook/src/props.rs), and
    /// the count of these is PlayerData `grubsCollected`.
    Grub = 7,
    /// A soul totem's `Value` left, from its `semiPersistent` PersistentIntItem;
    /// absent is full. Local id is the totem's index in its scene. A bench
    /// rest clears the kind, as the source resets semi-persistent items.
    SoulTotem = 8,
    /// An area title shown once, the source's visited bools. Scene field is the
    /// title's index in `title_card::TITLES`, local id 0.
    Visited = 9,
    /// A chest's or a pickup's `PersistentBoolItem`, value 1 is opened or
    /// taken. Local id is its index in its scene (host/pickups.py numbers the
    /// chests first, then the pickups).
    Pickup = 10,
    /// The map's per-scene PlayerData: `scenesVisited` in bit 0 and
    /// `scenesMapped` in bit 1 of the value (`game_map.rs`). Local id 0.
    /// 8, 9 and 10 are fix/slice-gaps-3's (soul totems, title cards, pickups).
    MapScene = 11,
}
impl Kind {
    fn of(bits: u32) -> Option<Kind> {
        Some(match bits {
            1 => Kind::Breakable,
            2 => Kind::GeoRock,
            3 => Kind::SecretMask,
            4 => Kind::Cocoon,
            5 => Kind::BattleScene,
            6 => Kind::Enemy,
            7 => Kind::Grub,
            8 => Kind::SoulTotem,
            9 => Kind::Visited,
            10 => Kind::Pickup,
            11 => Kind::MapScene,
            _ => return None,
        })
    }
}
pub const MAX_SCENE: usize = 1 << 10;
pub const MAX_LOCAL: usize = 1 << 10;

/// The item word, or None for an id the layout cannot hold.
pub const fn pack(kind: Kind, scene: usize, local: usize, value: u8) -> Option<u32> {
    if scene >= MAX_SCENE || local >= MAX_LOCAL {
        return None;
    }
    Some((kind as u32) << 28 | (scene as u32) << 18 | (local as u32) << 8 | value as u32)
}
/// (kind, scene, local, value), or None for a word no build ever wrote.
pub fn unpack(item: u32) -> Option<(Kind, usize, usize, u8)> {
    Some((Kind::of(item >> 28)?, (item >> 18 & 0x3ff) as usize, (item >> 8 & 0x3ff) as usize, item as u8))
}

/// PlayerData bools, by bit. Append only: a bit's meaning is part of the card
/// format, so a retired field keeps its bit.
pub const FALSE_KNIGHT_DEFEATED: u32 = 0;
pub const FALSE_KNIGHT_FIRST_PLOP: u32 = 1;
pub const HAS_DASH: u32 = 2;
pub const HAS_WALLJUMP: u32 = 3;
pub const HAS_DOUBLE_JUMP: u32 = 4;
pub const HAS_SUPER_DASH: u32 = 5;
pub const HAS_SHADOW_DASH: u32 = 6;
pub const HAS_DREAM_NAIL: u32 = 7;
/// `hasCityKey`: the City Crest the `Key Giver` hands over.
pub const HAS_CITY_KEY: u32 = 8;
/// The map and its mapper, from bit 16 so the abilities and pickups other
/// branches add below it do not collide with them. `hasMap` (any map bought),
/// `mapCrossroads`, `hasQuill`, `hasPinBench`, `metCornifer`,
/// `corniferIntroduced`, `openedMapperShop`, `metIselda`. `mapDirtmouth` is a
/// new-save default that nothing clears, so it has no bit.
pub const HAS_MAP: u32 = 16;
pub const MAP_CROSSROADS: u32 = 17;
pub const HAS_QUILL: u32 = 18;
pub const HAS_PIN_BENCH: u32 = 19;
pub const MET_CORNIFER: u32 = 20;
pub const CORNIFER_INTRODUCED: u32 = 21;
pub const OPENED_MAPPER_SHOP: u32 = 22;
pub const MET_ISELDA: u32 = 23;
/// PlayerData `shaman` (0..3 here), two bits: game/src/shaman.rs.
pub const SHAMAN_LOW: u32 = 24;
pub const SHAMAN_HIGH: u32 = 25;
/// PlayerData small integers, by index: `fireballLevel`, `quakeLevel`,
/// `screamLevel`. The fourth byte is zero and reserved.
pub const FIREBALL_LEVEL: usize = 0;
#[allow(dead_code)]
pub const QUAKE_LEVEL: usize = 1;
#[allow(dead_code)]
pub const SCREAM_LEVEL: usize = 2;
pub const LEVELS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Store {
    items: [u32; MAX_ITEMS],
    len: usize,
    pub player: u32,
    pub levels: [u8; LEVELS],
    /// Items refused because the store was full or the id did not fit.
    pub overflow: u32,
}
impl Store {
    pub const fn new() -> Self {
        Self { items: [0; MAX_ITEMS], len: 0, player: 0, levels: [0; LEVELS], overflow: 0 }
    }
    pub fn items(&self) -> &[u32] {
        &self.items[..self.len]
    }
    fn find(&self, key: u32) -> Result<usize, usize> {
        self.items[..self.len].binary_search_by_key(&key, |item| item >> 8)
    }
    pub fn get(&self, kind: Kind, scene: usize, local: usize) -> Option<u8> {
        let key = pack(kind, scene, local, 0)? >> 8;
        self.find(key).ok().map(|at| self.items[at] as u8)
    }
    /// Insert or overwrite. False when the store is full or the id cannot be
    /// packed, which is counted rather than silently dropped.
    #[inline(never)]
    pub fn set(&mut self, kind: Kind, scene: usize, local: usize, value: u8) -> bool {
        let Some(item) = pack(kind, scene, local, value) else {
            self.overflow = self.overflow.saturating_add(1);
            return false;
        };
        match self.find(item >> 8) {
            Ok(at) => self.items[at] = item,
            Err(at) => {
                if self.len == MAX_ITEMS {
                    self.overflow = self.overflow.saturating_add(1);
                    return false;
                }
                self.items.copy_within(at..self.len, at + 1);
                self.items[at] = item;
                self.len += 1;
            }
        }
        true
    }
    /// Items of one kind, in every scene.
    pub fn count(&self, kind: Kind) -> usize {
        self.items().iter().filter(|&&item| item >> 28 == kind as u32).count()
    }
    /// Drop every item of one kind, ahead of re-copying it from its owner.
    pub fn clear_kind(&mut self, kind: Kind) {
        let mut kept = 0;
        for at in 0..self.len {
            let item = self.items[at];
            if item >> 28 != kind as u32 {
                self.items[kept] = item;
                kept += 1;
            }
        }
        self.len = kept;
    }
    /// Items of one kind in one scene, as (local, value).
    pub fn scene_items(&self, kind: Kind, scene: usize) -> impl Iterator<Item = (usize, u8)> + '_ {
        self.items().iter().filter_map(move |&item| {
            let (k, s, local, value) = unpack(item)?;
            (k == kind && s == scene).then_some((local, value))
        })
    }
    /// Every item of one kind, as (scene, local, value).
    pub fn all(&self, kind: Kind) -> impl Iterator<Item = (usize, usize, u8)> + '_ {
        self.items().iter().filter_map(move |&item| {
            let (k, scene, local, value) = unpack(item)?;
            (k == kind).then_some((scene, local, value))
        })
    }
    pub fn player(&self, bit: u32) -> bool {
        self.player & (1 << bit) != 0
    }
    pub fn set_player(&mut self, bit: u32) {
        self.player |= 1 << bit;
    }
    pub fn clear_player(&mut self, bit: u32) {
        self.player &= !(1 << bit);
    }
}

#[cfg(not(test))]
static mut STORE: Store = Store::new();
/// The one store. A static rather than a field of `frame::Game` for the reason
/// `battle_gates` gives: the arena that writes it rides the boss actor several
/// layers inside `enemies.rs`.
#[cfg(not(test))]
pub fn store() -> &'static mut Store {
    unsafe { &mut *(&raw mut STORE) }
}
/// Items held, the PlayerData bools, items restored by the last load, and
/// items refused. `HK_WORLD_OVERFLOW` is the one a route pins at zero.
#[cfg(not(test))]
#[no_mangle] pub static mut HK_WORLD_ITEMS: u32 = 0;
#[cfg(not(test))]
#[no_mangle] pub static mut HK_WORLD_PLAYER: u32 = 0;
#[cfg(not(test))]
#[no_mangle] pub static mut HK_WORLD_RESTORED: u32 = 0;
#[cfg(not(test))]
#[no_mangle] pub static mut HK_WORLD_OVERFLOW: u32 = 0;
/// Mirror the store for routes. Called after every mutation site, which are
/// all events rather than per-tick work.
#[cfg(not(test))]
#[inline(never)]
pub fn publish() {
    let s = store();
    unsafe {
        HK_WORLD_ITEMS = s.len as u32;
        HK_WORLD_PLAYER = s.player;
        HK_WORLD_OVERFLOW = s.overflow;
    }
}
#[cfg(not(test))]
pub fn get(kind: Kind, scene: usize, local: usize) -> Option<u8> {
    store().get(kind, scene, local)
}
#[cfg(not(test))]
#[inline(never)]
pub fn set(kind: Kind, scene: usize, local: usize, value: u8) {
    store().set(kind, scene, local, value);
    publish();
}
#[cfg(not(test))]
pub fn count(kind: Kind) -> usize {
    store().count(kind)
}
#[cfg(not(test))]
pub fn player(bit: u32) -> bool {
    store().player(bit)
}
#[cfg(not(test))]
#[inline(never)]
pub fn set_player(bit: u32) {
    store().set_player(bit);
    publish();
}
#[cfg(not(test))]
#[inline(never)]
pub fn clear_player(bit: u32) {
    store().clear_player(bit);
    publish();
}
/// Set one of `levels` (`fireballLevel` and its siblings).
#[cfg(not(test))]
#[inline(never)]
pub fn set_level(index: usize, value: u8) {
    store().levels[index] = value;
    publish();
}
/// A new game, or the development reset back to one.
#[cfg(not(test))]
#[inline(never)]
pub fn reset() {
    *store() = Store::new();
    unsafe { HK_WORLD_RESTORED = 0 }
    publish();
}

/// Boot: PlayerData from the record, SceneData from the card for an HKS5
/// record or out of the script reserve for an HKS4 one.
#[cfg(not(test))]
#[inline(never)]
pub fn load(save: &crate::save::Save, profile: usize) {
    let s = store();
    s.player = save.player_bools;
    s.levels = save.player_levels;
    let restored = if save.version >= 5 {
        crate::save::load_world(profile, save.sequence, s)
    } else {
        migrate_hks4(save, s)
    };
    unsafe { HK_WORLD_RESTORED = restored as u32 }
    publish();
}
/// HKS4 kept the False Knight's arena `Activated` and `falseKnightFirstPlop`
/// in the script reserve's top two slots. They move here, and the slots are
/// cleared so the cooked bank can have them. Only when the record's field list
/// is this bank's: `script::boot` has already dropped the values otherwise, and
/// a won fight is the conservative thing to lose, as it always was.
#[cfg(not(test))]
#[inline(never)]
fn migrate_hks4(save: &crate::save::Save, s: &mut Store) -> usize {
    use crate::save::SCRIPT_FIELD_SLOTS as SLOTS;
    if save.script_field_fnv != crate::script::SCRIPT_FIELD_FNV {
        return 0;
    }
    let arena = save.script_fields[SLOTS - 1] != 0;
    let first_plop = save.script_fields[SLOTS - 2] != 0;
    crate::script::set_player_data(SLOTS - 1, 0);
    crate::script::set_player_data(SLOTS - 2, 0);
    if first_plop {
        s.set_player(FALSE_KNIGHT_FIRST_PLOP);
    }
    if !arena {
        return 0;
    }
    s.set_player(FALSE_KNIGHT_DEFEATED);
    crate::world::false_knight_scene().map_or(0, |scene| usize::from(s.set(Kind::BattleScene, scene, 0, 1)))
}
/// Hand the loaded SceneData to the stores that own it for the session:
/// broken walls to the world, mined rocks to the Geo pool, an opened cocoon to
/// Lifeblood. Secret masks and arenas read the store when their scene seats.
#[cfg(not(test))]
#[inline(never)]
#[optimize(size)]
pub fn apply(state: &mut crate::world::State, geo: &mut crate::geo::World, life: &mut crate::lifeblood::World) {
    let s = store();
    for (scene, local, _) in s.all(Kind::Breakable) {
        state.restore_broken(scene * crate::world::BREAKABLES_PER_SCENE + local);
    }
    for (scene, local, left) in s.all(Kind::GeoRock) {
        geo.restore_rock(crate::geo::GEO_ROCKS, scene, local, left);
    }
    restore_cocoon(life);
}
/// The one cocoon this disc cooks, if the save or the session opened it. Also
/// after a death, which resets the cocoon's live bugs but not its
/// `PersistentBoolItem`.
#[cfg(not(test))]
#[inline(never)]
pub fn restore_cocoon(life: &mut crate::lifeblood::World) {
    if get(Kind::Cocoon, crate::lifeblood::LIFE_SPEC.scene, 0).is_some() {
        life.restore_opened();
    }
}
/// Bring the kinds that have their own session stores up to date, just ahead
/// of a save. `scene` is the one resident, whose non-persistent breakables are
/// still broken in the world's table and have to be told apart by the bank.
#[cfg(not(test))]
#[inline(never)]
#[optimize(size)]
pub fn snapshot(state: &crate::world::State, geo: &crate::geo::World, scene: usize) {
    let s = store();
    s.clear_kind(Kind::Breakable);
    state.for_each_persistent_broken(scene, |id| {
        s.set(Kind::Breakable, id / crate::world::BREAKABLES_PER_SCENE, id % crate::world::BREAKABLES_PER_SCENE, 1);
    });
    s.clear_kind(Kind::GeoRock);
    for (scene, local, left) in geo.rock_states() {
        s.set(Kind::GeoRock, scene, local, left);
    }
    publish();
}
/// This scene's one-way reveal controllers already uncovered, as a mask.
#[cfg(not(test))]
#[inline(never)]
pub fn secrets(scene: usize) -> u16 {
    store().scene_items(Kind::SecretMask, scene).fold(0, |mask, (local, _)| mask | 1u16.checked_shl(local as u32).unwrap_or(0))
}
/// Record every uncovered one-way controller of this scene.
#[cfg(not(test))]
#[inline(never)]
pub fn record_secrets(scene: usize, revealed: u16) {
    let s = store();
    for local in 0..16 {
        if revealed & (1 << local) != 0 {
            s.set(Kind::SecretMask, scene, local, 1);
        }
    }
    publish();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn items_stay_sorted_unique_and_overwrite_in_place() {
        let mut s = Store::new();
        assert!(s.set(Kind::GeoRock, 3, 2, 4));
        assert!(s.set(Kind::Breakable, 0, 45, 1));
        assert!(s.set(Kind::Breakable, 0, 7, 1));
        assert!(s.set(Kind::GeoRock, 3, 2, 0));
        assert_eq!(s.items().len(), 3);
        assert!(s.items().windows(2).all(|w| w[0] >> 8 < w[1] >> 8));
        assert_eq!(s.get(Kind::GeoRock, 3, 2), Some(0));
        assert_eq!(s.get(Kind::Breakable, 0, 7), Some(1));
        assert_eq!(s.get(Kind::Breakable, 0, 8), None);
        s.clear_kind(Kind::Breakable);
        assert_eq!(s.items().len(), 1);
        assert_eq!(s.all(Kind::GeoRock).collect::<Vec<_>>(), [(3, 2, 0)]);
    }
    #[test]
    fn a_full_store_refuses_and_counts_rather_than_evicting() {
        let mut s = Store::new();
        for local in 0..MAX_ITEMS {
            assert!(s.set(Kind::Breakable, 1, local, 1));
        }
        assert!(!s.set(Kind::Breakable, 2, 0, 1));
        assert!(!s.set(Kind::Cocoon, MAX_SCENE, 0, 1));
        assert_eq!(s.overflow, 2);
        assert_eq!(s.items().len(), MAX_ITEMS);
        // An overwrite still lands when full.
        assert!(s.set(Kind::Breakable, 1, 5, 1));
    }
    #[test]
    fn the_word_layout_round_trips_at_its_bounds() {
        for (kind, scene, local, value) in [(Kind::BattleScene, 1023, 1023, 255), (Kind::Breakable, 0, 0, 1),
                                            (Kind::Enemy, 19, 0, 1), (Kind::SoulTotem, 44, 0, 4), (Kind::Visited, 3, 0, 1), (Kind::Pickup, 11, 1, 1),
                                            (Kind::MapScene, 59, 0, 3)] {
            let item = pack(kind, scene, local, value).unwrap();
            assert_eq!(unpack(item), Some((kind, scene, local, value)));
        }
        // The first kind no build has written.
        assert_eq!(unpack(0xC000_0000), None);
        assert_eq!(unpack(0), None);
    }
}
