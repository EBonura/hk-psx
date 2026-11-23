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
//! records the missing system in .hkpsx/charm-catalog.json. The ones that are
//! implemented compose onto the cooked no-charm `HeroParams` through `hero`,
//! which `hero_params` calls once per change, so every consumer of the live
//! parameters sees them without each one applying the bonus itself.
use hk_sim::{HeroParams, VitalParams, Vitals};

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
    /// Fragile Greed: `HealthManager::Die` adds `CeilToInt(n * 0.2f)` to each drop count
    /// while the charm is worn and unbroken; `GREED_SHARE` is that 0.2 (`geo_drops`).
    GeoDrops,
    /// Fragile Strength: `Set Slash Damage` truncates nail damage times 1.5.
    NailScale { numerator: u16, denominator: u16 },
    /// Grubsong: `TakeDamageCharmEffects` charges SOUL on a damaging hit.
    SoulOnDamage {
        alone: u16,
        combo: u16,
        combo_charm: u8,
    },
    /// Quick Focus: `Set Focus Speed` takes `Time Per MP Drain CH` (microseconds).
    FocusDrain(u16),
    /// Deep Focus: `Deep Focus Speed` multiplies the drain time after Quick
    /// Focus has set it, and `Set HP Amount` heals `heal` masks.
    DeepFocus {
        numerator: u16,
        denominator: u16,
        heal: u16,
    },
    /// Steady Body: `RecoilLeft`/`RecoilRight` return early, so a nail hit
    /// recoils for this many ticks (none).
    NailRecoil(u16),
    /// Shaman Stone: Vengeful Spirit's `Fireball Control` `Set Damage` writes
    /// the charmed damage and scales the ball, which scales its collider.
    Fireball {
        damage: u16,
        x: (u16, u16),
        y: (u16, u16),
    },
    /// Dashmaster: `HeroDash` arms `DASH_COOLDOWN_CH` (ticks).
    DashCooldown(u16),
    /// Quick Slash: `Attack` and `DoAttack` take `ATTACK_DURATION_CH` and
    /// `ATTACK_COOLDOWN_TIME_CH` (ticks).
    AttackTiming { duration: u16, cooldown: u16 },
    /// Spell Twister: `Set Spell Cost` writes `MP Cost`.
    SpellCost(u16),
    /// Longnail and Mark of Pride: `NailSlash::StartSlash` multiplies the slash's
    /// localScale by `alone` (Q16), or by `combo` with `combo_charm` worn too.
    SlashScale {
        alone: i32,
        combo: i32,
        combo_charm: u8,
    },
    /// Sprintmaster: `Move` on the ground runs at `RUN_SPEED_CH`, or at
    /// `RUN_SPEED_CH_COMBO` with `combo_charm` (Dashmaster) worn too (Q16).
    RunSpeed {
        alone: i32,
        combo: i32,
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
    /// `Deactivate UI` and `Set Current Item Num` test `atBench` first: away from a bench the
    /// board can be read but nothing goes on or comes off, and `Bench Reminder` raises `Charm
    /// Equip Msg`. Its line is `BENCH_REMINDER`, drawn over the description column while it stands.
    NotAtBench,
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
            Refusal::NotAtBench => BENCH_REMINDER,
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
    crate::hero_params::invalidate();
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
///
/// Placed right after `main` in `.psx_hot` (main.rs) and aligned to 64 bytes: `main` is 4 bytes longer in the
/// route-input twin than in the shipping build, and this function, the same in both, absorbs that, so the
/// laid-out hot text that follows starts at the same address in both builds (docs/HOT-TEXT.md).
#[inline(never)]
#[cfg_attr(target_arch = "mips", link_section = ".psx_hot.01_charms_boot", rustc_align(64))]
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
/// order. `hero_params` calls this when the board changes, so this is the one
/// place a charm bonus is applied and no consumer applies it a second time.
pub fn hero(base: HeroParams) -> HeroParams {
    compose(state(), base)
}
fn compose(s: &State, mut p: HeroParams) -> HeroParams {
    // Nothing equipped is the common case; nothing below runs for it. This
    // used to run on every read, several times a tick, and walking all 40
    // charms there cost enough to move kings-return 3.2 units and two masks: a
    // frame that slips past a VBlank boundary changes how many simulation
    // ticks fall between two input polls. That is why `hero_params` caches it.
    if s.equipped.is_empty() {
        return p;
    }
    for charm in 1..=CHARM_COUNT {
        if !s.equipped(charm) {
            continue;
        }
        let v = &mut p.vitals;
        match State::charm(charm).effect {
            Effect::Shell {
                invulnerable_ticks,
                hazard_invulnerable_ticks,
                recoil_ticks,
            } => {
                v.invulnerable_ticks = invulnerable_ticks;
                v.hazard_invulnerable_ticks = hazard_invulnerable_ticks;
                v.recoil_ticks = recoil_ticks;
            }
            Effect::SoulPerHit(extra) => v.soul_per_hit = v.soul_per_hit.saturating_add(extra),
            Effect::MaxHealth(extra) => v.max_health = v.max_health.saturating_add(extra),
            // `(int)((float)damage * 1.5f)` truncates, so integer division does too.
            Effect::NailScale {
                numerator,
                denominator,
            } => {
                v.nail_damage = v.nail_damage.saturating_mul(numerator) / denominator;
            }
            // Charm order puts Quick Focus (7) before Deep Focus (34), the
            // order `Set Focus Speed` and `Deep Focus Speed` run in.
            Effect::FocusDrain(us) => p.focus.drain_interval_us = us,
            Effect::DeepFocus {
                numerator,
                denominator,
                heal,
            } => {
                let us = u32::from(p.focus.drain_interval_us) * u32::from(numerator)
                    / u32::from(denominator);
                p.focus.drain_interval_us = us.min(u32::from(u16::MAX)) as u16;
                p.focus.heal_amount = heal;
            }
            Effect::NailRecoil(ticks) => p.nail_response.recoil_ticks = ticks,
            Effect::Fireball { damage, x, y } => {
                p.fireball.damage = damage;
                let b = &mut p.fireball.bounds;
                for i in [0, 2] {
                    b[i] = b[i] * i32::from(x.0) / i32::from(x.1);
                }
                for i in [1, 3] {
                    b[i] = b[i] * i32::from(y.0) / i32::from(y.1);
                }
            }
            Effect::DashCooldown(ticks) => p.dash_cooldown_ticks = ticks,
            Effect::AttackTiming { duration, cooldown } => {
                p.attack.duration = duration;
                p.attack.cooldown = cooldown;
            }
            Effect::SpellCost(cost) => p.fireball.cost = cost,
            Effect::RunSpeed {
                alone,
                combo,
                combo_charm,
            } => {
                p.ground_speed = if s.equipped(combo_charm as usize) {
                    combo
                } else {
                    alone
                };
            }
            Effect::SlashScale {
                alone,
                combo,
                combo_charm,
            } => {
                p.slash_scale = if s.equipped(combo_charm as usize) {
                    combo
                } else {
                    alone
                };
            }
            // `Die` tests `brokenCharm_24` as well as `equippedCharm_24`.
            Effect::GeoDrops => p.greed = !s.broken(charm),
            Effect::None | Effect::SoulOnDamage { .. } => {}
        }
    }
    p
}
/// An enemy's three drop counts (small, medium, large) as `HealthManager::Die` flings them:
/// with Fragile Greed each gains `ceil(n * GREED_SHARE)`, more coins of the same sizes.
/// `geo::World` queues what its loose pool cannot seat yet, so none is lost.
#[inline(never)]
pub fn geo_drops(drops: [u16; 3]) -> [u16; 3] {
    greed_drops(crate::hero_params::current(), drops)
}
fn greed_drops(p: &HeroParams, mut drops: [u16; 3]) -> [u16; 3] {
    let (numerator, denominator) = GREED_SHARE;
    if p.greed {
        for n in &mut drops {
            *n += (*n * numerator + denominator - 1) / denominator;
        }
    }
    drops
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
    /// `PlayerData.atBench`, which the caller sets from the bench before each step.
    pub at_bench: bool,
}
impl Screen {
    pub const fn new() -> Self {
        Self {
            open: false,
            row: 0,
            message: None,
            at_bench: false,
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
            // `Got Charm` is tested before `atBench`, so an empty slot still says so.
            let charm = self.row + 1;
            self.message = if self.at_bench || !state().owns(charm) {
                state().toggle(charm).err()
            } else {
                Some(Refusal::NotAtBench)
            };
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
        .uv_word();
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
            // `Charm Equip Msg` stands over the description until the next move.
            let text = if screen.message == Some(Refusal::NotAtBench) {
                BENCH_REMINDER
            } else {
                charm.text
            };
            for line in text.split('\n') {
                if line.is_empty() {
                    y += BLANK_PITCH;
                    continue;
                }
                panel_text(centre - panel_width(line) / 2, y, line);
                y += DESC_PITCH;
            }
        }
        let footer = match screen.message {
            Some(Refusal::NotAtBench) => None,
            Some(refusal) => Some(refusal.message()),
            // `Equip Prompt` moves the prompt off screen away from a bench.
            None if !s.owns(sel + 1) || !screen.at_bench => None,
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
    const QUICK_FOCUS: usize = 7;
    const STEADY_BODY: usize = 14;
    const SHAMAN_STONE: usize = 19;
    const DASHMASTER: usize = 31;
    const QUICK_SLASH: usize = 32;
    const SPELL_TWISTER: usize = 33;
    const DEEP_FOCUS: usize = 34;
    const SPRINTMASTER: usize = 37;
    /// The cooked hero record with the vitals above, so the vital asserts do
    /// not move with a recook.
    fn hero_base() -> HeroParams {
        HeroParams {
            vitals: BASE,
            ..crate::hero_params::BASE
        }
    }

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
        assert_eq!(State::charm(QUICK_FOCUS).name, "Quick Focus");
        assert_eq!(State::charm(STEADY_BODY).name, "Steady Body");
        assert_eq!(State::charm(SHAMAN_STONE).name, "Shaman Stone");
        assert_eq!(State::charm(DASHMASTER).name, "Dashmaster");
        assert_eq!(State::charm(QUICK_SLASH).name, "Quick Slash");
        assert_eq!(State::charm(SPELL_TWISTER).name, "Spell Twister");
        assert_eq!(State::charm(DEEP_FOCUS).name, "Deep Focus");
        assert_eq!(State::charm(SPRINTMASTER).name, "Sprintmaster");
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
            compose(&s, hero_base()).vitals.invulnerable_ticks,
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
        let p = compose(&s, hero_base()).vitals;
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
    fn the_numeric_charms_set_the_source_values() {
        let base = hero_base();
        let mut s = owning(&[
            QUICK_FOCUS,
            STEADY_BODY,
            SHAMAN_STONE,
            DASHMASTER,
            QUICK_SLASH,
            SPELL_TWISTER,
            SPRINTMASTER,
            DEEP_FOCUS,
        ]);
        s.notches = 20;
        let worn = |s: &mut State, charms: &[usize]| {
            s.equipped = psx_math::Mask64::EMPTY;
            for &c in charms {
                assert_eq!(s.toggle(c), Ok(true), "charm {c}");
            }
            compose(s, base)
        };
        // Nothing worn is the cooked record.
        let p = worn(&mut s, &[]);
        assert_eq!(p.focus.drain_interval_us, base.focus.drain_interval_us);
        assert_eq!(p.ground_speed, base.ground_speed);
        // `Time Per MP Drain CH` 0.018 s.
        assert_eq!(worn(&mut s, &[QUICK_FOCUS]).focus.drain_interval_us, 18_000);
        // `RecoilLeft`/`RecoilRight` return before arming the recoil.
        assert_eq!(worn(&mut s, &[STEADY_BODY]).nail_response.recoil_ticks, 0);
        // `Fireball Control` `Set Damage`: 20 damage, scale 1.3 by 1.6.
        let f = worn(&mut s, &[SHAMAN_STONE]).fireball;
        assert_eq!(f.damage, 20);
        assert_eq!(f.bounds[2], base.fireball.bounds[2] * 13 / 10);
        assert_eq!(f.bounds[3], base.fireball.bounds[3] * 8 / 5);
        // DASH_COOLDOWN_CH 0.4 s, ATTACK_DURATION_CH 0.28 s, ATTACK_COOLDOWN_TIME_CH 0.25 s.
        assert_eq!(worn(&mut s, &[DASHMASTER]).dash_cooldown_ticks, 24);
        let a = worn(&mut s, &[QUICK_SLASH]).attack;
        assert_eq!((a.duration, a.cooldown), (17, 15));
        // `Set Spell Cost`'s `Mage` state.
        assert_eq!(worn(&mut s, &[SPELL_TWISTER]).fireball.cost, 24);
        // RUN_SPEED_CH 10 alone, RUN_SPEED_CH_COMBO 11.5 with Dashmaster.
        assert_eq!(worn(&mut s, &[SPRINTMASTER]).ground_speed, 10 * 65536);
        assert_eq!(
            worn(&mut s, &[SPRINTMASTER, DASHMASTER]).ground_speed,
            23 * 32768
        );
        // Deep Focus multiplies whatever drain Quick Focus left: 0.018 s * 1.65,
        // or `Time Per MP Drain UnCH` 0.027 s * 1.65 alone, and heals two masks.
        let p = worn(&mut s, &[QUICK_FOCUS, DEEP_FOCUS]).focus;
        assert_eq!((p.drain_interval_us, p.heal_amount), (29_700, 2));
        assert_eq!(worn(&mut s, &[DEEP_FOCUS]).focus.drain_interval_us, 44_550);
    }
    #[test]
    fn longnail_and_pride_scale_the_slash_alone_and_together() {
        const PRIDE: usize = 13;
        const LONGNAIL: usize = 18;
        assert_eq!(State::charm(PRIDE).name, "Mark of Pride");
        assert_eq!(State::charm(LONGNAIL).name, "Longnail");
        let base = hero_base();
        assert_eq!(base.slash_scale, hk_sim::ONE);
        let mut s = owning(&[PRIDE, LONGNAIL]);
        s.notches = 20;
        let worn = |s: &mut State, charms: &[usize]| {
            s.equipped = psx_math::Mask64::EMPTY;
            for &c in charms {
                assert_eq!(s.toggle(c), Ok(true), "charm {c}");
            }
            compose(s, base).slash_scale
        };
        // StartSlash: 1.15 for Longnail, 1.25 for Pride, 1.4 for both (not 1.15 * 1.25).
        assert_eq!(worn(&mut s, &[]), hk_sim::ONE);
        assert_eq!(worn(&mut s, &[LONGNAIL]), 75366);
        assert_eq!(worn(&mut s, &[PRIDE]), 81920);
        assert_eq!(worn(&mut s, &[PRIDE, LONGNAIL]), 91750);
        assert_eq!(worn(&mut s, &[LONGNAIL, PRIDE]), 91750);
        // The pivots are the slash transforms' local positions: the Slash sits at (-0.01, -0.41).
        assert_eq!(NAIL_PIVOTS[0], [-655, -26870]);
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
    fn fragile_greed_adds_a_ceiling_fifth_to_every_drop_size() {
        const FRAGILE_GREED: usize = 24;
        let mut s = owning(&[FRAGILE_GREED]);
        let drops = |s: &State, d| greed_drops(&compose(s, hero_base()), d);
        assert_eq!(drops(&s, [40, 5, 0]), [40, 5, 0], "an unworn charm adds nothing");
        assert_eq!(s.toggle(FRAGILE_GREED), Ok(true));
        // CeilToInt(n * 0.2f): 2 becomes 3, 5 becomes 6, 40 becomes 48, 0 stays 0.
        assert_eq!(drops(&s, [2, 0, 0]), [3, 0, 0]);
        assert_eq!(drops(&s, [40, 5, 0]), [48, 6, 0]);
        assert_eq!(drops(&s, [1, 1, 1]), [2, 2, 2]);
        // `brokenCharm_24` turns it off, as Die tests it.
        s.broken = s.broken.or(bit(FRAGILE_GREED));
        assert_eq!(drops(&s, [40, 5, 0]), [40, 5, 0]);
    }
    #[test]
    fn charms_go_on_and_come_off_only_at_a_bench() {
        *state() = State::new();
        state().give(GRUBSONG);
        let mut screen = Screen::new();
        screen.enter();
        screen.row = GRUBSONG - 1;
        screen.step(CROSS);
        assert_eq!(screen.message, Some(Refusal::NotAtBench));
        assert!(!state().equipped(GRUBSONG));
        screen.at_bench = true;
        screen.step(CROSS);
        assert_eq!(screen.message, None);
        assert!(state().equipped(GRUBSONG));
        screen.at_bench = false;
        screen.step(CROSS);
        assert_eq!(screen.message, Some(Refusal::NotAtBench));
        assert!(state().equipped(GRUBSONG));
        // An empty slot answers as it does at a bench: `Got Charm` comes first.
        screen.row = 0;
        screen.step(CROSS);
        assert_eq!(screen.message, Some(Refusal::NotOwned));
        *state() = State::new();
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
                // `Charm Equip Msg` stands in for the description and takes the footer's place.
                c.name.len() + c.text.len().max(BENCH_REMINDER.len())
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
