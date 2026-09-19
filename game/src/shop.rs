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
//! Where it stands: `Room_shop` is admitted, so `SHOP_REGION` is the strip of
//! floor in front of the counter that Sly's own `Shop Region` FSM watches. That
//! FSM is the counter-side twin of `npc_control`, so the port answers it the
//! same way it answers an NPC: the prompt goes up inside the trigger, UP opens
//! the shelf and the shelf owns the pad until CIRCLE closes it.
//!
//! What this module still does not own: `Shop Region` also walks the Knight to
//! the counter, turns him, plays Sly's intro conversation and raises the shop
//! window art. None of that is reproduced; the trigger opens the shelf directly.
//! A mask shard or vessel fragment purchase is counted here and fuses here, but
//! nothing applies the mask or the SOUL reserve to the hero yet, because the
//! port has no PlayerData `maxHealthBase` for a purchase to move.
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
    pub const fn message(self) -> &'static str {
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

/// The shop's half of PlayerData, in the one place both the transaction and the
/// panel read it. `charms::state()` is the same shape and for the same reason:
/// `Confirm Control`'s write has to land where the shelf reads, and a second
/// copy passed down from `main` would be a copy that could disagree.
static mut STATE: State = State::new();
pub fn state() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}

/// The hero body inside `Shop Region`'s own trigger, which is the
/// `Trigger2dEvent` its `Out Of Range`/`In Range` pair answers.
pub fn in_region(scene: usize, body: [i32; 4]) -> bool {
    scene == SHOP_SCENE
        && SHOP_REGION[0] <= body[2] && SHOP_REGION[2] >= body[0]
        && SHOP_REGION[1] <= body[3] && SHOP_REGION[3] >= body[1]
}

/// One tick of the counter against the live wallet, returning the Geo the
/// purchase took. `wallet` is only what `CanBuy` compares against; the caller
/// moves the real wallet once, with `geo::World::take`, which is the port's
/// `HeroController::TakeGeo`. Keeping the deduction outside is what stops the
/// shelf and the wallet from both claiming to have charged for the same row.
pub fn tick(screen: &mut Screen, scene: usize, body: [i32; 4], eligible: bool,
            bits: u16, wallet: u32) -> u32 {
    let mut balance = wallet;
    let event = screen.step(in_region(scene, body), eligible, bits, &mut balance,
                            state(), crate::charms::state());
    event.bought.map_or(0, |receipt| u32::from(receipt.paid))
}

/// What one tick of the shelf did, in the terms `Shop Region` transitions in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Event {
    /// `Take Control`: the shelf went up and the Knight stopped being the
    /// player's to move.
    pub opened: bool,
    /// `Confirm Control` ran end to end. The Geo it took is `Receipt::paid`.
    pub bought: Option<Receipt>,
    /// `Regain Control`.
    pub closed: bool,
}

