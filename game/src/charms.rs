//! The charm collection, the notch board and the effects the port can express.
//!
//! The rules here are `UI Charms`, the source inventory's own FSM, and nothing
//! else. `Slot Open?` refuses to begin an equip unless `charmSlotsFilled <
//! charmSlots`, so a full board never overcharms. `Check Points` then spends
//! `charmCost_N` and branches when the total passes `charmSlots`. `Overcharm
//! Check` owns that branch: with `canOvercharm` the equip goes through and the
//! save is marked overcharmed, and without it the attempt is counted and
//! refused, `Fail Back` hands the notches back, and the fifth attempt sets
//! `canOvercharm` and lets it through. Unequipping runs `Return Points` and
//! `End Overcharm?`, which clears the flag only once the notches fit again,
//! the same predicate `GameManager::RefreshOvercharm` uses.
//!
//! A charm whose effect the port has no system for is refused rather than
//! equipped for nothing: `host/charms.py` cooks `Effect::None` for it and
//! records the missing system in .hkpsx/charm-catalog.json. The six that are
//! implemented compose onto `VITAL_PARAMS` through `vitals`, which
//! `cheats::Settings::params` calls, so every consumer of the live parameters
//! sees them without each one applying the bonus itself.
use hk_sim::{VitalParams, Vitals};

/// One cooked charm. `text` is its localized description, already wrapped to
/// the panel by the host against the same glyph advances the guest draws with:
/// the lines are joined by newlines, and an empty line is a paragraph break.
pub struct Charm {
    pub name: &'static str,
    pub cost: u8,
    pub text: &'static str,
    pub effect: Effect,
    /// Charms 23 to 25, which `Broken?` refuses while `brokenCharm_N` is set.
    pub fragile: bool,
    /// Charm 36, which `Royal?` refuses in its bound quest states.
    pub bound: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
pub enum Effect {
    /// Catalogued but not expressible here; the cooked report says which system
    /// is missing. Equipping is refused so nothing silently does nothing.
    None,
    /// Stalwart Shell: `INVUL_TIME_STAL` and `RECOIL_DURATION_STAL`.
    Shell {
        invulnerable_ticks: u16,
        hazard_invulnerable_ticks: u16,
        recoil_ticks: u16,
    },
    /// Soul Catcher and Soul Eater, which `HeroController::SoulGain` adds.
    SoulPerHit(u16),
    /// Fragile Heart: `CharmUpdate` raises `maxHealth` over `maxHealthBase`.
    MaxHealth(u16),
    /// Fragile Strength: `Set Slash Damage` truncates nail damage times 1.5.
    NailScale { numerator: u16, denominator: u16 },
    /// Grubsong: `TakeDamageCharmEffects` charges SOUL on a damaging hit.
    SoulOnDamage {
        alone: u16,
        combo: u16,
        combo_charm: u8,
    },
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/charms.rs"));

/// Why the inventory refused, in the terms the source FSM refuses in. The port
/// adds only `NoEffect`, for a charm it has no system to honour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotOwned,
    /// `Slot Open?`: the board is already full, so not even overcharm applies.
    NoOpenNotch,
    /// `Overcharm Check` refused this attempt; the notches were handed back.
    Overcharm(u8),
    Broken,
    Bound,
    NoEffect,
}
impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::NotOwned => "Not found yet",
            Refusal::NoOpenNotch => "No open notch",
            Refusal::Overcharm(_) => "It will not fit",
            Refusal::Broken => "This charm is broken",
            // Short enough for the board's description column (`COL_W`).
            Refusal::Bound => "Cannot be removed",
            Refusal::NoEffect => "No effect in this port yet",
        }
    }
}

/// The charm half of PlayerData: `gotCharm_N`, `equippedCharm_N`, `charmSlots`,
/// `canOvercharm` and the derived `charmSlotsFilled`/`overcharmed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    owned: psx_math::Mask64,
    equipped: psx_math::Mask64,
    broken: psx_math::Mask64,
    pub notches: u8,
    pub can_overcharm: bool,
    /// `UI Charms`' own `Overcharm Attempts`. It is an FSM variable in the
    /// source, not PlayerData, so it lasts the session and is never saved.
    attempts: u8,
}
const fn bit(charm: usize) -> psx_math::Mask64 {
    psx_math::Mask64::bit(charm - 1)
}
impl State {
    pub const fn new() -> Self {
        Self {
            owned: psx_math::Mask64::EMPTY,
            equipped: psx_math::Mask64::EMPTY,
            broken: psx_math::Mask64::EMPTY,
            notches: STARTING_NOTCHES,
            can_overcharm: false,
            attempts: 0,
        }
    }
    pub fn charm(charm: usize) -> &'static Charm {
        &CHARMS[charm - 1]
    }
    pub fn owns(&self, charm: usize) -> bool {
        !self.owned.and(bit(charm)).is_empty()
    }
    pub fn equipped(&self, charm: usize) -> bool {
        !self.equipped.and(bit(charm)).is_empty()
    }
    pub fn broken(&self, charm: usize) -> bool {
        !self.broken.and(bit(charm)).is_empty()
    }
    pub fn give(&mut self, charm: usize) {
        self.owned = self.owned.or(bit(charm));
    }
    /// `PlayerData::CalculateNotchesUsed`: the cost of everything equipped.
    /// Derived rather than stored so a save can never disagree with itself.
    pub fn filled(&self) -> u8 {
        (1..=CHARM_COUNT)
            .filter(|&n| self.equipped(n))
            .map(|n| Self::charm(n).cost)
            .sum()
    }
    /// `GameManager::RefreshOvercharm`.
    pub fn overcharmed(&self) -> bool {
        self.filled() > self.notches
    }
    /// What a hit takes: `HeroController::TakeDamage` hands `PlayerData.TakeHealth` twice the damage while
    /// `overcharmed` is set (more notches spent than the board has), blue health first as for any hit.
    pub fn damage_taken(&self, damage: u16) -> u16 {
        if self.overcharmed() {
            damage.saturating_mul(2)
        } else {
            damage
        }
    }
    fn refuse(&self, charm: usize) -> Option<Refusal> {
        let c = Self::charm(charm);
        if !self.owns(charm) {
            return Some(Refusal::NotOwned);
        }
        // Source order: `Broken?` then `Royal?` then `Equipped?`.
        if c.fragile && self.broken(charm) {
            return Some(Refusal::Broken);
        }
        if c.bound {
            return Some(Refusal::Bound);
        }
        None
    }
    /// One `UI CONFIRM` on a charm: equip it, or unequip it if it is worn.
    pub fn toggle(&mut self, charm: usize) -> Result<bool, Refusal> {
        if let Some(refusal) = self.refuse(charm) {
            return Err(refusal);
        }
        if self.equipped(charm) {
            self.unequip(charm);
            return Ok(false);
        }
        self.equip(charm).map(|()| true)
    }
    fn equip(&mut self, charm: usize) -> Result<(), Refusal> {
        let c = Self::charm(charm);
        if c.effect == Effect::None {
            return Err(Refusal::NoEffect);
        }
        if self.filled() >= self.notches {
            return Err(Refusal::NoOpenNotch);
        }
        if self.filled() + c.cost > self.notches && !self.can_overcharm {
            self.attempts = self.attempts.saturating_add(1);
            if self.attempts < OVERCHARM_BREAK_ATTEMPT {
                return Err(Refusal::Overcharm(self.attempts));
            }
            // `Break` unlocks overcharming and the same attempt goes through.
            self.can_overcharm = true;
        }
        self.equipped = self.equipped.or(bit(charm));
        Ok(())
    }
    fn unequip(&mut self, charm: usize) {
        self.equipped = self.equipped.and_not(bit(charm));
    }
    /// Everything the memory card keeps. `filled` and `overcharmed` are derived
    /// and the attempt count is deliberately session-only.
    pub fn record(&self) -> (psx_math::Mask64, psx_math::Mask64, u8, bool) {
        (self.owned, self.equipped, self.notches, self.can_overcharm)
    }
    /// Reject a record whose equipped set is not a subset of what it owns, or
    /// whose notch count cannot hold it without the overcharm it did not earn.
    pub fn restore(
        owned: psx_math::Mask64,
        equipped: psx_math::Mask64,
        notches: u8,
        can_overcharm: bool,
    ) -> Option<Self> {
        let mask = psx_math::Mask64::first(CHARM_COUNT);
        if !owned.and_not(mask).is_empty() || !equipped.and_not(owned).is_empty() || notches == 0 {
            return None;
        }
        let state = Self {
            owned,
            equipped,
            broken: psx_math::Mask64::EMPTY,
            notches,
            can_overcharm,
            attempts: 0,
        };
        if state.overcharmed() && !can_overcharm {
            return None;
        }
        Some(state)
    }
}

