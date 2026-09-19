//! Sly's shop: what is on the shelf, what it costs and what a purchase hands over.
//!
//! The source spreads one purchase across four places and this module is all
//! four. `ShopMenuStock::Start` picks the base or the alternate list from two
//! PlayerData bools. `BuildItemList` decides which rows of that list reach the
//! shelf: an item is listed while its own bool is unset, its
//! `requiredPlayerDataBool` is empty or set, and its `removalPlayerDataBool` is
//! empty or unset. `ShopMenuStock::CanBuy` decides what can be afforded, from
//! `geo` and `charmsOwned`. `ShopItemStats::OnEnable` is the only price
//! modifier in the shop: with `dungDiscount` and Defender's Crest worn it takes
//! `(int)(cost * 0.8f)`. The `Confirm Control` FSM is the transaction itself,
//! and it runs in one fixed order: set the item bool, call
//! `HeroController.TakeGeo(Cost)`, play the thank-you, then branch on the
//! special type. That order is why a purchase here can never take Geo without
//! recording what it was for.
//!
//! Prices are the trap this module exists to not walk into. `ShopItemStats`
//! serializes a `cost` field and `Awake` throws it away, reparsing the real
//! price from the encrypted `Prices` language sheet. Eleven of Sly's fourteen
//! serialized costs are stale: the Lumafly Lantern serializes 1500 and sells
//! for 1800, the Rancid Egg serializes 500 and sells for 60, and three of the
//! four mask shards serialize the first one's 150. The cooked table carries the
//! sheet value, and `host/shops.py` refuses to cook if it cannot read the sheet.
//!
//! What this module does not own: it has no scene to stand in. Every
//! `ShopMenuStock` in the installed game lives in a scene the port has not
//! admitted, Sly's included (`Room_shop`), so nothing calls `buy` in the disc
//! build yet. It is written and tested as the transaction so that admitting the
//! scene is the only remaining step, not the first one.
use crate::charms;

/// One cooked stock row. `lines` is its localized description, already wrapped
/// by the host against the same glyph advances the guest draws with.
pub struct Item {
    pub name: &'static str,
    pub lines: &'static [&'static str],
    /// `ShopItemStats.cost` after `Awake` reparses it from the `Prices` sheet.
    pub cost: u16,
    /// The PlayerData bool the purchase sets, which is also what stops the item
    /// being listed a second time.
    pub sets: Flag,
    /// `requiredPlayerDataBool`: the shelf hides the row until this is set.
    pub requires: Option<Flag>,
    /// `removalPlayerDataBool`: the shelf drops the row once this is set.
    pub removed_by: Option<Flag>,
    pub delivery: Delivery,
    /// `CanBuy` refuses below this many charms owned.
    pub charms_required: u8,
    pub dung_discount: bool,
}
/// A PlayerData bool as the guest can actually read it.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Flag {
    /// A `gotCharm_N` bool. `charms::State` already holds that set, so a charm
    /// row points at the charm instead of keeping a second copy of its bool.
    Charm(usize),
    /// Any other shop bool, in the bitmask `State` below owns. The port has no
    /// general PlayerData store, so these live here until it grows one.
    Slot(usize),
}
/// The `Special Type?` branch of `Confirm Control`, for the six types Sly's
/// stock uses. The other twelve belong to shops in scenes the port has not
/// admitted and are deliberately absent rather than stubbed.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Delivery {
    /// Type 0: the bool the transaction already set is the whole delivery.
    Item,
    /// Type 1: `Heart Container Control` counts shards and fuses four to a mask.
    MaskShard,
    /// Type 2: sets `hasCharm` and increments `charmsOwned`.
    Charm(usize),
    /// Type 3: `Vessel Fragment Control` fuses three fragments to a vessel.
    VesselFragment,
    /// Type 10: increments `simpleKeys`.
    SimpleKey,
    /// Type 11: increments `rancidEggs`.
    RancidEgg,
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/shop.rs"));