/// The shelf as the player moves over it: a cursor, the last refusal and the
/// prompt marker that stands over the counter before any of it opens.
///
/// The source's own list scrolls a spawned row per item and `ShopItemStats`
/// colours a row it cannot afford; this keeps the cursor and the refusal, and
/// `presentation::panel` draws them in the port's shared text panel.
pub struct Screen {
    pub open: bool,
    pub row: usize,
    pub message: Option<Refusal>,
    pub last: Option<Receipt>,
    /// `In Range`: the hero body is inside the trigger and the prompt is up.
    pub prompt: bool,
    previous: u16,
    release: bool,
}
// The charm panel's own pad bits, taken from it rather than restated: a shop
// list and a charm list that disagreed about which button confirms would be a
// bug no test of either one on its own could see.
use crate::charms::{CIRCLE as BACK, CROSS as CONFIRM, DOWN, UP};
/// The nail, which the shelf never reads but must not release into. Held with
/// the rest below so a press that closed the shop cannot also swing it, which
/// is the same hold `npc_state::Convo` keeps for the conversation panel.
const NAIL: u16 = 0x8000;
const ACTIONS: u16 = UP | DOWN | CONFIRM | BACK | NAIL;
impl Screen {
    pub const fn new() -> Self {
        Self { open: false, row: 0, message: None, last: None, prompt: false,
               previous: 0, release: false }
    }
    /// True while the shelf, or the press that closed it, owns the pad.
    pub fn consumes_actions(&self) -> bool {
        self.open || self.release
    }
    /// Put the shelf away and hold whatever was pressed until it is released,
    /// the way `dialogue::cancel` does for a conversation panel.
    fn cancel(&mut self) {
        self.open = false;
        self.prompt = false;
        self.message = None;
        self.release = self.previous & ACTIONS != 0;
    }
    /// One tick of `Shop Region`. `in_region` is the hero body inside the
    /// counter trigger, `eligible` is the same grounded-and-unlocked test the
    /// NPC prompt answers, and `bits` is the held pad word rather than a
    /// pressed one, because the release hold has to watch a button stay down.
    pub fn step(&mut self, in_region: bool, eligible: bool, bits: u16, geo: &mut u32,
                state: &mut State, charms: &mut charms::State) -> Event {
        let pressed = bits & !self.previous;
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
        let mut event = Event::default();
        if !self.open {
            self.prompt = in_region && eligible && !self.release;
            // `In Range` listens for UP alone, exactly as npc_control does.
            if self.prompt && pressed & UP != 0 {
                self.open = true;
                self.row = 0;
                self.message = None;
                self.last = None;
                self.prompt = false;
                event.opened = true;
            }
            return event;
        }
        let rows = state.shelf(charms).count();
        // `BuildItemList` spawns no row when nothing is listed, so a shelf the
        // player has emptied closes rather than standing there blank. Leaving
        // the trigger closes it too: the Knight cannot walk out while the shelf
        // owns him, so the only way out of it is a death, a hazard respawn or a
        // scene change, and each of those should take the shelf with it the way
        // `dialogue::cancel` takes a conversation.
        if rows == 0 || !in_region || pressed & BACK != 0 {
            self.cancel();
            event.closed = true;
            return event;
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
                    event.bought = Some(receipt);
                    // The bought row left the shelf, so keeping the cursor
                    // where it was would move it onto whatever slid up.
                    self.row = self.row.saturating_sub(1);
                }
                Err(refusal) => self.message = Some(refusal),
            }
        }
        event
    }
}

/// Rows of the shelf on screen at once. The list pages rather than scrolling
/// under a fixed cursor, the way the charm board's does.
pub const VISIBLE: usize = 6;
const FOOTER: &str = "X: buy    O: leave";
/// Drawn glyphs, which is what the shared panel budget counts: `dialogue::text`
/// spends a slot per non-space byte.
const fn glyphs(text: &str) -> usize {
    let (bytes, mut i, mut drawn) = (text.as_bytes(), 0, 0);
    while i < bytes.len() {
        if bytes[i] != b' ' {
            drawn += 1;
        }
        i += 1;
    }
    drawn
}
const fn widest_name() -> usize {
    let (mut i, mut most) = (0, 0);
    while i < ITEM_COUNT {
        let width = glyphs(ITEMS[i].name);
        if width > most {
            most = width;
        }
        i += 1;
    }
    most
}
const fn widest_description() -> usize {
    let (mut i, mut most) = (0, 0);
    while i < ITEM_COUNT {
        let (mut line, mut total) = (0, 0);
        while line < ITEMS[i].lines.len() {
            total += glyphs(ITEMS[i].lines[line]);
            line += 1;
        }
        if total > most {
            most = total;
        }
        i += 1;
    }
    most
}
const fn widest_footer() -> usize {
    // Every refusal replaces the footer, so the budget holds the widest of them.
    let all = [Refusal::NotListed, Refusal::TooPoor, Refusal::NotEnoughCharms];
    let (mut i, mut most) = (0, glyphs(FOOTER));
    while i < all.len() {
        let width = glyphs(all[i].message());
        if width > most {
            most = width;
        }
        i += 1;
    }
    most
}
/// A loose upper bound on one frame of the panel: the widest description, plus
/// `VISIBLE` rows all as wide as the widest name with a cursor and a five digit
/// price, plus the heading and the footer.
const PANEL_GLYPHS: usize = widest_description()
    + VISIBLE * (widest_name() + 1 + 5)
    + glyphs(SHOP_PROMPT) + widest_footer();
/// `dialogue::prepare` has already drawn the HUD wallet by the time the panel
/// composes, and `dialogue::text` panics rather than truncating, so the two
/// together have to fit. A longer cooked description would fail here instead of
/// on whichever row a player happened to select.
const HUD_GLYPHS: usize = glyphs("Geo") + 10;
const _: () = assert!(PANEL_GLYPHS + HUD_GLYPHS <= crate::dialogue::CAP);