static mut STATE: State = State::new();
#[no_mangle]
pub static mut HK_CHARMS_OWNED: u32 = 0;
#[no_mangle]
pub static mut HK_CHARMS_EQUIPPED: u32 = 0;
#[no_mangle]
pub static mut HK_CHARM_NOTCHES: u32 = 0;
#[no_mangle]
pub static mut HK_CHARM_OVERCHARMED: u32 = 0;

pub fn state() -> &'static mut State {
    unsafe { &mut *core::ptr::addr_of_mut!(STATE) }
}
fn publish() {
    let s = state();
    unsafe {
        HK_CHARMS_OWNED = (1..=CHARM_COUNT).filter(|&n| s.owns(n)).count() as u32;
        HK_CHARMS_EQUIPPED = (1..=CHARM_COUNT).filter(|&n| s.equipped(n)).count() as u32;
        HK_CHARM_NOTCHES = u32::from(s.filled()) | (u32::from(s.notches) << 8);
        HK_CHARM_OVERCHARMED = u32::from(s.overcharmed());
    }
}
/// `Get Charm`: a charm picked up in the world.
pub fn pick_up(charm: usize) {
    state().give(charm);
    publish();
}
/// Load a card record, or start a fresh collection when it does not validate.
pub fn load(
    owned: psx_math::Mask64,
    equipped: psx_math::Mask64,
    notches: u8,
    can_overcharm: bool,
) -> bool {
    let restored = State::restore(owned, equipped, notches, can_overcharm);
    *state() = restored.unwrap_or_else(State::new);
    publish();
    restored.is_some()
}
/// The profile's board plus whatever the boot Cheats page already asked for.
pub fn boot(saved: Option<(psx_math::Mask64, psx_math::Mask64, u8, bool)>, grant: bool) -> bool {
    let loaded = match saved {
        Some((owned, equipped, notches, over)) => load(owned, equipped, notches, over),
        None => {
            *state() = State::new();
            publish();
            true
        }
    };
    if grant {
        grant_all(true);
    }
    loaded
}
pub fn record() -> (psx_math::Mask64, psx_math::Mask64, u8, bool) {
    state().record()
}
/// What the cheat added, so turning it off takes back only that and never a
/// charm the save already owned.
static mut GRANTED: psx_math::Mask64 = psx_math::Mask64::EMPTY;
/// The cheat grant, kept out of the equipped set: it only marks charms found,
/// so equipping still runs the same notch board a real pickup would.
pub fn grant_all(on: bool) {
    let s = state();
    if on {
        let mut added = psx_math::Mask64::EMPTY;
        for charm in 1..=CHARM_COUNT {
            if !s.owns(charm) {
                added = added.or(bit(charm));
                s.give(charm);
            }
        }
        unsafe { GRANTED = GRANTED.or(added) };
    } else {
        let taken = core::mem::replace(
            unsafe { &mut *core::ptr::addr_of_mut!(GRANTED) },
            psx_math::Mask64::EMPTY,
        );
        s.owned = s.owned.and_not(taken);
        // Dropping the grant must not leave a charm worn that is no longer owned.
        s.equipped = s.equipped.and(s.owned);
    }
    publish();
}