/// Why the shop refused, in the terms the source refuses in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `BuildItemList` never put this row on the shelf.
    NotListed,
    /// `CanBuy`: `geo < GetCost()`.
    TooPoor,
    /// `CanBuy`: `charmsOwned < charmsRequired`.
    NotEnoughCharms,
}
impl Refusal {
    /// The port's own wording, as the charm refusals are: the source greys the
    /// row out and says nothing, so there is no line to take from a sheet.
    pub fn message(self) -> &'static str {
        match self {
            Refusal::NotListed => "Sold out",
            Refusal::TooPoor => "Not enough Geo",
            Refusal::NotEnoughCharms => "Too few charms",
        }
    }
}

/// What one purchase handed over, above the bool the transaction always sets.
/// The shop counts shards and fragments because the source's own fuse FSMs do;
/// the masks and SOUL reserve they turn into are the caller's to apply, since
/// the hero's vitals are `hk_sim`'s and not this module's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Delivered {
    pub charm: Option<usize>,
    pub masks: u8,
    pub soul_reserve: u16,
    pub simple_keys: u8,
    pub rancid_eggs: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// What `TakeGeo` took, after the Defender's Crest discount.
    pub paid: u16,
    pub delivered: Delivered,
}

/// `(int)(cost * 0.8f)`. The guest has no floats and divides instead;
/// `host/shops.py` checks the two agree on every shipped price at cook time,
/// because 0.8f rounds up and so never lands just below an exact fifth.
pub fn discounted(cost: u16) -> u16 {
    cost / 5 * 4 + cost % 5 * 4 / 5
}
/// `PlayerData.charmsOwned`, which is the size of the `gotCharm_N` set rather
/// than a stored number. `charms::State` derives it the same way.
pub fn charms_owned(charms: &charms::State) -> u32 {
    (1..=charms::CHARM_COUNT).filter(|&n| charms.owns(n)).count() as u32
}

