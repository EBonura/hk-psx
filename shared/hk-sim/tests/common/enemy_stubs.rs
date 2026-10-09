// The hardware-free stand-ins the guest enemy module needs to compile natively.
// Shared by tests/enemy_runtime.rs and tools/behaviour-parity, which both include this file
// at their crate root (the guest module reaches these through `crate::`).
// cheats composes the equipped charms into the live parameters and reconciles
// the all-charms grant. The real module links the cooked catalogue this harness
// has no manifest directory for, and an empty board composes to the base
// anyway, so the contract is stubbed here; tests/charms_runtime.rs exercises
// the real one.
mod charms{
 pub fn vitals(base:hk_sim::VitalParams)->hk_sim::VitalParams{base}
 pub fn grant_all(_:bool){}
 pub fn on_damage(_:&mut hk_sim::Vitals,_:hk_sim::VitalParams){}
}
// And the shop's half of PlayerData, which `cheats::params` composes under
// the charms. The real module `include!`s the cooked stock relative to its own
// crate, which this one is not, and a shop nobody has visited composes to the
// base anyway; tests/shop_runtime.rs exercises the real one.
mod shop{
 pub fn vitals(base:hk_sim::VitalParams)->hk_sim::VitalParams{base}
}
#[path="../../../../game/src/cheats.rs"]mod cheats;
const NAIL_RESPONSE_PARAMS: NailResponseParams = NailResponseParams {
    recoil_ticks: 11,
    recoil_speed: 245760,
    bounce_ticks: 15,
    high_bounce_ticks: 17,
    bounce_speed: 786432,
    down_speed: 0,
};
const KNIGHT_SCALE: i32 = 60693;
const DREAM_NAIL_POLYGON: &[[i32;2]] = &[[0,0],[0,0],[0,0]];
const PARAMS: Params = Params { speed: 8 * ONE, jump: 16 * ONE, gravity: 48 * ONE, fall: 20 * ONE, hold_ticks: 12, min_ticks: 5, half_width: ONE / 4, bottom: -ONE, ..Params::ZERO };
const VITAL_PARAMS: VitalParams = VitalParams {
    max_health: 5,
    max_soul: 99,
    nail_damage: 5,
    soul_per_hit: 11,
    invulnerable_ticks: 79,
    hazard_invulnerable_ticks: 40,
    recoil_ticks: 12,
    freeze_ticks: 19,
    death_ticks: 171,
    recoil_speed: 15 * ONE,
};
mod render {
    std::thread_local! {
        /// Submitted quads, so a frame drawn as several tiles can be checked
        /// tile by tile instead of only counted.
        pub static QUADS: std::cell::RefCell<Vec<(usize, [(i16, i16); 4])>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    pub fn texture(id: usize, verts: [(i16, i16); 4], _: (u8, u8, u8)) {
        QUADS.with(|q| q.borrow_mut().push((id, verts)));
    }
    std::thread_local! {
        /// Hit-flash overlays, with the tint each was modulated by.
        pub static FLASHES: std::cell::RefCell<Vec<(usize, (u8, u8, u8))>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    pub fn texture_flash(id: usize, _: [(i16, i16); 4], tint: (u8, u8, u8)) {
        FLASHES.with(|q| q.borrow_mut().push((id, tint)));
    }
    /// Every fixture texture streams, so the key budget is what it always was.
    pub fn streamed(_: usize) -> bool { true }
}
/// The False Knight's cooked bank, in place of the real table
/// (`game/src/fk_art.rs` includes it from `data/`). Each art clip stands on
/// the fixture clip the six-clip port used to substitute for it, so `boss_room`
/// draws texture 4 for the open armour exactly as it did; the Head, the Death
/// Head and the floor are sprites with no parts here.
/// `game/src/boss_art.rs`'s `Bank`, answered by each fixture module's own
/// functions: the False Knight's sprites stand on fixture clips as above, and
/// the Mawlek's bank has no art in these fixtures.
mod boss_art {
    use hk_format::Room;
    pub struct Bank { pub mawlek: bool }
    impl Bank {
        pub fn frame(&self, clip: usize, tick: u32) -> Option<usize> {
            if self.mawlek { None } else { Some(crate::fk_art::sprite(clip, tick)) }
        }
        pub fn sprite(&self, clip: usize, tick: u32) -> usize { crate::fk_art::sprite(clip, tick) }
        pub fn parts(&self, sprite: usize) -> usize { if self.mawlek { 0 } else { crate::fk_art::parts(sprite) } }
        pub fn part(&self, room: &Room, sprite: usize, index: usize) -> (u16, [i32; 4]) {
            crate::fk_art::part(room, sprite, index)
        }
    }
}
/// Brooding Mawlek's cooked geometry, as data/mawlek_art.rs carries it for
/// Crossroads_09; no fixture here seats a Mawlek.
mod mawlek_art {
    pub const BANK: crate::boss_art::Bank = crate::boss_art::Bank { mawlek: true };
    pub const MW_SCENE: usize = 10;
    pub const SHOT: usize = 20;
    pub const SHOT_IMPACT: usize = 21;
    pub const SPIT: usize = 22;
    pub const CORPSE: usize = 23;
    pub const MW_ART_CLIPS: [(u16, u8, u32, u8, u8); 24] = [(0, 0, 65536, 2, 0); 24];
    pub const MW_DUMMY_OFFSET: [i32; 2] = [0, 17467];
    pub const MW_ARM_OFFSET: [[i32; 2]; 2] = [[201327, 5046], [-209977, 5046]];
    pub const MW_HEAD_OFFSET: [i32; 2] = [-3932, 120390];
    pub const MW_SPIT_OFFSET: [i32; 2] = [14942, 258081];
    pub const MW_HEAD_BOX: [i32; 4] = [-70287, 71954, 56279, 127148];
    pub const MW_ARM_RANGE: [[i32; 4]; 2] = [[6685, -201797, 318189, 146668], [-326839, -201797, -15335, 146668]];
    pub const MW_ARM_HITBOX: [[i32; 4]; 2] = [[87663, -156029, 334037, 143593], [-342688, -156029, -96313, 143593]];
    pub const MW_CORPSE_BOX: [i32; 4] = [-98959, -148767, 98959, 50463];
    pub const MW_CORPSE_FLING: i32 = 655360;
}
/// Gruz Mother's cooked geometry, as data/gruz_art.rs carries it for
/// Crossroads_04 (host/false_knight_art.py's Gruz Mother section); no art.
mod gruz_art {
    pub const BANK: crate::boss_art::Bank = crate::boss_art::Bank { mawlek: true };
    pub const GZ_DAMAGER_BOX: [i32; 4] = [-75520, -61440, 90880, 61440];
    pub const GZ_RANGE: [[i32; 2]; 8] = [[-844233, 139318], [-839924, 424814], [-961761, 527197], [-1006723, 733500],
        [192706, 690271], [194037, -154763], [-949781, -161938], [-924533, 89887]];
    pub const GZ_BURSTER_BOX: [i32; 4] = [-96000, -122880, 103680, 16640];
    pub const GZ_SPRITE_BOX: [[i32; 4]; 0] = [];
}
mod fk_art {
    use hk_format::Room;
    pub const BANK: crate::boss_art::Bank = crate::boss_art::Bank { mawlek: false };
    pub const BODY: usize = hk_sim::false_knight::CLIP_TICKS.len();
    pub const HEAD_IDLE: usize = BODY + 1;
    pub const HEAD_HIT: usize = BODY + 2;
    pub const HEAD_SPAZ: usize = BODY + 3;
    pub const DEATH_HEAD_1: usize = BODY + 4;
    pub const DEATH_HEAD_2: usize = BODY + 5;
    const NOTHING: usize = 99;
    pub const FK_FLOOR_CRACKED: [u16; 0] = [];
    pub const FK_FLOOR_BROKEN: [u16; 0] = [];
    pub const FK_FLOOR_ANCHOR: [i32; 2] = [0, 0];
    pub const FK_DEATH_HEAD_OFFSET: [i32; 2] = [0, 0];
    pub const FK_DEATH_HEAD_SPEED: i32 = 4 * hk_sim::ONE;
    pub const SPURT: usize = BODY + 6;
    // The wave's cooked constants, as host/false_knight_art.py reads them from
    // `Shockwave Wave` and `Shockwave Spurt` (sharedassets48.assets:63, :68).
    pub const FK_WAVE_ORIGIN_Y: i32 = -380109;
    pub const FK_WAVE_START_SPEED: i32 = 36045;
    pub const FK_WAVE_ACCEL: i32 = 44 * hk_sim::ONE;
    pub const FK_WAVE_BOX: [i32; 4] = [-37090, 21182, 5971, 115753];
    pub const FK_WAVE_GROUND_RAY: i32 = 104858;
    pub const FK_SPURT_BOX: [i32; 4] = [-16352, -118, 10151, 114151];
    pub const FK_SPURT_DAMAGE_FROM: u16 = 3;
    pub const FK_SPURT_DAMAGE_TO: u16 = 6;
    pub const FK_SPURT_DAMAGE: u16 = 1;
    pub const FK_SPURT_TICKS: u16 = 18;
    pub const FK_FLOOR_BREAK_SILENCE_TICKS: u16 = 120;
    pub const WAVE: hk_sim::shockwave::Params = hk_sim::shockwave::Params {
        start_speed: FK_WAVE_START_SPEED, accel: FK_WAVE_ACCEL, wave_box: FK_WAVE_BOX,
        ground_ray: FK_WAVE_GROUND_RAY, spurt_box: FK_SPURT_BOX, damage_from: FK_SPURT_DAMAGE_FROM,
        damage_to: FK_SPURT_DAMAGE_TO, damage: FK_SPURT_DAMAGE, spurt_ticks: FK_SPURT_TICKS,
    };
    pub fn sprite(clip: usize, _tick: u32) -> usize {
        use hk_sim::false_knight::Clip::*;
        const CLIPS: [hk_sim::false_knight::Clip; 25] = [Idle, Turn, JumpAntic, Jump, Land, Run, RunAntic,
            JumpAttackUp, JumpAttackHit1, JumpAttackHit2, JumpAttackHit3, AttackAntic, Attack, AttackRecover,
            Rage, StunRoll, StunRollEnd, StunOpen, StunOpened, StunHit, StunRecover, DeathFall, DeathLand,
            DeathSpaz, Blank];
        if clip >= CLIPS.len() { return NOTHING; }
        match CLIPS[clip] {
            Turn => 1,
            JumpAntic | Jump | JumpAttackUp => 2,
            Land | JumpAttackHit1 | JumpAttackHit2 | JumpAttackHit3 | StunRoll | StunRollEnd
            | StunRecover | DeathFall | DeathLand => 3,
            StunOpen | StunOpened | StunHit => 4,
            Attack | AttackAntic | AttackRecover | Rage => 5,
            Blank => NOTHING,
            _ => 0,
        }
    }
    pub fn last_sprite(_clip: usize) -> usize { NOTHING }
    pub fn parts(sprite: usize) -> usize { if sprite == NOTHING { 0 } else { 1 } }
    pub fn part(room: &Room, sprite: usize, _index: usize) -> (u16, [i32; 4]) {
        let f = room.frame(room.clip(sprite)[0] as usize);
        let word = |at: usize| u32::from_le_bytes([f[at], f[at + 1], f[at + 2], f[at + 3]]);
        (word(0) as u16, core::array::from_fn(|k| word(4 + k * 4) as i32))
    }
}
mod world {
    #[derive(Clone, Copy)]
    pub struct Region {
        pub scene: usize,
        pub bounds: [i32; 4],
        pub collision_bounds: [i32; 4],
        /// One entry per placement: where it stands, and the scene-shared
        /// `ActorSpec` type it is a placement of.
        pub actors: &'static [(hk_sim::ActorPlacement, &'static hk_sim::ActorSpec)],
    }
    pub fn region_actors(
        region: &Region,
    ) -> impl Iterator<Item = (hk_sim::ActorPlacement, &'static hk_sim::ActorSpec)> {
        region.actors.iter().copied()
    }
    pub fn region_actors_indexed(
        region: &Region,
    ) -> impl Iterator<Item = (usize, hk_sim::ActorPlacement, &'static hk_sim::ActorSpec)> {
        region.actors.iter().enumerate().map(|(i, &(p, s))| (i, p, s))
    }
    pub fn region_actor(region: &Region, index: usize) -> Option<(hk_sim::ActorPlacement, &'static hk_sim::ActorSpec)> {
        region.actors.get(index).copied()
    }
    /// A fixture region is its placement list: the same list is the same region.
    pub fn actors_key(region: &Region) -> Option<usize> {
        Some(region.actors.as_ptr() as usize ^ region.actors.len() << 20 ^ region.scene << 28)
    }
    /// One fixture spike: a down-slash-only target owned by breakable 0.
    pub struct PogoTarget;
    impl PogoTarget {
        pub fn bounds(&self) -> [i32; 4] { [-hk_sim::ONE, -hk_sim::ONE, hk_sim::ONE, 0] }
        pub fn breakable(&self) -> Option<usize> { Some(0) }
        pub fn horizontal_and_up(&self) -> bool { false }
        pub fn hit_by(&self, polygon: &[[i32; 2]]) -> bool {
            let one = hk_sim::ONE;
            polygons_overlap(polygon, &[[-one, -one], [one, -one], [one, 0], [-one, 0]])
        }
    }
    pub fn pogo_targets(_: &Region) -> impl Iterator<Item = PogoTarget> { core::iter::once(PogoTarget) }
    pub mod debris {
        /// Cardinal-only rotation for the enemy draw test; the guest uses the debris sine table.
        pub fn rotate(p: [i32; 2], angle: i32) -> [i32; 2] {
            match (angle / (90 * hk_sim::ONE)).rem_euclid(4) { 0 => p, 1 => [-p[1], p[0]], 2 => [-p[0], -p[1]], _ => [p[1], -p[0]] }
        }
        /// The guest's one-rotation-per-quad form, over the same cardinal stub.
        #[derive(Clone, Copy)]
        pub struct Rotation(i32);
        impl Rotation {
            pub fn new(angle: i32) -> Self { Self(angle) }
            pub fn apply(self, p: [i32; 2]) -> [i32; 2] { rotate(p, self.0) }
        }
    }
    pub struct State;
    std::thread_local!{pub static DEATHS:std::cell::RefCell<Vec<(u32,[i32;3])>>=const{std::cell::RefCell::new(Vec::new())};}
    impl State {
        pub fn enemy_death_particles(&mut self,_:&Region,source:u32,position:[i32;3]) {DEATHS.with(|v|v.borrow_mut().push((source,position)));}
        pub fn broken(&self, _: usize) -> bool {
            false
        }
    }
    pub fn polygons_overlap(a: &[[i32; 2]], b: &[[i32; 2]]) -> bool {
        let bounds = [
            b.iter().map(|p| p[0]).min().unwrap(),
            b.iter().map(|p| p[1]).min().unwrap(),
            b.iter().map(|p| p[0]).max().unwrap(),
            b.iter().map(|p| p[1]).max().unwrap(),
        ];
        hk_sim::polygon_hits_box(a, bounds)
    }
    impl State {
        pub fn fill_edges_in_view(&self, _: &Region, _: &hk_format::Room, _: &Region, r: &hk_format::Room, edges:&mut [[i32;4]]) {
            for(i,edge)in edges.iter_mut().enumerate(){*edge=r.edge(i);}
        }
        pub fn edge_in_view(&self, _: &Region, _: &hk_format::Room, _: &Region, r: &hk_format::Room, i: usize) -> [i32; 4] {
            r.edge(i)
        }
        pub fn edge(&self, _: &Region, r: &hk_format::Room, i: usize) -> [i32; 4] {
            r.edge(i)
        }
    }
    pub fn contains(b: [i32; 4], x: i32, y: i32) -> bool {
        x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3]
    }
}
/// The port's SceneData and PlayerData, which the arena's `Activated` bool and
/// `falseKnightFirstPlop` ride. The real store is `game/src/persist.rs`'s
/// `Store`, path-included here so the arena reads and writes the same type the
/// guest does; only the guest's one global becomes a per-thread one, which
/// keeps the parallel cases apart.
#[path="../../../../game/src/persist.rs"]
#[allow(dead_code)]
mod persist_store;
mod persist {
    pub use super::persist_store::*;
    std::thread_local! {
        pub static STORE: std::cell::RefCell<Store> = const { std::cell::RefCell::new(Store::new()) };
    }
    pub fn get(kind: Kind, scene: usize, local: usize) -> Option<u8> { STORE.with(|s| s.borrow().get(kind, scene, local)) }
    pub fn set(kind: Kind, scene: usize, local: usize, value: u8) { STORE.with(|s| { s.borrow_mut().set(kind, scene, local, value); }) }
    pub fn player(bit: u32) -> bool { STORE.with(|s| s.borrow().player(bit)) }
    pub fn set_player(bit: u32) { STORE.with(|s| s.borrow_mut().set_player(bit)) }
}
/// The arena gates. The real module links the cooked `(slot, gate, edges)`
/// table, which `tests/battle_gates_runtime.rs` drives directly; what this
/// harness has to see is that every broadcast leaves the boss with the room it
/// names, so they are recorded in order.
mod battle_gates {
    std::thread_local! {
        pub static EVENTS: std::cell::RefCell<Vec<(&'static str, usize)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    fn record(event: &'static str, scene: usize) { EVENTS.with(|e| e.borrow_mut().push((event, scene))); }
    pub fn arena_entry(scene: usize, activated: bool) {
        record(if activated { "quick open" } else { "placement" }, scene);
    }
    pub fn close(scene: usize) { record("close", scene); }
    pub fn open(scene: usize) { record("open", scene); }
    pub const FLOOR_WHOLE: u8 = 0;
    pub const FLOOR_CRACKED: u8 = 1;
    pub const FLOOR_BROKEN: u8 = 2;
    std::thread_local! { static FLOOR: std::cell::Cell<u8> = const { std::cell::Cell::new(0) }; }
    pub fn floor_crack() { record("crack", 0); FLOOR.with(|f| f.set(FLOOR_CRACKED)); }
    pub fn floor_break() { record("break", 0); FLOOR.with(|f| f.set(FLOOR_BROKEN)); }
    pub fn floor() -> u8 { FLOOR.with(|f| f.get()) }
    pub fn armour_tink_hidden(_: [i32; 4]) -> bool { false }
}
/// The camera and the SPU, which the boss reaches through two global modules
/// the real guest keeps in `static mut`. A shared static cannot be asserted
/// against while the other cases in this file run beside it, so both record
/// into one per-thread log here. One log rather than two, because the order is
/// the interesting part: `S Attack` swings and then `Slam` shakes, where the
/// jump attack's `JA Slam` shakes and then `JA Hit 2` swings.
mod boss_effects {
    std::thread_local! {
        pub static LOG: std::cell::RefCell<Vec<&'static str>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    pub fn record(what: &'static str) { LOG.with(|l| l.borrow_mut().push(what)); }
    pub fn take() -> Vec<&'static str> { LOG.with(|l| l.borrow_mut().drain(..).collect()) }
}
mod camera {
    /// `game/src/camera.rs` declares these; `tests/camera_shake_runtime.rs`
    /// drives the real module over the serialized `CameraShake` table.
    #[allow(dead_code)]
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Shake { Small, Kill, Average, Big }
    pub fn request(kind: Shake) {
        crate::boss_effects::record(match kind {
            Shake::Small => "shake small",
            Shake::Kill => "shake kill",
            Shake::Average => "shake average",
            Shake::Big => "shake big",
        });
    }
}
mod audio {
    pub fn boss_land() { crate::boss_effects::record("land voice"); }
    pub fn boss_swing() { crate::boss_effects::record("swing voice"); }
}
/// The boss name cards; only the two titles the bosses ask for.
mod title_card { pub const FALSE_KNIGHT: u8 = 0; pub const MAWLEK: u8 = 1; pub const BIGFLY: u8 = 2; pub fn show_boss(_title: u8) {} }
/// Each actor carries the off-view resolver's cached answer; these fixtures
/// resolve through their own closures, which ignore it. The stand-in has the
/// real one's size (three u16 and a four-word box), so the size ceiling
/// below measures what the guest pays.
mod music {
    pub const MAWLEK_TRACK: u8 = 4;
    pub const BOSS_DEFEAT_TRACK: u8 = 5;
    pub fn boss(_active: bool) {}
    pub fn boss_track(_track: u8) {}
    pub fn sting(_track: u8) {}
    pub fn boss_silence(_ticks: u16) {}
}
/// The Blockers' Terrain Block table, which `game/src/blocker_terrain.rs` joins
/// against the cooked regions; here only its two calls, recorded.
/// The per-scene one-shot banks; only the event ids the actors name.
mod scene_sfx {
    pub const ASPID_SPIT: u8 = 0;
    pub const FALSE_KNIGHT_STRIKE_GROUND: u8 = 1;
    pub const FALSE_KNIGHT_RAGE: u8 = 2;
    pub const BOSS_FINAL_HIT: u8 = 3;
    pub const FALSE_KNIGHT_JUMP: u8 = 4;
    pub const FALSE_KNIGHT_LAND_1ST_TIME: u8 = 5;
    pub const FALSE_KNIGHT_DAMAGE_ARMOUR_FINAL: u8 = 6;
    pub const FALSE_KNIGHT_ROLL: u8 = 7;
    pub const FALSE_KNIGHT_DEATH: u8 = 8;
    pub const FALSE_KNIGHT_CEILING_BREAK: u8 = 9;
    pub const ZOMBIE_SHIELD_RAISE: u8 = 10;
    pub const ZOMBIE_GUARD_CLUB: u8 = 11;
    pub const MAWLEK_JUMP: u8 = 12;
    pub const MAWLEK_SPIT: u8 = 13;
    pub const MAWLEK_WHIP: u8 = 14;
    pub const MAWLEK_CALL: u8 = 15;
    pub const MAWLEK_BIG_SPIT: u8 = 16;
    pub const MAWLEK_SCREAM: u8 = 17;
    pub const HERO_PARRY: u8 = 18;
    pub const BOSS_EXPLODE: u8 = 19;
    pub const BOSS_GUSHING: u8 = 20;
    pub const MAWLEK_JUMP_OFFSCREEN: u8 = 21;
    pub const MAWLEK_SPIT_B: u8 = 22;
    pub const ZOMBIE_SHIELD_MOVE: u8 = 23;
    pub const ZOMBIE_GUARD_FOOTSTEP: u8 = 24;
    pub const BIG_FLY_WALL_HIT: u8 = 25;
    pub const BIG_FLY_SNORE_STARTLE: u8 = 26;
    pub const BIG_FLY_STOMACHE_PROBLEMS_1: u8 = 27;
    pub const BIG_FLY_STOMACHE_PROBLEMS_2: u8 = 26;
    pub const BIG_FLY_STOMACHE_PROBLEMS_FINAL_AND_EXPLODE: u8 = 27;
    pub const GRUZ_FINAL_HIT: u8 = 28;
    pub const GRUZ_EXPLODE: u8 = 29;
    pub const GRUZ_GUSHING: u8 = 30;
    pub fn play(event: u8) { if event != ASPID_SPIT { crate::boss_effects::record("scene voice"); } }
    pub fn play_shared(_event: u8) { crate::boss_effects::record("scene clip, shared voice"); }
}
// The spat Roller needs the spell, which no enemy harness case has.
mod blocker_roller { pub fn can_spawn() -> bool { false } }
mod blocker_terrain {
    std::thread_local! {
        pub static KILLED: std::cell::RefCell<Vec<(usize, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    pub fn dead(_scene: usize, _source_id: u32) -> bool { false }
    pub fn killed(scene: usize, source_id: u32) { KILLED.with(|k| k.borrow_mut().push((scene, source_id))); }
}
mod modules {
    pub const FALSE_KNIGHT: usize = 0;
    pub const MAWLEK: usize = 1;
    pub const HUSKS: usize = 2;
    pub const VENGEFLY: usize = 3;
    pub const GRUZZER: usize = 4;
    pub const BALDUR: usize = 5;
    pub const ASPID: usize = 6;
    pub const HATCHER: usize = 7;
    pub const BLOCKER: usize = 8;
    pub const PIGEON: usize = 9;
    pub const CLIMBER: usize = 10;
    pub const GRUZ_MOTHER: usize = 11;
    pub const ACID_FLYER: usize = 12;
    pub const MOSQUITO: usize = 13;
    pub const MOSS_WALKER: usize = 14;
    pub fn require(_k: usize) -> bool { true }
}
mod disc {
    #[derive(Clone, Copy)]
    pub struct Located { _key: [u16; 3], _rect: [i32; 4] }
    impl Located { pub const NONE: Self = Located { _key: [0; 3], _rect: [0; 4] }; }
}