/// The equipped charms composed onto the cooked no-charm parameters, in charm
/// order. `cheats::Settings::params` calls this, so this is the one place a
/// charm bonus is applied and no consumer applies it a second time.
pub fn vitals(base: VitalParams) -> VitalParams {
    compose(state(), base)
}
fn compose(s: &State, mut base: VitalParams) -> VitalParams {
    // Nothing equipped is the overwhelmingly common case and it is on a very
    // hot path: `params` is called several times a tick, by `hurt`, `maintain`,
    // `new_vitals` and the enemy soul gain. Walking all 40 charms there cost
    // enough per tick to move kings-return 3.2 units and two masks, because a
    // frame that slips past a VBlank boundary changes how many simulation ticks
    // fall between two input polls. The route's combat was identical; only its
    // timing moved. Nothing below this line runs for an empty board.
    if s.equipped.is_empty() {
        return base;
    }
    for charm in 1..=CHARM_COUNT {
        if !s.equipped(charm) {
            continue;
        }
        match State::charm(charm).effect {
            Effect::Shell {
                invulnerable_ticks,
                hazard_invulnerable_ticks,
                recoil_ticks,
            } => {
                base.invulnerable_ticks = invulnerable_ticks;
                base.hazard_invulnerable_ticks = hazard_invulnerable_ticks;
                base.recoil_ticks = recoil_ticks;
            }
            Effect::SoulPerHit(extra) => {
                base.soul_per_hit = base.soul_per_hit.saturating_add(extra)
            }
            Effect::MaxHealth(extra) => base.max_health = base.max_health.saturating_add(extra),
            // `(int)((float)damage * 1.5f)` truncates, so integer division does too.
            Effect::NailScale {
                numerator,
                denominator,
            } => {
                base.nail_damage = base.nail_damage.saturating_mul(numerator) / denominator;
            }
            Effect::None | Effect::SoulOnDamage { .. } => {}
        }
    }
    base
}
/// `HeroController::TakeDamageCharmEffects`, called after a hit that landed.
/// A hit's damage at the board's present state (doubled while overcharmed).
pub fn damage_taken(damage: u16) -> u16 {
    state().damage_taken(damage)
}
pub fn on_damage(vitals: &mut Vitals, params: VitalParams) {
    charge_on_damage(state(), vitals, params);
}
fn charge_on_damage(s: &State, vitals: &mut Vitals, params: VitalParams) {
    for charm in 1..=CHARM_COUNT {
        if !s.equipped(charm) {
            continue;
        }
        if let Effect::SoulOnDamage {
            alone,
            combo,
            combo_charm,
        } = State::charm(charm).effect
        {
            let amount = if s.equipped(combo_charm as usize) {
                combo
            } else {
                alone
            };
            vitals.add_soul(params, amount);
        }
    }
}

// The board's own pad bits. `pause` holds its copy to these, so the two panels
// sharing one pad cannot silently answer different buttons.
pub const UP: u16 = 0x10;
pub const RIGHT: u16 = 0x20;
pub const DOWN: u16 = 0x40;
pub const LEFT: u16 = 0x80;
pub const CIRCLE: u16 = 0x2000;
pub const CROSS: u16 = 0x4000;
/// The cursor (a charm's index, 0 to 39) and the last refusal on the pause charm screen.
pub struct Screen {
    pub open: bool,
    pub row: usize,
    pub message: Option<Refusal>,
}
impl Screen {
    pub const fn new() -> Self {
        Self {
            open: false,
            row: 0,
            message: None,
        }
    }
    pub fn enter(&mut self) {
        self.open = true;
        self.row = 0;
        self.message = None;
    }
    /// One edge-triggered step. Returns true when the screen has closed.
    pub fn step(&mut self, press: u16) -> bool {
        if press & CIRCLE != 0 {
            self.open = false;
            return true;
        }
        // The board is a grid of GRID_COLS slots a row, as the source's is: left and right step a
        // slot (and wrap through the whole collection), up and down a row (and wrap round the
        // columns, so the cursor stays in its column).
        let before = self.row;
        if press & LEFT != 0 {
            self.row = (self.row + CHARM_COUNT - 1) % CHARM_COUNT;
        }
        if press & RIGHT != 0 {
            self.row = (self.row + 1) % CHARM_COUNT;
        }
        if press & UP != 0 {
            self.row = (self.row + CHARM_COUNT - GRID_COLS) % CHARM_COUNT;
        }
        if press & DOWN != 0 {
            self.row = (self.row + GRID_COLS) % CHARM_COUNT;
        }
        if self.row != before {
            self.message = None;
        }
        if press & CROSS != 0 {
            self.message = state().toggle(self.row + 1).err();
            publish();
        }
        false
    }
}

