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

/// One cooked charm. `lines` is its localized description, already wrapped to
/// the panel by the host against the same glyph advances the guest draws with.
pub struct Charm {
    pub name: &'static str,
    pub cost: u8,
    pub lines: &'static [&'static str],
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
            Refusal::Bound => "This charm cannot be removed",
            Refusal::NoEffect => "No effect in this port yet",
        }
    }
}

/// The charm half of PlayerData: `gotCharm_N`, `equippedCharm_N`, `charmSlots`,
/// `canOvercharm` and the derived `charmSlotsFilled`/`overcharmed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    owned: u64,
    equipped: u64,
    broken: u64,
    pub notches: u8,
    pub can_overcharm: bool,
    /// `UI Charms`' own `Overcharm Attempts`. It is an FSM variable in the
    /// source, not PlayerData, so it lasts the session and is never saved.
    attempts: u8,
}
const fn bit(charm: usize) -> u64 {
    1u64 << (charm - 1)
}
impl State {
    pub const fn new() -> Self {
        Self {
            owned: 0,
            equipped: 0,
            broken: 0,
            notches: STARTING_NOTCHES,
            can_overcharm: false,
            attempts: 0,
        }
    }
    pub fn charm(charm: usize) -> &'static Charm {
        &CHARMS[charm - 1]
    }
    pub fn owns(&self, charm: usize) -> bool {
        self.owned & bit(charm) != 0
    }
    pub fn equipped(&self, charm: usize) -> bool {
        self.equipped & bit(charm) != 0
    }
    pub fn broken(&self, charm: usize) -> bool {
        self.broken & bit(charm) != 0
    }
    pub fn give(&mut self, charm: usize) {
        self.owned |= bit(charm);
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
        self.equipped |= bit(charm);
        Ok(())
    }
    fn unequip(&mut self, charm: usize) {
        self.equipped &= !bit(charm);
    }
    /// Everything the memory card keeps. `filled` and `overcharmed` are derived
    /// and the attempt count is deliberately session-only.
    pub fn record(&self) -> (u64, u64, u8, bool) {
        (self.owned, self.equipped, self.notches, self.can_overcharm)
    }
    /// Reject a record whose equipped set is not a subset of what it owns, or
    /// whose notch count cannot hold it without the overcharm it did not earn.
    pub fn restore(owned: u64, equipped: u64, notches: u8, can_overcharm: bool) -> Option<Self> {
        let mask = if CHARM_COUNT >= 64 {
            u64::MAX
        } else {
            (1u64 << CHARM_COUNT) - 1
        };
        if owned & !mask != 0 || equipped & !owned != 0 || notches == 0 {
            return None;
        }
        let state = Self {
            owned,
            equipped,
            broken: 0,
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
pub fn load(owned: u64, equipped: u64, notches: u8, can_overcharm: bool) -> bool {
    let restored = State::restore(owned, equipped, notches, can_overcharm);
    *state() = restored.unwrap_or_else(State::new);
    publish();
    restored.is_some()
}
/// The profile's board plus whatever the boot Cheats page already asked for.
pub fn boot(saved: Option<(u64, u64, u8, bool)>, grant: bool) -> bool {
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
pub fn record() -> (u64, u64, u8, bool) {
    state().record()
}
/// What the cheat added, so turning it off takes back only that and never a
/// charm the save already owned.
static mut GRANTED: u64 = 0;
/// The cheat grant, kept out of the equipped set: it only marks charms found,
/// so equipping still runs the same notch board a real pickup would.
pub fn grant_all(on: bool) {
    let s = state();
    if on {
        let mut added = 0;
        for charm in 1..=CHARM_COUNT {
            if !s.owns(charm) {
                added |= bit(charm);
                s.give(charm);
            }
        }
        unsafe { GRANTED |= added };
    } else {
        let taken = core::mem::take(unsafe { &mut *core::ptr::addr_of_mut!(GRANTED) });
        s.owned &= !taken;
        // Dropping the grant must not leave a charm worn that is no longer owned.
        s.equipped &= s.owned;
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
    if s.equipped == 0 {
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
pub const DOWN: u16 = 0x40;
pub const CIRCLE: u16 = 0x2000;
pub const CROSS: u16 = 0x4000;
/// The cursor and the last refusal on the pause charm screen.
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
    /// The window of rows the cursor sits in, so the list scrolls a page at a
    /// time rather than one row at a time under a fixed cursor.
    pub fn top(&self) -> usize {
        self.row / VISIBLE * VISIBLE
    }
    /// One edge-triggered step. Returns true when the screen has closed.
    pub fn step(&mut self, press: u16) -> bool {
        if press & CIRCLE != 0 {
            self.open = false;
            return true;
        }
        if press & UP != 0 {
            self.row = (self.row + CHARM_COUNT - 1) % CHARM_COUNT;
            self.message = None;
        }
        if press & DOWN != 0 {
            self.row = (self.row + 1) % CHARM_COUNT;
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
    use crate::dialogue::{panel_frame, panel_number, panel_text, panel_width};
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
    /// linked RAM and reach VRAM through the shared 64x64 slots on demand.
    pub const KEY_BASE: u16 =
        crate::ability_art::KEY_BASE + crate::ability_art::ABILITY_FRAMES.len() as u16;
    const _: () = assert!(KEY_BASE as usize + CHARM_COUNT <= hk_cache::MAX_KEYS);
    const _: () = assert!(
        ICON_BYTES <= 2048,
        "an icon has to reach VRAM in one slot upload"
    );
    // The board's own claim on the cache. It is not the whole story: the frozen
    // view behind the panel keeps requesting its enemies, whose cap is larger
    // than what is left, so `append_needed` yields rather than asserting.
    const _: () = assert!(VISIBLE + hk_cache::RESERVED_SLOTS <= hk_cache::MAX_REQUESTS);
    /// Frames the board wanted an icon for and could not have, because the view
    /// behind it had already claimed every request the cache takes in a frame.
    #[no_mangle]
    pub static mut HK_CHARM_ICONS_CROWDED: u32 = 0;
    /// The row window `append_needed` declared to the animation cache this
    /// frame. `panel` draws for this rather than for its own current window, so
    /// a board that opened after the working set was fixed draws no icon that
    /// frame instead of asking the cache for a slot it was never given.
    static mut PREPARED: Option<usize> = None;
    static mut ICONS: [QuadTextured; VISIBLE] =
        [const { QuadTextured::new([(0, 0); 4], [(0, 0); 4], 0, 0, (128, 128, 128)) }; VISIBLE];
    static mut ICON_COUNT: usize = 0;

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
    /// Texels for one charm icon, for the animation cache's upload closure.
    /// Every icon is the same square, so the offset is a multiply rather than
    /// the `Frame` table the Shade and the ability clips carry.
    pub fn texels(index: usize) -> Option<(&'static [u8], u16, u16)> {
        if index >= CHARM_COUNT {
            return None;
        }
        let start = ICON_PALETTE_BYTES + index * ICON_BYTES;
        Some((&ICON_DATA[start..start + ICON_BYTES], ICON_PX, ICON_PX))
    }
    /// The visible rows' animation keys, for the frame's working set. A board
    /// that is not on screen adds nothing, so an unpaused frame pays one branch
    /// for this.
    ///
    /// `paused` is not redundant with `screen.open`. Start unpauses without
    /// going through `pause::State::step`, so the board can stay open behind
    /// resumed play until the next `enter` resets it, and the same `paused` is
    /// what `dialogue::prepare` already gates the panel on. Without it those
    /// six keys would be claimed out of the cache all through that play.
    #[inline(never)]
    pub fn append_needed(paused: bool, screen: &Screen, needed: &mut [u16], len: &mut usize) {
        unsafe { PREPARED = None };
        if !paused || !screen.open {
            return;
        }
        let top = screen.top();
        let rows = VISIBLE.min(CHARM_COUNT - top);
        if *len + rows > needed.len() {
            // The view behind the board already needs every request the cache
            // takes this frame. The icons are what can wait a frame; the view
            // cannot, and losing rows off the board to fit would be worse than
            // a text-only page.
            unsafe { HK_CHARM_ICONS_CROWDED = HK_CHARM_ICONS_CROWDED.saturating_add(1) };
            return;
        }
        for charm in top..top + rows {
            needed[*len] = KEY_BASE + charm as u16;
            *len += 1;
        }
        unsafe { PREPARED = Some(top) };
    }
    /// Added between the panel box and the panel text, so an icon sits on the
    /// board and under the row it belongs to. Consumes the set it draws.
    pub fn append_icons(ot: &mut OrderingTable<1>) {
        unsafe {
            for i in (0..ICON_COUNT).rev() {
                ot.add(0, &mut ICONS[i], QuadTextured::WORDS);
            }
            ICON_COUNT = 0;
        }
    }
    fn icon(index: usize, x: i16, y: i16) {
        let key = KEY_BASE + index as u16;
        let (u, v) = crate::render::animation_uv(key);
        let right = (u16::from(u) + ICON_PX - 1) as u8;
        let bottom = (u16::from(v) + ICON_PX - 1) as u8;
        let clut = Clut::new(
            ICON_CLUT_RECT.0,
            ICON_CLUT_RECT.1 + ICON_PALETTE[index] as u16,
        )
        .uv_clut_word();
        let (x1, y1) = (x + ICON_PX as i16, y + ICON_PX as i16);
        unsafe {
            ICONS[ICON_COUNT] = QuadTextured::with_material(
                [(x, y), (x1, y), (x, y1), (x1, y1)],
                [(u, v), (right, v), (u, bottom), (right, bottom)],
                TextureMaterial::blended(
                    clut,
                    crate::render::animation_tpage_word(key),
                    (128, 128, 128),
                    BlendMode::Average,
                ),
            );
            ICON_COUNT += 1;
        }
    }
    /// The charm board, over the paused view. Rows carry the icon, the
    /// collection mark, the name and the notch cost; the selected charm's
    /// description fills the bottom of the box, and a refusal replaces the
    /// footer while it stands. Every position is cooked, because the icon size
    /// sets the row pitch and the row pitch sets everything under it.
    pub fn panel(screen: &Screen) {
        unsafe { ICON_COUNT = 0 };
        panel_frame(PANEL_RECT.0, PANEL_RECT.1, PANEL_RECT.2, PANEL_RECT.3);
        panel_text(160 - panel_width("Charms") / 2, TITLE_Y, "Charms");
        let s = state();
        let label = if s.overcharmed() {
            "Overcharmed"
        } else {
            "Notches"
        };
        panel_text(110, NOTCH_Y, label);
        panel_number(110 + panel_width(label) + 6, NOTCH_Y, u32::from(s.filled()));
        panel_text(110 + panel_width(label) + 18, NOTCH_Y, "/");
        panel_number(110 + panel_width(label) + 26, NOTCH_Y, u32::from(s.notches));
        let top = screen.top();
        let drawable = unsafe { PREPARED } == Some(top);
        for offset in 0..VISIBLE {
            let charm = top + offset + 1;
            if charm > CHARM_COUNT {
                break;
            }
            let y = ROW_TOP + offset as i16 * ROW_PITCH;
            let text_y = y + ROW_TEXT_OFFSET;
            if charm == screen.row + 1 {
                panel_text(CURSOR_X, text_y, ">");
            }
            if drawable {
                icon(charm - 1, ICON_X, y);
            }
            panel_text(
                MARK_X,
                text_y,
                if s.equipped(charm) {
                    "E"
                } else if s.owns(charm) {
                    "o"
                } else {
                    "-"
                },
            );
            panel_text(NAME_X, text_y, State::charm(charm).name);
            panel_number(COST_X, text_y, u32::from(State::charm(charm).cost));
        }
        for (i, line) in State::charm(screen.row + 1).lines.iter().enumerate() {
            panel_text(28, DESC_TOP + i as i16 * DESC_PITCH, line);
        }
        let footer = match screen.message {
            Some(refusal) => refusal.message(),
            None => "X: equip    O: back",
        };
        panel_text(160 - panel_width(footer) / 2, FOOTER_Y, footer);
    }
}
#[cfg(not(test))]
pub use presentation::{append_icons, append_needed, panel, texels, upload, KEY_BASE};

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
            assert!(c.lines.iter().all(|l| l.is_ascii()));
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
        s.broken |= bit(FRAGILE_HEART);
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
        assert_eq!(State::restore(0, equipped, notches, over), None);
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
    fn the_screen_pages_the_list_and_reports_the_refusal() {
        *state() = State::new();
        let mut screen = Screen::new();
        screen.enter();
        assert_eq!(screen.top(), 0);
        for _ in 0..VISIBLE {
            screen.step(0x40);
        }
        assert_eq!(screen.row, VISIBLE);
        assert_eq!(screen.top(), VISIBLE);
        screen.step(0x10);
        assert_eq!(screen.top(), 0);
        // Up from the first row wraps to the last, as the pause rows do.
        screen.row = 0;
        screen.step(0x10);
        assert_eq!(screen.row, CHARM_COUNT - 1);
        *state() = State::new();
        screen.row = 0;
        screen.step(0x4000);
        assert_eq!(screen.message, Some(Refusal::NotOwned));
        screen.step(0x40);
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
        // A 4bpp row is the width rounded up to four pixels, and one icon has
        // to reach VRAM in a single 64x64 animation slot upload.
        assert_eq!(
            ICON_BYTES,
            (ICON_PX as usize + 3) / 4 * 2 * ICON_PX as usize
        );
        assert!(ICON_BYTES <= 2048);
    }
    #[test]
    fn the_cooked_board_seats_every_part_inside_the_screen() {
        // `host/charms.py::panel_layout` picked the icon size against these 240
        // scanlines and emitted the positions. This is the same arithmetic from
        // the other side, so a hand-edited layout fails here rather than
        // overlapping two rows on a television.
        const GLYPH: i16 = 12;
        let (x, top, width, height) = PANEL_RECT;
        let bottom = top + height as i16;
        assert!(x > 0 && i32::from(x) + i32::from(width) < 320);
        assert!(top > 0 && bottom < 240);
        assert!(ROW_PITCH >= ICON_PX as i16 && ROW_TEXT_OFFSET >= 0);
        assert!(TITLE_Y > top && NOTCH_Y >= TITLE_Y + GLYPH);
        assert!(ROW_TOP >= NOTCH_Y + GLYPH);
        let rows_end = ROW_TOP + (VISIBLE as i16 - 1) * ROW_PITCH + ICON_PX as i16;
        assert!(
            DESC_TOP >= rows_end,
            "the last row runs into the description"
        );
        let lines = (1..=CHARM_COUNT)
            .map(|n| State::charm(n).lines.len())
            .max()
            .unwrap() as i16;
        assert!(
            FOOTER_Y >= DESC_TOP + (lines - 1) * DESC_PITCH + GLYPH,
            "the longest description runs into the footer"
        );
        assert!(FOOTER_Y + GLYPH <= bottom);
        // The icon column has to clear the cursor and leave the name its width.
        assert!(ICON_X >= CURSOR_X + GLYPH && MARK_X >= ICON_X + ICON_PX as i16);
        assert!(NAME_X > MARK_X && COST_X > NAME_X + 100);
    }
    #[test]
    fn panel_fits_the_glyph_budget() {
        // `dialogue` draws one glyph per non-space character into a 416-entry
        // table and asserts on overflow, so the worst page has to fit at cook
        // time rather than at the moment the player opens the screen.
        let fixed = [
            "Charms",
            "Overcharmed",
            "/",
            ">",
            "E",
            "X: equip    O: back",
        ]
        .iter()
        .map(|s| s.len())
        .sum::<usize>()
            + 8;
        let widest_rows = (0..CHARM_COUNT / VISIBLE)
            .map(|page| {
                (0..VISIBLE)
                    .map(|i| State::charm(page * VISIBLE + i + 1).name.len() + 2)
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);
        let widest_text = (1..=CHARM_COUNT)
            .map(|n| State::charm(n).lines.iter().map(|l| l.len()).sum::<usize>())
            .max()
            .unwrap_or(0);
        assert!(
            fixed + widest_rows + widest_text <= crate::dialogue::CAP,
            "charm panel needs {} glyphs",
            fixed + widest_rows + widest_text
        );
    }
}