/// The shop half of PlayerData: the `FLAG_COUNT` bools the cook indexed and the
/// counters the delivery branches increment. There is deliberately no `Default`:
/// a new save starts on five masks, not zero, and `new` is the only origin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    slots: u32,
    /// `heartPieces` and `vesselFragments`, which reset on each fuse.
    pub mask_shards: u8,
    pub vessel_fragments: u8,
    /// `maxHealthBase` in masks, so a shard purchase can stop at the cap.
    pub masks: u8,
    pub simple_keys: u8,
    pub rancid_eggs: u8,
}
impl State {
    pub const fn new() -> Self {
        Self { slots: 0, mask_shards: 0, vessel_fragments: 0, masks: STARTING_MASKS,
               simple_keys: 0, rancid_eggs: 0 }
    }
    pub fn item(index: usize) -> &'static Item {
        &ITEMS[index]
    }
    /// `PlayerData::GetBool` for the one bool shape the shop reads.
    pub fn bool_set(&self, flag: Flag, charms: &charms::State) -> bool {
        match flag {
            Flag::Charm(charm) => charms.owns(charm),
            Flag::Slot(slot) => self.slots & 1 << slot != 0,
        }
    }
    fn set_bool(&mut self, flag: Flag, charms: &mut charms::State) {
        match flag {
            Flag::Charm(charm) => charms.give(charm),
            Flag::Slot(slot) => self.slots |= 1 << slot,
        }
    }
    /// `ShopMenuStock::Start`: the alternate list once either key bool is set.
    /// `UpdateStock` rechecks only the first, which is why the two are separate
    /// bools rather than one; the shop is rebuilt on entry either way.
    pub fn stock(&self, charms: &charms::State) -> &'static [u8] {
        if ALTERNATE_WHEN.iter().any(|&f| self.bool_set(f, charms)) {
            ALTERNATE_STOCK
        } else {
            BASE_STOCK
        }
    }
    /// `BuildItemList`, per row. An item with no `playerDataBoolName` would
    /// never leave the shelf, and none of Sly's stock is shaped that way, so
    /// every cooked row carries one.
    pub fn listed(&self, index: usize, charms: &charms::State) -> bool {
        let item = Self::item(index);
        !self.bool_set(item.sets, charms)
            && item.requires.map_or(true, |f| self.bool_set(f, charms))
            && item.removed_by.map_or(true, |f| !self.bool_set(f, charms))
    }
    /// The rows of the current list that are on the shelf, in source order.
    pub fn shelf(&self, charms: &charms::State) -> impl Iterator<Item = usize> + '_ {
        let owned = *charms;
        self.stock(charms).iter().map(|&i| i as usize)
            .filter(move |&i| self.listed(i, &owned))
    }
    /// `ShopItemStats::OnEnable`: `runningCost` is the sheet price, discounted
    /// while the row is marked and Defender's Crest is worn.
    pub fn price(&self, index: usize, charms: &charms::State) -> u16 {
        let item = Self::item(index);
        if item.dung_discount && charms.equipped(DISCOUNT_CHARM) {
            discounted(item.cost)
        } else {
            item.cost
        }
    }
    /// `ShopMenuStock::CanBuy`, plus the shelf test the source gets for free by
    /// only ever offering rows `BuildItemList` put there.
    pub fn can_buy(&self, index: usize, geo: u32, charms: &charms::State) -> Result<u16, Refusal> {
        if !self.listed(index, charms) {
            return Err(Refusal::NotListed);
        }
        let price = self.price(index, charms);
        if geo < u32::from(price) {
            return Err(Refusal::TooPoor);
        }
        if charms_owned(charms) < u32::from(Self::item(index).charms_required) {
            return Err(Refusal::NotEnoughCharms);
        }
        Ok(price)
    }
    /// `Confirm Control` in its own order: set the item bool, take the Geo,
    /// then run the special-type branch. A refusal moves nothing at all.
    pub fn buy(&mut self, index: usize, geo: &mut u32, charms: &mut charms::State)
        -> Result<Receipt, Refusal> {
        let paid = self.can_buy(index, *geo, charms)?;
        let item = Self::item(index);
        self.set_bool(item.sets, charms);
        *geo -= u32::from(paid);
        let mut delivered = Delivered::default();
        match item.delivery {
            Delivery::Item => {}
            Delivery::MaskShard => {
                self.mask_shards += 1;
                if self.mask_shards >= SHARDS_PER_MASK {
                    self.mask_shards = 0;
                    // `Heart Container Control` awards AddToMaxHealth(1); the
                    // cap is the source's own maxHealthCap, not a port limit.
                    if self.masks < MASK_CAP {
                        self.masks += 1;
                        delivered.masks = 1;
                    }
                }
            }
            Delivery::Charm(charm) => {
                // The bool the transaction already set is `gotCharm_N`, so the
                // charm is owned before this branch runs; `charmsOwned` is the
                // count of that set and needs nothing stored here.
                delivered.charm = Some(charm);
            }
            Delivery::VesselFragment => {
                self.vessel_fragments += 1;
                if self.vessel_fragments >= FRAGMENTS_PER_VESSEL {
                    self.vessel_fragments = 0;
                    delivered.soul_reserve = SOUL_PER_VESSEL;
                }
            }
            Delivery::SimpleKey => {
                self.simple_keys += 1;
                delivered.simple_keys = 1;
            }
            Delivery::RancidEgg => {
                self.rancid_eggs += 1;
                delivered.rancid_eggs = 1;
            }
        }
        Ok(Receipt { paid, delivered })
    }
    /// The save record. `save::Save` does not carry it yet, so a purchase does
    /// not survive a quit; the shape is here so adopting it is a field, not a
    /// redesign. Charms are absent on purpose: the charm half of a purchase is
    /// already in `charms::State`'s own record.
    pub fn record(&self) -> (u32, [u8; 5]) {
        (self.slots, [self.mask_shards, self.vessel_fragments, self.masks,
                      self.simple_keys, self.rancid_eggs])
    }
    /// Refuse a record that contradicts itself rather than half-decoding it.
    pub fn restore(slots: u32, counters: [u8; 5]) -> Option<Self> {
        let [mask_shards, vessel_fragments, masks, simple_keys, rancid_eggs] = counters;
        let named = if FLAG_COUNT >= 32 { u32::MAX } else { (1u32 << FLAG_COUNT) - 1 };
        // A shard count that reached the fuse would already have fused, so a
        // record holding one is a record that never ran the delivery branch.
        if slots & !named != 0
            || mask_shards >= SHARDS_PER_MASK
            || vessel_fragments >= FRAGMENTS_PER_VESSEL
            || !(STARTING_MASKS..=MASK_CAP).contains(&masks) {
            return None;
        }
        Some(Self { slots, mask_shards, vessel_fragments, masks, simple_keys, rancid_eggs })
    }
}