#[cfg(not(test))]
mod presentation {
    use super::*;
    use crate::dialogue::{panel_text, panel_text_gain, panel_width};
    use crate::dialogue_art::{
        DIALOGUE_BOARD_NOTCH_LIT, DIALOGUE_BOARD_ORB, DIALOGUE_BOARD_SLOT,
    };
    use psx_gpu::{
        material::{BlendMode, TextureMaterial},
        ot::OrderingTable,
        prim::QuadTextured,
    };
    use psx_vram::{upload_bytes, Clut, VramRect};
    static ICON_DATA: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../data/charm-icons.hk"
    ));
    /// Animation-cache keys above the ability clips', which are themselves
    /// above the Shade's. A pause screen is reachable from every view, so these
    /// icons can no more ride in a room atlas than the Shade can: they stay in
    /// linked RAM and reach VRAM through the shared 64x64 slots on demand. The
    /// board shows every slot at once, so the icons travel sixteen to a slot
    /// (`ICON_SHEETS` keys, not one a charm).
    pub const KEY_BASE: u16 =
        crate::ability_art::KEY_BASE + crate::ability_art::FRAME_COUNT as u16;
    /// Keys the sheets take.
    pub const KEYS: usize = ICON_SHEETS;
    const _: () = assert!(KEY_BASE as usize + KEYS <= hk_cache::MAX_KEYS);
    const _: () = assert!(
        SHEET_BYTES <= 2048,
        "a sheet has to reach VRAM in one slot upload"
    );
    const _: () = assert!(ICON_PX as usize * ICONS_PER_SHEET_ROW == 64);
    // The board's own claim on the cache: the sheets and its four art tiles. It is not the
    // whole story: the frozen view behind the panel keeps requesting its enemies, whose cap is
    // larger than what is left, so `append_needed` yields rather than asserting.
    const _: () = assert!(KEYS + 4 + hk_cache::RESERVED_SLOTS <= hk_cache::MAX_REQUESTS);
    const PER_SHEET: usize = ICONS_PER_SHEET_ROW * ICONS_PER_SHEET_ROW;
    const _: () = assert!(ICON_SHEETS * PER_SHEET >= CHARM_COUNT);
    /// Frames the board wanted its icons for and could not have, because the view
    /// behind it had already claimed every request the cache takes in a frame.
    #[no_mangle]
    pub static mut HK_CHARM_ICONS_CROWDED: u32 = 0;
    /// Whether `append_needed` declared the sheets to the animation cache this
    /// frame. `panel` draws icons only then, so a board that opened after the
    /// working set was fixed draws none that frame instead of asking the cache
    /// for a slot it was never given.
    static mut PREPARED: bool = false;
    /// Worn charms the top row seats (`ROW_X` to the right margin, `ICON_PITCH` apart).
    const EQUIPPED_SLOTS: usize = 11;
    /// A grid slot's icon for every charm found, the worn ones a second time on top, and the
    /// chosen charm's picture in the right-hand column.
    const MAX_ICONS: usize = CHARM_COUNT + EQUIPPED_SLOTS + 1;
    static mut ICONS: [QuadTextured; MAX_ICONS] =
        [const { QuadTextured::new([(0, 0); 4], [(0, 0); 4], 0, 0, (128, 128, 128)) }; MAX_ICONS];
    static mut ICON_COUNT: usize = 0;
    /// The worn set the last board frame showed, and the charm whose icon is travelling: (charm,
    /// the vblank it set off, being worn). Wearing or taking off a charm sends its icon between its
    /// slot in the collection and the worn row in 0.25 s (`UI Charms`: Tween Up / Tween Down,
    /// iTweenMoveTo time 0.25, easeOutSine; here the quadratic ease-out).
    static mut LAST_WORN: psx_math::Mask64 = psx_math::Mask64::EMPTY;
    static mut FLY: (u8, u32, bool) = (0, 0, false);
    const FLY_VBLANKS: u32 = 15;

    /// One resident CLUT row per cooked palette, above the four the ability
    /// clips took out of the block host/shade.py reserved at y482.
    #[inline(never)]
    pub fn upload() {
        assert!(
            ICON_DATA.len() == ICON_PALETTE_BYTES + CHARM_COUNT * ICON_BYTES
                && ICON_PALETTE_BYTES == ICON_PALETTE_COUNT * 32
        );
        for i in 0..ICON_PALETTE_COUNT {
            upload_bytes(
                VramRect::new(
                    ICON_CLUT_RECT.0,
                    ICON_CLUT_RECT.1 + i as u16,
                    ICON_CLUT_RECT.2,
                    ICON_CLUT_RECT.3,
                ),
                &ICON_DATA[i * 32..i * 32 + 32],
            );
        }
    }
    /// Texels of one icon sheet, for the animation cache's upload closure: four icons to a
    /// row of the 64 texel slot, in charm order, the last sheet only as tall as it is full.
    pub fn texels(sheet: usize) -> Option<(&'static [u8], u16, u16)> {
        if sheet >= ICON_SHEETS {
            return None;
        }
        let icons = (CHARM_COUNT - sheet * PER_SHEET).min(PER_SHEET);
        let height = icons.div_ceil(ICONS_PER_SHEET_ROW) * ICON_PX as usize;
        let start = ICON_PALETTE_BYTES + sheet * SHEET_BYTES;
        Some((&ICON_DATA[start..start + 32 * height], 64, height as u16))
    }
    /// The sheets' animation keys, for the frame's working set. A board that is not on screen
    /// adds nothing, so an unpaused frame pays one branch for this.
    ///
    /// `paused` is not redundant with `screen.open`. Start unpauses without
    /// going through `pause::State::step`, so the board can stay open behind
    /// resumed play until the next `enter` resets it, and the same `paused` is
    /// what `dialogue::prepare` already gates the panel on. Without it those
    /// keys would be claimed out of the cache all through that play.
    #[inline(never)]
    pub fn append_needed(paused: bool, screen: &Screen, needed: &mut [u16], len: &mut usize) {
        unsafe { PREPARED = false };
        if !paused || !screen.open {
            return;
        }
        if *len + KEYS > needed.len() {
            // The view behind the board already needs every request the cache
            // takes this frame. The icons are what can wait a frame; the view
            // cannot.
            unsafe { HK_CHARM_ICONS_CROWDED = HK_CHARM_ICONS_CROWDED.saturating_add(1) };
            return;
        }
        for sheet in 0..KEYS {
            needed[*len] = KEY_BASE + sheet as u16;
            *len += 1;
        }
        unsafe { PREPARED = true };
    }
    /// Added between the panel box and the panel text, so an icon sits on the
    /// board and under the text. Consumes the set it draws.
    pub fn append_icons(ot: &mut OrderingTable<1>) {
        unsafe {
            for i in (0..ICON_COUNT).rev() {
                crate::display::ot_add(ot, 0, &mut ICONS[i], QuadTextured::WORDS);
            }
            ICON_COUNT = 0;
        }
    }
    fn icon(index: usize, x: i16, y: i16, gain: u8) {
        icon_sized(index, x, y, ICON_PX as i16, gain);
    }
    /// An icon drawn `size` texels square from the same 16 texel frame: twice the size is the
    /// chosen charm's picture, a nearest-neighbour doubling as the PS1 draws any scaled quad.
    fn icon_sized(index: usize, x: i16, y: i16, size: i16, gain: u8) {
        let key = KEY_BASE + (index / PER_SHEET) as u16;
        let k = index % PER_SHEET;
        let (u, v) = crate::render::animation_uv(key);
        let u = u + (k % ICONS_PER_SHEET_ROW) as u8 * ICON_PX as u8;
        let v = v + (k / ICONS_PER_SHEET_ROW) as u8 * ICON_PX as u8;
        let right = (u16::from(u) + ICON_PX - 1) as u8;
        let bottom = (u16::from(v) + ICON_PX - 1) as u8;
        let clut = Clut::new(
            ICON_CLUT_RECT.0,
            ICON_CLUT_RECT.1 + ICON_PALETTE[index] as u16,
        )
        .uv_clut_word();
        let (x1, y1) = (x + size, y + size);
        unsafe {
            assert!(ICON_COUNT < MAX_ICONS);
            ICONS[ICON_COUNT] = QuadTextured::with_material(
                [(x, y), (x1, y), (x, y1), (x1, y1)],
                [(u, v), (right, v), (u, bottom), (right, bottom)],
                TextureMaterial::blended(
                    clut,
                    crate::render::animation_tpage_word(key),
                    (gain, gain, gain),
                    BlendMode::Average,
                ),
            );
            ICON_COUNT += 1;
        }
    }
    /// Where slot `index` of the grid sits: ten to a row, every second row shifted in.
    pub fn slot_xy(index: usize) -> (i16, i16) {
        let (col, row) = ((index % GRID_COLS) as i16, (index / GRID_COLS) as i16);
        (GRID_X + col * COL_PITCH + (row & 1) * ODD_SHIFT, GRID_Y + row * ROW_PITCH)
    }
    /// The charm board, as the source's `Charms` pane on a purple backdrop. On the left: the worn
    /// charms and the notch circles (lit as far as the worn charms spend them, tinted past the
    /// board's own notches), the "Collection" rule and the whole collection as a grid of slots
    /// (an empty ring where a charm has not been found, the icon where it has, a lit mark on the
    /// worn ones). On the right, once the chosen charm has been found: its cost, its picture at
    /// twice the size, its name and its description, all centred, and the line that says what
    /// the cross does; a refusal replaces that line while it stands. The glow behind the soul
    /// vessel is drawn under the labels.
    pub fn panel(screen: &Screen) {
        unsafe { ICON_COUNT = 0 };
        let s = state();
        let sel = screen.row;
        let (sx, sy) = slot_xy(sel);
        crate::dialogue::board_backdrop(sx - 8, sx + SLOT_PX + 8, sy - 2, SLOT_PX + 4);
        let now = psx_rt::interrupts::vblank_count();
        let fly = unsafe {
            if crate::dialogue::panel_age() < 3 {
                (LAST_WORN, FLY.0) = (s.equipped, 0);
            } else if s.equipped != LAST_WORN {
                if let Some(charm) = (1..=CHARM_COUNT).find(|&n| s.equipped(n) != !LAST_WORN.and(bit(n)).is_empty())
                {
                    FLY = (charm as u8, now, s.equipped(charm));
                }
                LAST_WORN = s.equipped;
            }
            (usize::from(FLY.0), now.wrapping_sub(FLY.1), FLY.2)
        };
        let flying = fly.0 != 0 && fly.1 < FLY_VBLANKS;
        let art = crate::dialogue_art::board_prepared();
        let icons = unsafe { PREPARED } && art;
        if art {
            crate::dialogue::board_glow_art(DIALOGUE_BOARD_ORB, ORB_X, ORB_Y, (255, 72, 232));
        }
        // Equipped, then Notches, each a label and a row.
        panel_text(LABEL_X, EQUIPPED_Y + 1, "Equipped");
        let mut x = ROW_X;
        let mut seated = 0;
        for charm in 1..=CHARM_COUNT {
            if s.equipped(charm) && seated < EQUIPPED_SLOTS {
                // The icon on its way into the row lands when its flight ends.
                if icons && !(flying && fly.0 == charm) {
                    icon(charm - 1, x, EQUIPPED_Y, 128);
                }
                x += ICON_PITCH;
                seated += 1;
            }
        }
        if icons && flying {
            // Between the collection slot and the place in the row it holds (or held).
            let slot = slot_xy(fly.0 - 1);
            let place = (1..fly.0).filter(|&n| s.equipped(n)).count().min(EQUIPPED_SLOTS - 1) as i16;
            let (from, to) = ((slot.0 + 1, slot.1 + 1), (ROW_X + place * ICON_PITCH, EQUIPPED_Y));
            let (from, to) = if fly.2 { (from, to) } else { (to, from) };
            let u = (fly.1 * 128 / FLY_VBLANKS) as i32;
            let eased = u * (256 - u) / 128;
            let at = |a: i16, b: i16| (i32::from(a) + (i32::from(b) - i32::from(a)) * eased / 128) as i16;
            icon(fly.0 - 1, at(from.0, to.0), at(from.1, to.1), 128);
        }
        panel_text(
            LABEL_X,
            NOTCH_Y - 1,
            if s.overcharmed() { "Overcharmed" } else { "Notches" },
        );
        if art {
            let filled = usize::from(s.filled());
            // As the source's: a pale circle for a free notch, a pink one for a spent one, a red
            // one past the board's own notches.
            for i in 0..filled.max(usize::from(s.notches)) {
                let colour = if i >= usize::from(s.notches) {
                    (0xc0, 0x38, 0x48)
                } else if i < filled {
                    (0xd8, 0x50, 0xb0)
                } else {
                    (0x58, 0x58, 0x78)
                };
                crate::dialogue::board_art_rgb(
                    DIALOGUE_BOARD_NOTCH_LIT,
                    ROW_X + i as i16 * NOTCH_PITCH,
                    NOTCH_Y + 1,
                    colour,
                );
            }
        }
        panel_text(GRID_X + 18, COLLECTION_Y, "Collection");
        crate::dialogue::board_rule(GRID_X, RULE_Y, (GRID_COLS as i16 * COL_PITCH + ODD_SHIFT) as u16);
        for charm in 1..=CHARM_COUNT {
            let (x, y) = slot_xy(charm - 1);
            if s.owns(charm) {
                if icons {
                    icon(charm - 1, x + 1, y + 1, if charm - 1 == sel { 128 } else { 100 });
                }
                if s.equipped(charm) && art {
                    crate::dialogue::board_art(DIALOGUE_BOARD_NOTCH_LIT, x + SLOT_PX - 8, y - 3, 0x80);
                }
            } else if art {
                crate::dialogue::board_art(DIALOGUE_BOARD_SLOT, x, y, if charm - 1 == sel { 0x80 } else { 0x50 });
            }
        }
        // The chosen charm, once it has been found; the source's `Collected Charms` shows nothing
        // for an empty slot either.
        let charm = State::charm(sel + 1);
        let centre = COL_X + COL_W / 2;
        if s.owns(sel + 1) {
            if art {
                let n = i16::from(charm.cost);
                let left = centre - (n * NOTCH_PITCH - 2) / 2;
                for i in 0..n {
                    crate::dialogue::board_art(DIALOGUE_BOARD_NOTCH_LIT, left + i * NOTCH_PITCH, COST_Y, 0x80);
                }
            }
            if icons {
                icon_sized(sel, centre - BIG_PX / 2, BIG_Y, BIG_PX, 128);
            }
            panel_text_gain(centre - panel_width(charm.name) / 2, NAME_Y, charm.name, 0x80);
            let mut y = DESC_TOP;
            for line in charm.text.split('\n') {
                if line.is_empty() {
                    y += BLANK_PITCH;
                    continue;
                }
                panel_text(centre - panel_width(line) / 2, y, line);
                y += DESC_PITCH;
            }
        }
        let footer = match screen.message {
            Some(refusal) => Some(refusal.message()),
            None if !s.owns(sel + 1) => None,
            None if s.equipped(sel + 1) => Some("Unequip"),
            None => Some("Equip"),
        };
        if let Some(footer) = footer {
            panel_text(COL_X + 4, FOOTER_Y, footer);
        }
    }
    /// The soul vessel's glow: its top left corner, where its 24 texel disc sits between the
    /// notch row and the Collection rule.
    const ORB_X: i16 = 100;
    const ORB_Y: i16 = 52;
}
#[cfg(not(test))]
pub use presentation::{append_icons, append_needed, panel, texels, upload, KEYS, KEY_BASE};