#[cfg(not(test))]
mod presentation {
    use super::*;
    use crate::dialogue::{panel_frame, panel_number, panel_prompt, panel_text, panel_width};
    /// The shelf, or the prompt marker that stands over the counter before it
    /// opens. Composes after `dialogue::prepare`, which leaves the box count at
    /// zero whenever no tablet, conversation or bench prompt is up, so the
    /// frame below is the only one on screen. It has already drawn the wallet
    /// inside this box, at the HUD's own place, so the shelf does not draw a
    /// second copy of the number the player is about to spend.
    // Never inlined: `main` is one 611-line function whose generated code sits
    // against the 128 KB a MIPS PC16 branch reaches, and absorbing a body this
    // size pushes a branch out of range. The assembler reports only "out of
    // range PC16 fixup" when that happens, naming nothing.
    #[inline(never)]
pub fn prepare(screen: &Screen, camera: (i32, i32), paused: bool) {
        if paused {
            return;
        }
        if !screen.open {
            if screen.prompt {
                panel_prompt(camera, SHOP_MARKER, SHOP_PROMPT);
            }
            return;
        }
        let state = state();
        let charms = crate::charms::state();
        // The charm board's box, because the shelf is the same shape of list
        // and the HUD it has to share the screen with has not moved.
        panel_frame(14, 28, 292, 204);
        panel_text(160 - panel_width(SHOP_PROMPT) / 2, 36, SHOP_PROMPT);
        let top = screen.row / VISIBLE * VISIBLE;
        let mut selected = None;
        for (offset, index) in state.shelf(charms).skip(top).take(VISIBLE).enumerate() {
            let y = 64 + offset as i16 * 12;
            if top + offset == screen.row {
                panel_text(26, y, ">");
                selected = Some(index);
            }
            panel_text(38, y, State::item(index).name);
            // The listed price, so Defender's Crest would show its discount
            // rather than the sheet value the row was cooked with.
            panel_number(268, y, u32::from(state.price(index, charms)));
        }
        if let Some(index) = selected {
            for (i, line) in State::item(index).lines.iter().enumerate() {
                panel_text(28, 140 + i as i16 * 11, line);
            }
        }
        let footer = match screen.message {
            Some(refusal) => refusal.message(),
            None => FOOTER,
        };
        panel_text(160 - panel_width(footer) / 2, 214, footer);
    }
}
#[cfg(not(test))]
pub use presentation::prepare;

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

    /// One frame at the counter, with the pad word held rather than pressed.
    fn tick(screen: &mut Screen, bits: u16, geo: &mut u32, state: &mut State,
            charms: &mut Charms) -> Event {
        screen.step(true, true, bits, geo, state, charms)
    }
    /// Standing in the trigger and pressing UP, which is the whole way in.
    fn open(screen: &mut Screen, geo: &mut u32, state: &mut State, charms: &mut Charms) -> Event {
        tick(screen, 0, geo, state, charms);
        tick(screen, UP, geo, state, charms)
    }

    #[test]
    fn the_counter_trigger_raises_the_prompt_and_up_opens_the_shelf() {
        let (mut geo, mut state, mut charms) = rich();
        let mut screen = Screen::new();
        // Out of the trigger there is no prompt, and UP there opens nothing.
        assert!(!screen.step(false, true, UP, &mut geo, &mut state, &mut charms).opened);
        assert!(!screen.prompt && !screen.open);
        // Inside it but not free to act: `can_inspect` is what the NPC prompt
        // answers too, so a Knight mid-attack or mid-fall gets no prompt.
        screen.step(true, false, 0, &mut geo, &mut state, &mut charms);
        assert!(!screen.prompt);
        screen.step(true, true, 0, &mut geo, &mut state, &mut charms);
        assert!(screen.prompt && !screen.open);
        assert!(tick(&mut screen, UP, &mut geo, &mut state, &mut charms).opened);
        assert!(screen.open && !screen.prompt && screen.consumes_actions());
    }

    #[test]
    fn the_cooked_trigger_is_the_one_the_hero_body_can_stand_in() {
        // Q16 world units, the same as world::GATES. The strip is the floor in
        // front of the counter, so a hero body standing on that floor overlaps
        // it and one standing a room away does not.
        assert_eq!(SHOP_SCENE, 45);
        assert!(SHOP_REGION[0] < SHOP_REGION[2] && SHOP_REGION[1] < SHOP_REGION[3]);
        let at = |x: i32, y: i32| [x - 16384, y - 91136, x + 16384, y - 7168];
        let middle = (SHOP_REGION[0] + SHOP_REGION[2]) / 2;
        // The Room_shop floor is at world y 5, so a Knight standing on it has
        // his feet there and his body reaches up past the strip.
        assert!(in_region(SHOP_SCENE, at(middle, 5 * 65536 + 91136)));
        assert!(!in_region(SHOP_SCENE + 1, at(middle, 5 * 65536 + 91136)));
        assert!(!in_region(SHOP_SCENE, at(SHOP_REGION[0] - 65536, 5 * 65536 + 91136)));
        // And a Knight four world units above the floor is over it, not in it.
        assert!(!in_region(SHOP_SCENE, at(middle, 9 * 65536 + 91136)));
    }

    #[test]
    fn the_cursor_survives_the_row_it_bought_and_a_cancel_closes() {
        let (mut geo, mut state, mut charms) = rich();
        let mut screen = Screen::new();
        open(&mut screen, &mut geo, &mut state, &mut charms);
        let rows = state.shelf(&charms).count();
        tick(&mut screen, DOWN, &mut geo, &mut state, &mut charms);
        assert_eq!(screen.row, 1);
        tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
        tick(&mut screen, UP, &mut geo, &mut state, &mut charms);
        tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
        tick(&mut screen, UP, &mut geo, &mut state, &mut charms);
        assert_eq!(screen.row, rows - 1, "up from the first row wraps");
        screen.row = 0;
        geo = 0;
        tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
        let refused = tick(&mut screen, CONFIRM, &mut geo, &mut state, &mut charms);
        assert_eq!(screen.message, Some(Refusal::TooPoor));
        assert!(refused.bought.is_none() && screen.last.is_none());
        assert_eq!(state.shelf(&charms).count(), rows, "a refusal sells nothing");
        geo = 100_000;
        let bought = state.shelf(&charms).next().expect("a first row");
        tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
        let sale = tick(&mut screen, CONFIRM, &mut geo, &mut state, &mut charms);
        assert!(sale.bought.is_some() && screen.last.is_some() && screen.message.is_none());
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
        tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
        tick(&mut screen, CONFIRM, &mut geo, &mut state, &mut charms);
        assert_eq!(state.shelf(&charms).count(), rows - 1);
        tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
        assert!(tick(&mut screen, BACK, &mut geo, &mut state, &mut charms).closed);
        assert!(!screen.open);
    }

    #[test]
    fn the_press_that_closed_the_shelf_cannot_also_reopen_it_or_swing_the_nail() {
        let (mut geo, mut state, mut charms) = rich();
        let mut screen = Screen::new();
        open(&mut screen, &mut geo, &mut state, &mut charms);
        // Closing with CIRCLE and the nail held: both stay consumed until every
        // action button is released, so standing in the trigger does not put
        // the shelf straight back up and the close does not reach the nail.
        assert!(tick(&mut screen, BACK | NAIL, &mut geo, &mut state, &mut charms).closed);
        assert!(screen.consumes_actions() && !screen.open);
        assert!(!tick(&mut screen, UP | NAIL, &mut geo, &mut state, &mut charms).opened);
        assert!(!screen.prompt, "the prompt waits for the release too");
        tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
        assert!(!screen.consumes_actions() && screen.prompt);
        assert!(tick(&mut screen, UP, &mut geo, &mut state, &mut charms).opened);
    }

    #[test]
    fn an_emptied_shelf_closes_instead_of_standing_there_blank() {
        let (mut geo, mut state, mut charms) = rich();
        let mut screen = Screen::new();
        open(&mut screen, &mut geo, &mut state, &mut charms);
        // Buy the base list out. Each purchase can add the row behind it, so
        // this keeps going until BuildItemList would spawn nothing.
        for _ in 0..BASE_STOCK.len() * 2 {
            if !screen.open {
                break;
            }
            screen.row = 0;
            tick(&mut screen, 0, &mut geo, &mut state, &mut charms);
            tick(&mut screen, CONFIRM, &mut geo, &mut state, &mut charms);
        }
        assert_eq!(state.shelf(&charms).count(), 0);
        assert!(!screen.open, "an empty shelf is a closed shop");
    }

    #[test]
    fn the_panel_fits_the_shared_glyph_budget_with_room_left() {
        // The const assertion above is what fails a build; this reports the
        // headroom, because a cooked description is what would eat it.
        assert!(PANEL_GLYPHS + HUD_GLYPHS <= crate::dialogue::CAP,
                "{PANEL_GLYPHS} glyphs against {}", crate::dialogue::CAP);
        assert_eq!(widest_name(), glyphs("Vessel Fragment"));
        assert!(VISIBLE <= BASE_STOCK.len());
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