/// The shelf as the player moves over it: a cursor, and the last refusal.
///
/// The source's own list scrolls a spawned row per item and `ShopItemStats`
/// colours a row it cannot afford; this keeps the cursor and the refusal and
/// leaves the drawing to whatever admits the scene, because there is no scene
/// to draw it in yet.
pub struct Screen {
    pub open: bool,
    pub row: usize,
    pub message: Option<Refusal>,
    pub last: Option<Receipt>,
}
// The charm panel's own pad bits, taken from it rather than restated: a shop
// list and a charm list that disagreed about which button confirms would be a
// bug no test of either one on its own could see.
use crate::charms::{CIRCLE as BACK, CROSS as CONFIRM, DOWN, UP};
impl Screen {
    pub const fn new() -> Self {
        Self { open: false, row: 0, message: None, last: None }
    }
    pub fn enter(&mut self) {
        self.open = true;
        self.row = 0;
        self.message = None;
        self.last = None;
    }
    /// One pressed-this-frame pad word. Returns true when the shop closed.
    pub fn step(&mut self, pressed: u16, geo: &mut u32, state: &mut State,
                charms: &mut charms::State) -> bool {
        let rows = state.shelf(charms).count();
        if rows == 0 || pressed & BACK != 0 {
            self.open = false;
            return true;
        }
        // A purchase shortens the shelf under the cursor, and so does arriving
        // with a bought row already gone.
        self.row = self.row.min(rows - 1);
        if pressed & DOWN != 0 {
            self.row = (self.row + 1) % rows;
            self.message = None;
        } else if pressed & UP != 0 {
            self.row = (self.row + rows - 1) % rows;
            self.message = None;
        }
        if pressed & CONFIRM != 0 {
            let index = state.shelf(charms).nth(self.row).expect("cursor is inside the shelf");
            match state.buy(index, geo, charms) {
                Ok(receipt) => {
                    self.last = Some(receipt);
                    self.message = None;
                    // The bought row left the shelf, so keeping the cursor
                    // where it was would move it onto whatever slid up.
                    self.row = self.row.saturating_sub(1);
                }
                Err(refusal) => self.message = Some(refusal),
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charms::State as Charms;

    /// The cooked row that sets a named PlayerData bool, so a test names an
    /// item the way the source does rather than by a position that could move.
    fn find(field: &str) -> usize {
        let slot = FLAG_NAMES.iter().position(|&n| n == field)
            .unwrap_or_else(|| panic!("{field} is not a cooked shop bool"));
        (0..ITEM_COUNT).find(|&i| ITEMS[i].sets == Flag::Slot(slot))
            .unwrap_or_else(|| panic!("no shop row sets {field}"))
    }
    fn rich() -> (u32, State, Charms) {
        (100_000, State::new(), Charms::new())
    }

    #[test]
    fn the_shelf_is_what_build_item_list_would_have_spawned() {
        let (_, state, charms) = rich();
        // Sly opens on the base list, and the second mask shard is behind the
        // first, so it is not on the shelf until the first is bought.
        assert_eq!(state.stock(&charms), BASE_STOCK);
        assert!(!state.listed(find("slyShellFrag2"), &charms), "a required bool must gate its row");
        assert!(state.listed(find("slyShellFrag1"), &charms));
        assert_eq!(state.shelf(&charms).count(), BASE_STOCK.len() - 1);
        assert!(state.shelf(&charms).all(|i| BASE_STOCK.contains(&(i as u8))));
    }

    #[test]
    fn the_alternate_list_arrives_with_slys_key() {
        let (_, state, charms) = rich();
        let mut owned = Charms::new();
        assert_eq!(state.stock(&charms), BASE_STOCK);
        for &flag in ALTERNATE_WHEN {
            let mut with = state;
            with.set_bool(flag, &mut owned);
            assert_eq!(with.stock(&charms), ALTERNATE_STOCK, "{flag:?} did not swap the list");
        }
        assert!(ALTERNATE_STOCK.len() > BASE_STOCK.len());
        // Everything the base list sold is still sold; the key only adds rows.
        for index in BASE_STOCK {
            assert!(ALTERNATE_STOCK.contains(index));
        }
    }

    #[test]
    fn a_purchase_pays_the_sheet_price_and_leaves_the_shelf() {
        let (mut geo, mut state, mut charms) = rich();
        let lantern = find("hasLantern");
        assert_eq!(State::item(lantern).cost, 1800, "the Lantern serializes 1500 and sells for 1800");
        let receipt = state.buy(lantern, &mut geo, &mut charms).expect("affordable");
        assert_eq!(receipt.paid, 1800);
        assert_eq!(geo, 100_000 - 1800);
        assert!(!state.listed(lantern, &charms), "a bought item is never listed again");
        assert_eq!(state.buy(lantern, &mut geo, &mut charms), Err(Refusal::NotListed));
        assert_eq!(geo, 100_000 - 1800, "a refused purchase moves no Geo");
    }

    #[test]
    fn insufficient_funds_refuse_before_anything_moves() {
        let (_, mut state, mut charms) = rich();
        let lantern = find("hasLantern");
        let mut geo = 1799;
        assert_eq!(state.buy(lantern, &mut geo, &mut charms), Err(Refusal::TooPoor));
        assert_eq!(geo, 1799);
        assert!(state.listed(lantern, &charms));
        geo = 1800;
        assert!(state.buy(lantern, &mut geo, &mut charms).is_ok());
        assert_eq!(geo, 0, "exactly the price is enough, as CanBuy's >= says");
    }

    #[test]
    fn a_charm_purchase_lands_in_the_charm_inventory() {
        let (mut geo, mut state, mut charms) = rich();
        let row = (0..ITEM_COUNT).find(|&i| matches!(ITEMS[i].delivery, Delivery::Charm(_)))
            .expect("Sly sells charms");
        let Delivery::Charm(charm) = State::item(row).delivery else { unreachable!() };
        assert!(!charms.owns(charm));
        let receipt = state.buy(row, &mut geo, &mut charms).expect("affordable");
        assert_eq!(receipt.delivered.charm, Some(charm));
        assert!(charms.owns(charm), "the transaction's own bool is gotCharm_N");
        assert_eq!(charms_owned(&charms), 1, "charmsOwned counts the set it just joined");
        // And the row is gone, because its bool and the charm's are the same one.
        assert!(!state.listed(row, &charms));
    }

    /// Every row of a list with a given delivery, in source order.
    fn rows_delivering(list: &'static [u8], kind: Delivery) -> impl Iterator<Item = usize> {
        list.iter().map(|&i| i as usize).filter(move |&i| State::item(i).delivery == kind)
    }

    #[test]
    fn shards_and_fragments_fuse_on_the_counts_the_source_fuses_on() {
        let (mut geo, mut state, mut charms) = rich();
        state.set_bool(ALTERNATE_WHEN[0], &mut charms);
        let shards = rows_delivering(ALTERNATE_STOCK, Delivery::MaskShard).count() as u8;
        assert_eq!(shards, SHARDS_PER_MASK, "Sly sells a whole mask across four rows");
        let mut masks = 0;
        for (bought, row) in rows_delivering(ALTERNATE_STOCK, Delivery::MaskShard).enumerate() {
            masks += state.buy(row, &mut geo, &mut charms).expect("affordable").delivered.masks;
            let last = bought as u8 + 1 == SHARDS_PER_MASK;
            assert_eq!(masks, u8::from(last), "a mask arrives only on the fourth shard");
            assert_eq!(state.mask_shards, if last { 0 } else { bought as u8 + 1 });
        }
        assert_eq!(state.masks, STARTING_MASKS + 1);
        let fragments = rows_delivering(ALTERNATE_STOCK, Delivery::VesselFragment).count() as u8;
        // Sly sells two of the three a vessel needs, so nothing fuses here.
        assert_eq!(fragments, FRAGMENTS_PER_VESSEL - 1);
        let mut soul = 0;
        for row in rows_delivering(ALTERNATE_STOCK, Delivery::VesselFragment) {
            soul += state.buy(row, &mut geo, &mut charms).expect("affordable").delivered.soul_reserve;
        }
        assert_eq!(soul, 0);
        assert_eq!(state.vessel_fragments, FRAGMENTS_PER_VESSEL - 1);
    }

    #[test]
    fn the_discount_truncates_the_way_the_float_cast_does() {
        // The cast truncates, so 999 discounts to 799 and not to 800. The cook
        // is what checks this against the float for every shipped price; these
        // pin the integer identity itself, including at the ends of the range.
        assert_eq!(discounted(100), 80);
        assert_eq!(discounted(999), 799);
        assert_eq!(discounted(1), 0);
        assert_eq!(discounted(u16::MAX), 52428);
        // Sly marks nothing with dungDiscount, so no price in this shop moves.
        assert!(!(0..ITEM_COUNT).any(|i| State::item(i).dung_discount));
        let (_, state, charms) = rich();
        for index in 0..ITEM_COUNT {
            assert_eq!(state.price(index, &charms), State::item(index).cost);
        }
    }

    #[test]
    fn the_cursor_survives_the_row_it_bought_and_a_cancel_closes() {
        let (mut geo, mut state, mut charms) = rich();
        let mut screen = Screen::new();
        screen.enter();
        let rows = state.shelf(&charms).count();
        screen.step(DOWN, &mut geo, &mut state, &mut charms);
        assert_eq!(screen.row, 1);
        screen.step(UP, &mut geo, &mut state, &mut charms);
        screen.step(UP, &mut geo, &mut state, &mut charms);
        assert_eq!(screen.row, rows - 1, "up from the first row wraps");
        screen.row = 0;
        geo = 0;
        screen.step(CONFIRM, &mut geo, &mut state, &mut charms);
        assert_eq!(screen.message, Some(Refusal::TooPoor));
        assert!(screen.last.is_none());
        assert_eq!(state.shelf(&charms).count(), rows, "a refusal sells nothing");
        geo = 100_000;
        let bought = state.shelf(&charms).next().expect("a first row");
        screen.step(CONFIRM, &mut geo, &mut state, &mut charms);
        assert!(screen.last.is_some() && screen.message.is_none());
        assert!(!state.listed(bought, &charms));
        // The first row is the first mask shard, and buying it sets the bool
        // the second shard requires, so one row leaves the shelf and the next
        // one arrives on the same frame. That is the source's own chain, not a
        // cursor bug, so the count is unchanged rather than one shorter.
        assert_eq!(bought, find("slyShellFrag1"));
        assert!(state.listed(find("slyShellFrag2"), &charms));
        assert_eq!(state.shelf(&charms).count(), rows);
        // A row with nothing behind it does shorten the shelf.
        let egg = state.shelf(&charms).position(|i| i == find("slyRancidEgg")).expect("on sale");
        screen.row = egg;
        screen.step(CONFIRM, &mut geo, &mut state, &mut charms);
        assert_eq!(state.shelf(&charms).count(), rows - 1);
        assert!(screen.step(BACK, &mut geo, &mut state, &mut charms) && !screen.open);
    }

    #[test]
    fn a_record_that_contradicts_itself_is_refused() {
        let (mut geo, mut state, mut charms) = rich();
        state.buy(find("slyRancidEgg"), &mut geo, &mut charms).expect("affordable");
        state.buy(find("slyShellFrag1"), &mut geo, &mut charms).expect("affordable");
        let (slots, counters) = state.record();
        let back = State::restore(slots, counters).expect("round trip");
        assert_eq!(back, state);
        assert_eq!(back.rancid_eggs, 1);
        // A shard count that should already have fused, and a bool with no name.
        assert_eq!(State::restore(slots, [SHARDS_PER_MASK, 0, STARTING_MASKS, 0, 0]), None);
        assert_eq!(State::restore(1 << FLAG_COUNT, counters), None);
        assert_eq!(State::restore(slots, [0, 0, MASK_CAP + 1, 0, 0]), None);
    }

    #[test]
    fn the_whole_shelf_is_a_bigger_geo_sink_than_the_admitted_slice() {
        // 1,177 Geo per clear of the admitted scenes against 230 of sinks is the
        // measured economy those scenes ship with. Sly alone is the other half.
        let total: u32 = ALTERNATE_STOCK.iter().map(|&i| u32::from(ITEMS[i as usize].cost)).sum();
        assert_eq!(total, 9260);
        assert!(total > 230 * 40);
    }
}