#[cfg(test)]
mod tests {
    use super::*;
    const BASE: VitalParams = VitalParams {
        max_health: 5,
        max_soul: 99,
        nail_damage: 5,
        soul_per_hit: 11,
        invulnerable_ticks: 79,
        hazard_invulnerable_ticks: 40,
        recoil_ticks: 12,
        freeze_ticks: 19,
        death_ticks: 171,
        recoil_speed: 983040,
    };
    /// The cooked ids the effects below rely on, so a renumbered catalogue
    /// fails here rather than silently applying the wrong charm.
    const GRUBSONG: usize = 3;
    const STALWART: usize = 4;
    const SOUL_CATCHER: usize = 20;
    const SOUL_EATER: usize = 21;
    const FRAGILE_HEART: usize = 23;
    const FRAGILE_STRENGTH: usize = 25;

    fn owning(charms: &[usize]) -> State {
        let mut s = State::new();
        for &c in charms {
            s.give(c);
        }
        s
    }
    #[test]
    fn cooked_table_names_the_charms_the_effects_assume() {
        assert_eq!(CHARM_COUNT, 40);
        assert_eq!(State::charm(GRUBSONG).name, "Grubsong");
        assert_eq!(State::charm(STALWART).name, "Stalwart Shell");
        assert_eq!(State::charm(SOUL_CATCHER).name, "Soul Catcher");
        assert_eq!(State::charm(SOUL_EATER).name, "Soul Eater");
        assert_eq!(State::charm(FRAGILE_HEART).name, "Fragile Heart");
        assert_eq!(State::charm(FRAGILE_STRENGTH).name, "Fragile Strength");
        assert!(State::charm(FRAGILE_HEART).fragile && State::charm(FRAGILE_STRENGTH).fragile);
        assert!(State::charm(36).bound);
        // Every charm is drawable with the guest's ASCII glyph strip.
        for charm in 1..=CHARM_COUNT {
            let c = State::charm(charm);
            assert!(c.cost >= 1 && c.name.is_ascii());
            assert!(c.text.is_ascii());
        }
    }
    #[test]
    fn equip_spends_notches_and_unequip_returns_them() {
        let mut s = owning(&[SOUL_CATCHER, GRUBSONG]);
        assert_eq!(s.notches, 3);
        assert_eq!(s.toggle(SOUL_CATCHER), Ok(true));
        assert_eq!(s.filled(), 2);
        assert_eq!(s.toggle(GRUBSONG), Ok(true));
        assert_eq!(s.filled(), 3);
        assert!(!s.overcharmed());
        assert_eq!(s.toggle(SOUL_CATCHER), Ok(false));
        assert_eq!(s.filled(), 1);
        assert!(s.equipped(GRUBSONG) && !s.equipped(SOUL_CATCHER));
    }
    #[test]
    fn an_overcharmed_knight_takes_double_damage_until_a_charm_comes_off() {
        // `HeroController::TakeDamage`: TakeHealth(damage * 2) while `overcharmed`, which
        // `GameManager::RefreshOvercharm` sets when more notches are spent than the board has.
        let mut s = owning(&[GRUBSONG, SOUL_EATER]);
        s.can_overcharm = true;
        assert_eq!(s.toggle(GRUBSONG), Ok(true));
        assert_eq!(s.damage_taken(1), 1, "within the board a hit takes its own damage");
        // One free notch is enough to start: Grubsong spent 1 of 3, Soul Eater costs 4.
        assert_eq!(s.toggle(SOUL_EATER), Ok(true));
        assert!(s.overcharmed());
        assert_eq!((s.damage_taken(1), s.damage_taken(2)), (2, 4));
        assert_eq!(s.damage_taken(0), 0);
        assert_eq!(s.damage_taken(u16::MAX), u16::MAX, "saturating");
        // Unequipping puts the spend back within the board and the hit back to one.
        assert_eq!(s.toggle(SOUL_EATER), Ok(false));
        assert!(!s.overcharmed());
        assert_eq!(s.damage_taken(1), 1);
    }
    #[test]
    fn a_full_board_refuses_before_overcharm_is_considered() {
        let mut s = owning(&[GRUBSONG, STALWART, SOUL_CATCHER]);
        assert_eq!(s.toggle(GRUBSONG), Ok(true));
        assert_eq!(s.toggle(STALWART), Ok(true));
        assert_eq!(s.filled(), 3);
        // `Slot Open?` cancels on equal, so the attempt never reaches
        // `Overcharm Check` and no attempt is counted against the wall.
        s.can_overcharm = true;
        assert_eq!(s.toggle(SOUL_CATCHER), Err(Refusal::NoOpenNotch));
        assert_eq!(s.attempts, 0);
        assert_eq!(s.filled(), 3);
    }
    #[test]
    fn overcharming_is_refused_four_times_then_breaks_through() {
        let mut s = owning(&[GRUBSONG, SOUL_EATER]);
        assert_eq!(s.toggle(GRUBSONG), Ok(true));
        // One notch open, a four-notch charm: every attempt reaches the wall.
        for attempt in 1..OVERCHARM_BREAK_ATTEMPT {
            assert_eq!(s.toggle(SOUL_EATER), Err(Refusal::Overcharm(attempt)));
            assert_eq!(s.filled(), 1, "the refused attempt kept the notches");
            assert!(!s.can_overcharm && !s.overcharmed());
        }
        assert_eq!(s.toggle(SOUL_EATER), Ok(true));
        assert!(s.can_overcharm && s.overcharmed());
        assert_eq!(s.filled(), 5);
        // Taking it off again fits the board, so the flag clears.
        assert_eq!(s.toggle(SOUL_EATER), Ok(false));
        assert!(!s.overcharmed() && s.can_overcharm);
    }
    #[test]
    fn an_already_earned_overcharm_needs_no_further_attempts() {
        let mut s = owning(&[GRUBSONG, SOUL_EATER]);
        s.can_overcharm = true;
        assert_eq!(s.toggle(GRUBSONG), Ok(true));
        assert_eq!(s.toggle(SOUL_EATER), Ok(true));
        assert_eq!(s.attempts, 0);
        assert!(s.overcharmed());
    }
    #[test]
    fn unowned_broken_bound_and_effectless_charms_are_refused() {
        let mut s = State::new();
        assert_eq!(s.toggle(GRUBSONG), Err(Refusal::NotOwned));
        s.give(FRAGILE_HEART);
        s.broken = s.broken.or(bit(FRAGILE_HEART));
        assert_eq!(s.toggle(FRAGILE_HEART), Err(Refusal::Broken));
        s.give(36);
        assert_eq!(s.toggle(36), Err(Refusal::Bound));
        let effectless = (1..=CHARM_COUNT)
            .find(|&n| State::charm(n).effect == Effect::None && !State::charm(n).bound)
            .expect("the cooked table leaves some charm unimplemented");
        s.give(effectless);
        assert_eq!(s.toggle(effectless), Err(Refusal::NoEffect));
        assert_eq!(s.filled(), 0);
    }
    #[test]
    fn effects_compose_onto_the_cooked_no_charm_parameters() {
        let mut s = owning(&[
            STALWART,
            SOUL_CATCHER,
            SOUL_EATER,
            FRAGILE_HEART,
            FRAGILE_STRENGTH,
        ]);
        // Owned but not worn changes nothing.
        assert_eq!(
            compose(&s, BASE).invulnerable_ticks,
            BASE.invulnerable_ticks
        );
        // A board wide enough to wear all five at once. Salubra and the other
        // notch sources are outside the admitted slice, so a run cannot reach
        // this yet and the test sets it rather than earning it.
        s.notches = 13;
        s.can_overcharm = true;
        for charm in [
            STALWART,
            SOUL_CATCHER,
            SOUL_EATER,
            FRAGILE_HEART,
            FRAGILE_STRENGTH,
        ] {
            assert_eq!(s.toggle(charm), Ok(true));
        }
        let p = compose(&s, BASE);
        // INVUL_TIME_STAL 1.75 s and RECOIL_DURATION_STAL 0.08 s at 60 Hz.
        assert_eq!(p.invulnerable_ticks, 106);
        assert_eq!(p.hazard_invulnerable_ticks, 53);
        assert_eq!(p.recoil_ticks, 5);
        // SoulGain charges 11, and both SOUL charms stack.
        assert_eq!(p.soul_per_hit, 11 + 3 + 8);
        assert_eq!(p.max_health, 5 + 2);
        // (int)(5 * 1.5f) truncates to 7.
        assert_eq!(p.nail_damage, 7);
    }
    #[test]
    fn grubsong_charges_soul_on_a_hit_that_landed() {
        let mut s = owning(&[GRUBSONG]);
        let mut v = Vitals::new(BASE);
        charge_on_damage(&s, &mut v, BASE);
        assert_eq!(v.soul, 0, "an unworn charm charges nothing");
        assert_eq!(s.toggle(GRUBSONG), Ok(true));
        charge_on_damage(&s, &mut v, BASE);
        assert_eq!(v.soul, 15);
        // The 25 SOUL combo needs Grubberfly's Elegy, which stays unequippable.
        assert_eq!(State::charm(35).effect, Effect::None);
    }
    #[test]
    fn the_cheat_grant_takes_back_only_what_it_gave() {
        // Set up rather than clean up. `state()` is one process-wide `static
        // mut` shared by every test in the binary, so a case that only resets
        // on the way out passes in declaration order and fails the moment
        // anything runs before it. Adding a second test module to the same
        // binary is enough to reorder them.
        let s = state();
        *s = owning(&[GRUBSONG]);
        grant_all(true);
        assert!((1..=CHARM_COUNT).all(|n| state().owns(n)));
        assert_eq!(state().toggle(SOUL_CATCHER), Ok(true));
        grant_all(false);
        let s = state();
        assert!(
            s.owns(GRUBSONG),
            "a charm the save already held stays owned"
        );
        assert!(!s.owns(SOUL_CATCHER) && !s.equipped(SOUL_CATCHER));
        assert_eq!(s.filled(), 0, "nothing worn survives losing its charm");
    }
    #[test]
    fn a_record_that_contradicts_itself_is_refused() {
        let mut s = owning(&[SOUL_CATCHER]);
        assert_eq!(s.toggle(SOUL_CATCHER), Ok(true));
        let (owned, equipped, notches, over) = s.record();
        let back = State::restore(owned, equipped, notches, over).expect("round trip");
        assert_eq!(back.record(), (owned, equipped, notches, over));
        assert_eq!(back.filled(), 2);
        // Worn without being owned, and overcharmed without having earned it.
        assert_eq!(State::restore(psx_math::Mask64::EMPTY, equipped, notches, over), None);
        assert_eq!(State::restore(owned, equipped, 0, over), None);
        let mut heavy = owning(&[SOUL_EATER, GRUBSONG]);
        heavy.can_overcharm = true;
        assert_eq!(heavy.toggle(GRUBSONG), Ok(true));
        assert_eq!(heavy.toggle(SOUL_EATER), Ok(true));
        let (owned, equipped, notches, _) = heavy.record();
        assert_eq!(State::restore(owned, equipped, notches, false), None);
        assert!(State::restore(owned, equipped, notches, true).is_some());
    }
    #[test]
    fn the_cursor_walks_the_grid_and_reports_the_refusal() {
        *state() = State::new();
        let mut screen = Screen::new();
        screen.enter();
        assert_eq!(screen.row, 0);
        // Right and left step a slot and wrap through the whole collection.
        screen.step(RIGHT);
        assert_eq!(screen.row, 1);
        screen.step(LEFT);
        screen.step(LEFT);
        assert_eq!(screen.row, CHARM_COUNT - 1);
        // Down and up step a row and keep the column, wrapping round it.
        screen.row = 3;
        screen.step(DOWN);
        assert_eq!(screen.row, 3 + GRID_COLS);
        screen.step(UP);
        screen.step(UP);
        assert_eq!(screen.row, 3 + CHARM_COUNT - GRID_COLS);
        *state() = State::new();
        screen.row = 0;
        screen.step(0x4000);
        assert_eq!(screen.message, Some(Refusal::NotOwned));
        screen.step(DOWN);
        assert_eq!(screen.message, None);
        assert!(screen.step(0x2000) && !screen.open);
    }
    #[test]
    fn every_charm_has_an_icon_in_a_cooked_palette() {
        assert_eq!(ICON_PALETTE.len(), CHARM_COUNT);
        assert!(ICON_PALETTE
            .iter()
            .all(|&p| (p as usize) < ICON_PALETTE_COUNT));
        assert_eq!(ICON_PALETTE_BYTES, ICON_PALETTE_COUNT * 32);
        // A 4bpp row is the width rounded up to four pixels; four icons tile one row of a 64
        // texel slot, and a sheet has to reach VRAM in a single slot upload.
        assert_eq!(
            ICON_BYTES,
            (ICON_PX as usize + 3) / 4 * 2 * ICON_PX as usize
        );
        assert_eq!(ICON_PX as usize * ICONS_PER_SHEET_ROW, 64);
        assert!(SHEET_BYTES <= 2048);
        let per_sheet = ICONS_PER_SHEET_ROW * ICONS_PER_SHEET_ROW;
        assert_eq!(ICON_SHEETS, (CHARM_COUNT + per_sheet - 1) / per_sheet);
    }
    #[test]
    fn the_cooked_board_seats_every_part_inside_the_screen() {
        // `host/charms.py::panel_layout` fitted the grid beside the description column and the
        // parts to the 240 scanlines and emitted the positions. This is the same arithmetic from
        // the other side, so a hand-edited layout fails here rather than overlapping on a
        // television.
        const GLYPH: i16 = 14;
        let rows = ((CHARM_COUNT + GRID_COLS - 1) / GRID_COLS) as i16;
        let grid_right = GRID_X + (GRID_COLS as i16 - 1) * COL_PITCH + ODD_SHIFT + SLOT_PX;
        assert!(GRID_X >= 8, "the grid leaves the screen");
        assert!(grid_right <= COL_X, "the grid runs into the description column");
        assert!(COL_X + COL_W <= 320 - 4, "the column leaves the screen");
        assert!(SLOT_PX >= ICON_PX as i16 + 2 && COL_PITCH >= ICON_PX as i16 && ROW_PITCH >= SLOT_PX);
        assert!(ODD_SHIFT + SLOT_PX <= COL_PITCH * 2, "a shifted row crosses a slot");
        let grid_bottom = GRID_Y + (rows - 1) * ROW_PITCH + SLOT_PX;
        // Top to bottom on the left: worn charms, notch circles, the label and rule, the grid.
        assert!(EQUIPPED_Y >= 0 && EQUIPPED_Y + ICON_PX as i16 <= NOTCH_Y);
        assert!(NOTCH_Y + 12 <= COLLECTION_Y, "the notch row runs into the Collection label");
        assert!(COLLECTION_Y + GLYPH <= RULE_Y && RULE_Y < GRID_Y - 2);
        assert!(grid_bottom <= 240 - 4);
        // On the right: cost circles, the picture, the name, the description, the Equip line.
        assert!(COST_Y + 10 <= BIG_Y && BIG_Y + BIG_PX <= NAME_Y);
        assert!(NAME_Y + GLYPH <= DESC_TOP);
        let height = |text: &str| -> i16 {
            text.split('\n').map(|l| if l.is_empty() { BLANK_PITCH } else { DESC_PITCH }).sum()
        };
        let tallest = (1..=CHARM_COUNT).map(|n| height(State::charm(n).text)).max().unwrap();
        assert!(
            FOOTER_Y >= DESC_TOP + tallest,
            "the longest description runs into the Equip line"
        );
        assert!(FOOTER_Y + GLYPH <= 240 - 4);
        // The labels clear the circles and icons they head, and seventeen circles, a worn row and
        // the refusals fit the screen and the column.
        assert!(LABEL_X + 76 <= ROW_X, "Overcharmed runs into its row");
        assert!(ROW_X + 17 * NOTCH_PITCH <= 320 - 8, "seventeen circles run off the screen");
        assert!(ROW_X + 11 * ICON_PITCH <= 320 - 8, "eleven worn icons run off the screen");
        // The cost circles of the dearest charm centre over the picture inside the column.
        let dearest = (1..=CHARM_COUNT).map(|n| State::charm(n).cost).max().unwrap() as i16;
        assert!(dearest * NOTCH_PITCH <= COL_W);
    }
    #[test]
    fn panel_fits_the_glyph_budget() {
        // `dialogue` draws one packet per non-space character and per art piece into a
        // 360-entry table and asserts on overflow, so the worst board has to fit at cook time
        // rather than at the moment the player opens the screen. The worst board: the chosen
        // charm owned with the longest description, every other slot an empty ring (but the
        // owned one), seventeen notch circles, eleven worn-charm marks and a full cost.
        let fixed = ["Equipped", "Overcharmed", "Collection", "Cannot be removed"]
            .iter()
            .map(|s| s.len())
            .sum::<usize>();
        let widest = (1..=CHARM_COUNT)
            .map(|n| {
                let c = State::charm(n);
                c.name.len() + c.text.len()
            })
            .max()
            .unwrap_or(0);
        // Empty rings, notch circles, worn marks, cost circles, and the vessel's glow.
        let art = (CHARM_COUNT - 1) + 17 + 11 + 6 + 1;
        assert!(
            fixed + widest + art <= crate::dialogue::CAP,
            "charm panel needs {} glyphs",
            fixed + widest + art
        );
    }
}
