//! The wire form of `ActorSpec` and `CorpseSpec`: how a scene's enemy types travel on the disc and
//! come back as plain values (host/hk-specpack writes them, game/src/spec_pool.rs reads them).
//!
//! Little-endian, field by field in the order written below; no struct, enum or pointer is ever
//! cast from bytes. The only reference a spec holds, `ActorSpec::corpse`, travels as the index of
//! a record in the same chunk's corpse list (`NO_CORPSE` for none); the decoder resolves it
//! against the slice of decoded corpses it is given. Every `match` below is exhaustive on
//! purpose: a new `ActorController` variant or `runner::Attack` does not compile until it has a
//! tag here, which is what keeps a family added later (the Shaker's gas attack, say) honest.
//! Reading is bounds-checked and refuses unknown tags, so a bad chunk is an `Err`, not a stray
//! variant.
use crate::runner::{Attack, Params};
use crate::{ActorController, ActorSpec, CorpseSpec, EnemyParams, WalkParams};

/// `ActorSpec::corpse` index meaning no corpse.
pub const NO_CORPSE: u8 = 0xFF;
/// Bytes the widest encoded `ActorSpec` may take; the writer refuses a spec that needs more.
pub const MAX_SPEC_BYTES: usize = 192;
/// Bytes of one encoded `CorpseSpec`.
pub const CORPSE_BYTES: usize = 46;

pub struct Writer<'a> {
    buf: &'a mut [u8],
    len: usize,
    overflow: bool,
}
impl<'a> Writer<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0, overflow: false }
    }
    /// Bytes written; `None` when the buffer was too small.
    pub fn finish(self) -> Option<usize> {
        (!self.overflow).then_some(self.len)
    }
    fn put(&mut self, bytes: &[u8]) {
        match self.buf.get_mut(self.len..self.len + bytes.len()) {
            Some(slot) => slot.copy_from_slice(bytes),
            None => self.overflow = true,
        }
        self.len += bytes.len();
    }
    pub fn u8(&mut self, v: u8) {
        self.put(&[v]);
    }
    pub fn bool(&mut self, v: bool) {
        self.put(&[v as u8]);
    }
    pub fn u16(&mut self, v: u16) {
        self.put(&v.to_le_bytes());
    }
    pub fn i16(&mut self, v: i16) {
        self.put(&v.to_le_bytes());
    }
    pub fn i32(&mut self, v: i32) {
        self.put(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.put(&v.to_le_bytes());
    }
    fn i32s(&mut self, v: &[i32]) {
        for &x in v {
            self.i32(x);
        }
    }
    fn u16s(&mut self, v: &[u16]) {
        for &x in v {
            self.u16(x);
        }
    }
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    bad: bool,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0, bad: false }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn ok(&self) -> bool {
        !self.bad
    }
    /// The next `n` (at most 4) bytes, little-endian, as a number; zero and `bad` when they are not there.
    /// One out-of-line copy serves every field: the decoder reads some forty of them per spec, and
    /// inlined each became its own bounds check in the resident image.
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn take(&mut self, n: usize) -> u32 {
        match self.bytes.get(self.pos..self.pos + n) {
            Some(src) => {
                self.pos += n;
                src.iter().rev().fold(0u32, |v, &b| (v << 8) | u32::from(b))
            }
            None => {
                self.bad = true;
                self.pos += n;
                0
            }
        }
    }
    pub fn u8(&mut self) -> u8 {
        self.take(1) as u8
    }
    pub fn bool(&mut self) -> bool {
        match self.u8() {
            0 => false,
            1 => true,
            _ => {
                self.bad = true;
                false
            }
        }
    }
    pub fn u16(&mut self) -> u16 {
        self.take(2) as u16
    }
    pub fn i16(&mut self) -> i16 {
        self.take(2) as u16 as i16
    }
    pub fn i32(&mut self) -> i32 {
        self.take(4) as i32
    }
    pub fn u32(&mut self) -> u32 {
        self.take(4)
    }
    fn i32s<const N: usize>(&mut self) -> [i32; N] {
        let mut out = [0i32; N];
        self.fill_i32(&mut out);
        out
    }
    fn u16s<const N: usize>(&mut self) -> [u16; N] {
        let mut out = [0u16; N];
        self.fill_u16(&mut out);
        out
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn fill_i32(&mut self, out: &mut [i32]) {
        for slot in out.iter_mut() {
            *slot = self.i32();
        }
    }
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    fn fill_u16(&mut self, out: &mut [u16]) {
        for slot in out.iter_mut() {
            *slot = self.u16();
        }
    }
}

pub fn encode_corpse(c: &CorpseSpec, w: &mut Writer) {
    w.u16(c.air_clip);
    w.u16(c.land_clip);
    w.i32s(&c.bounds);
    w.i32s(&c.spawn_offset);
    w.i32(c.bounce_factor);
    w.i32(c.fling_speed);
    w.i32(c.gravity);
    w.bool(c.breaker);
    w.u8(c.smash_bounces);
    w.u16(c.remove_after_land);
    w.u16(c.hold_ticks);
}

#[inline(never)]
#[cfg_attr(target_arch = "mips", optimize(size))]
pub fn decode_corpse(r: &mut Reader) -> CorpseSpec {
    CorpseSpec {
        air_clip: r.u16(),
        land_clip: r.u16(),
        bounds: r.i32s(),
        spawn_offset: r.i32s(),
        bounce_factor: r.i32(),
        fling_speed: r.i32(),
        gravity: r.i32(),
        breaker: r.bool(),
        smash_bounces: r.u8(),
        remove_after_land: r.u16(),
        hold_ticks: r.u16(),
    }
}

fn encode_attack(a: &Attack, w: &mut Writer) {
    match *a {
        Attack::Swipe => w.u8(0),
        Attack::Leap { trigger_ticks, jump_speed_y, jump_x_factor, idle_ticks } => {
            w.u8(1);
            w.u16(trigger_ticks);
            w.i32(jump_speed_y);
            w.i32(jump_x_factor);
            w.u16(idle_ticks);
        }
        Attack::SwipeCalm => w.u8(2),
        Attack::Gas { delay, antic_ticks, attack_ticks, cd_ticks, idle_ticks } => {
            w.u8(3);
            w.u16(delay);
            w.u16(antic_ticks);
            w.u16(attack_ticks);
            w.u16(cd_ticks);
            w.u16(idle_ticks);
        }
    }
}

#[inline(never)]
#[cfg_attr(target_arch = "mips", optimize(size))]
fn decode_attack(r: &mut Reader) -> Option<Attack> {
    Some(match r.u8() {
        0 => Attack::Swipe,
        1 => Attack::Leap {
            trigger_ticks: r.u16(),
            jump_speed_y: r.i32(),
            jump_x_factor: r.i32(),
            idle_ticks: r.u16(),
        },
        2 => Attack::SwipeCalm,
        3 => Attack::Gas {
            delay: r.u16(),
            antic_ticks: r.u16(),
            attack_ticks: r.u16(),
            cd_ticks: r.u16(),
            idle_ticks: r.u16(),
        },
        _ => return None,
    })
}

fn encode_params(p: &Params, w: &mut Writer) {
    w.i32(p.walk_speed);
    w.i32(p.lunge_speed);
    w.u16s(&p.walking_wait);
    w.u16s(&p.paused_wait);
    encode_attack(&p.attack, w);
    w.i32(p.gravity);
}

#[inline(never)]
#[cfg_attr(target_arch = "mips", optimize(size))]
fn decode_params(r: &mut Reader) -> Option<Params> {
    Some(Params {
        walk_speed: r.i32(),
        lunge_speed: r.i32(),
        walking_wait: r.u16s(),
        paused_wait: r.u16s(),
        attack: decode_attack(r)?,
        gravity: r.i32(),
    })
}

fn encode_controller(c: &ActorController, w: &mut Writer) {
    use ActorController as C;
    match *c {
        C::Crawler { recoil_steps } => {
            w.u8(0);
            w.u8(recoil_steps);
        }
        C::Mawlek { wake } => {
            w.u8(1);
            w.i32s(&wake);
        }
        C::GruzMother => w.u8(2),
        C::Runner { idle_clip, anticipate_clip, lunge_clip, cooldown_clip, ref params, alert } => {
            w.u8(3);
            w.u16s(&[idle_clip, anticipate_clip, lunge_clip, cooldown_clip]);
            encode_params(params, w);
            w.i32s(&alert);
        }
        C::Climber { stun_clip } => {
            w.u8(4);
            w.u16(stun_clip);
        }
        C::Vengefly { startle_clip, chase_clip, turn_fly_clip } => {
            w.u8(5);
            w.u16s(&[startle_clip, chase_clip, turn_fly_clip]);
        }
        C::Gruzzer => w.u8(6),
        C::GruzzerReserve { origin } => {
            w.u8(7);
            w.i32s(&origin);
        }
        C::AcidFlyer { amount, speed, lead, shell } => {
            w.u8(8);
            w.i32(amount);
            w.i32(speed);
            w.i32s(&lead);
            w.i32s(&shell);
        }
        C::Mosquito { clips, tile } => {
            w.u8(9);
            w.u16s(&clips);
            w.i32s(&tile);
        }
        C::MossWalker { clips } => {
            w.u8(10);
            w.u16s(&clips);
        }
        C::Baldur { start_clip, roll_clip, stop_clip } => {
            w.u8(11);
            w.u16s(&[start_clip, roll_clip, stop_clip]);
        }
        C::Aspid { fire_clip, shot_clip, impact_clip } => {
            w.u8(12);
            w.u16s(&[fire_clip, shot_clip, impact_clip]);
        }
        C::Hatcher { fire_clip, max_hatched } => {
            w.u8(13);
            w.u16(fire_clip);
            w.u8(max_hatched);
        }
        C::HatcherBaby => w.u8(14),
        C::ZombieShield { clips, attack } => {
            w.u8(15);
            w.u16s(&clips);
            w.i32s(&attack);
        }
        C::HuskGuard { clips, spurt_clip, slam_clip } => {
            w.u8(16);
            w.u16s(&clips);
            w.u16s(&[spurt_clip, slam_clip]);
        }
        C::Blocker { clips, shot_clip, impact_clip, sleeps } => {
            w.u8(17);
            w.u16s(&clips);
            w.u16s(&[shot_clip, impact_clip]);
            w.bool(sleeps);
        }
        C::Pigeon { clips } => {
            w.u8(18);
            w.u16s(&clips);
        }
        C::FsmActor { family, clips, trigger } => {
            w.u8(19);
            w.u8(family);
            w.u16s(&clips);
            w.i32s(&trigger);
        }
        C::Static { idle_clip } => {
            w.u8(20);
            w.u16(idle_clip);
        }
        C::FalseKnight {
            jump_antic_clip,
            land_clip,
            stun_opened_clip,
            attack_clip,
            trigger,
            barrel_clip,
            barrel_spawn_y,
        } => {
            w.u8(21);
            w.u16s(&[jump_antic_clip, land_clip, stun_opened_clip, attack_clip]);
            w.i32s(&trigger);
            w.u16(barrel_clip);
            w.i32(barrel_spawn_y);
        }
    }
}

/// Field widths of each controller tag's payload, in wire order: 1 `u8`, 2 `u16`, 4 `i32`, 5 `bool`. The
/// decoder reads a payload with one loop over this table and builds the variant from the numbers, which
/// takes a fraction of the code of a reader call per field (`Runner`, tag 3, carries a nested enum and
/// is read by hand).
const SHAPES: [&[u8]; 22] = [
    &[1],
    &[4, 4, 4, 4],
    &[],
    &[],
    &[2],
    &[2, 2, 2],
    &[],
    &[4, 4],
    &[4, 4, 4, 4, 4, 4, 4, 4],
    &[2, 2, 2, 2, 4, 4, 4, 4],
    &[2, 2, 2, 2],
    &[2, 2, 2],
    &[2, 2, 2],
    &[2, 1],
    &[],
    &[2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 4, 4, 4, 4],
    &[2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
    &[2, 2, 2, 2, 2, 2, 2, 2, 5],
    &[2, 2],
    &[1, 2, 2, 2, 2, 2, 2, 2, 2, 4, 4, 4, 4],
    &[2],
    &[2, 2, 2, 2, 4, 4, 4, 4, 2, 4],
];
const MAX_FIELDS: usize = 24;

/// `a[at..at + N]` as clip ids / coordinates.
fn clips<const N: usize>(a: &[i32; MAX_FIELDS], at: usize) -> [u16; N] {
    core::array::from_fn(|i| a[at + i] as u16)
}
fn coords<const N: usize>(a: &[i32; MAX_FIELDS], at: usize) -> [i32; N] {
    core::array::from_fn(|i| a[at + i])
}

#[inline(never)]
#[cfg_attr(target_arch = "mips", optimize(size))]
fn decode_controller(r: &mut Reader) -> Option<ActorController> {
    use ActorController as C;
    let tag = r.u8() as usize;
    if tag == 3 {
        let [idle_clip, anticipate_clip, lunge_clip, cooldown_clip] = r.u16s();
        return Some(C::Runner {
            idle_clip,
            anticipate_clip,
            lunge_clip,
            cooldown_clip,
            params: decode_params(r)?,
            alert: r.i32s(),
        });
    }
    let mut a = [0i32; MAX_FIELDS];
    for (slot, &width) in a.iter_mut().zip(*SHAPES.get(tag)?) {
        *slot = match width {
            1 => r.u8() as i32,
            2 => r.u16() as i32,
            4 => r.i32(),
            _ => r.bool() as i32,
        };
    }
    Some(match tag {
        0 => C::Crawler { recoil_steps: a[0] as u8 },
        1 => C::Mawlek { wake: coords(&a, 0) },
        2 => C::GruzMother,
        4 => C::Climber { stun_clip: a[0] as u16 },
        5 => C::Vengefly { startle_clip: a[0] as u16, chase_clip: a[1] as u16, turn_fly_clip: a[2] as u16 },
        6 => C::Gruzzer,
        7 => C::GruzzerReserve { origin: coords(&a, 0) },
        8 => C::AcidFlyer { amount: a[0], speed: a[1], lead: coords(&a, 2), shell: coords(&a, 4) },
        9 => C::Mosquito { clips: clips(&a, 0), tile: coords(&a, 4) },
        10 => C::MossWalker { clips: clips(&a, 0) },
        11 => C::Baldur { start_clip: a[0] as u16, roll_clip: a[1] as u16, stop_clip: a[2] as u16 },
        12 => C::Aspid { fire_clip: a[0] as u16, shot_clip: a[1] as u16, impact_clip: a[2] as u16 },
        13 => C::Hatcher { fire_clip: a[0] as u16, max_hatched: a[1] as u8 },
        14 => C::HatcherBaby,
        15 => C::ZombieShield { clips: clips(&a, 0), attack: coords(&a, 20) },
        16 => C::HuskGuard { clips: clips(&a, 0), spurt_clip: a[12] as u16, slam_clip: a[13] as u16 },
        17 => C::Blocker {
            clips: clips(&a, 0),
            shot_clip: a[6] as u16,
            impact_clip: a[7] as u16,
            sleeps: a[8] != 0,
        },
        18 => C::Pigeon { clips: clips(&a, 0) },
        19 => C::FsmActor { family: a[0] as u8, clips: clips(&a, 1), trigger: coords(&a, 9) },
        20 => C::Static { idle_clip: a[0] as u16 },
        _ => C::FalseKnight {
            jump_antic_clip: a[0] as u16,
            land_clip: a[1] as u16,
            stun_opened_clip: a[2] as u16,
            attack_clip: a[3] as u16,
            trigger: coords(&a, 4),
            barrel_clip: a[8] as u16,
            barrel_spawn_y: a[9],
        },
    })
}

/// Write `spec`; `corpse` is the index of its corpse in the chunk's list, or `NO_CORPSE`.
pub fn encode_spec(spec: &ActorSpec, corpse: u8, w: &mut Writer) {
    encode_controller(&spec.controller, w);
    w.i32s(&spec.bounds);
    let EnemyParams { health, contact_damage, evasion_ticks, invincible, damage_override } =
        spec.health;
    w.i16(health);
    w.u16(contact_damage);
    w.u16(evasion_ticks);
    w.bool(invincible);
    w.bool(damage_override);
    w.i32(spec.walk.speed);
    w.u16(spec.walk.turn_ticks);
    w.u16(spec.walk.turn_cooldown_ticks);
    w.u16(spec.walk_clip);
    w.u16(spec.turn_clip);
    w.u8(corpse);
    w.i32(spec.recoil_speed);
    w.u16(spec.recoil_ticks);
    w.u16(spec.dream_soul);
}

/// Read one spec; its corpse reference is resolved against `corpses` (`None` when the index is
/// out of range, or the bytes are short or malformed).
#[inline(never)]
#[cfg_attr(target_arch = "mips", optimize(size))]
pub fn decode_spec(r: &mut Reader, corpses: &'static [CorpseSpec]) -> Option<ActorSpec> {
    let controller = decode_controller(r)?;
    let bounds = r.i32s();
    let health = EnemyParams {
        health: r.i16(),
        contact_damage: r.u16(),
        evasion_ticks: r.u16(),
        invincible: r.bool(),
        damage_override: r.bool(),
    };
    let walk = WalkParams { speed: r.i32(), turn_ticks: r.u16(), turn_cooldown_ticks: r.u16() };
    let (walk_clip, turn_clip) = (r.u16(), r.u16());
    let corpse = match r.u8() {
        NO_CORPSE => None,
        index => Some(corpses.get(index as usize)?),
    };
    let spec = ActorSpec {
        controller,
        bounds,
        health,
        walk,
        walk_clip,
        turn_clip,
        corpse,
        recoil_speed: r.i32(),
        recoil_ticks: r.u16(),
        dream_soul: r.u16(),
    };
    r.ok().then_some(spec)
}

/// The magic of a scene's spec chunk.
pub const CHUNK_MAGIC: [u8; 8] = *b"HKSPEC01";
/// Bytes before the payload: magic, scene id, spec count, corpse count, two zero bytes, payload length.
pub const CHUNK_HEAD: usize = 20;

/// FNV-1a, the chunk's own checksum (it covers the head and the payload, not the padding).
pub fn fnv(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for &b in bytes {
        hash = (hash ^ u32::from(b)).wrapping_mul(0x01000193);
    }
    hash
}

/// What a scene's chunk says it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkHead {
    pub scene_id: u32,
    pub specs: usize,
    pub corpses: usize,
    pub payload: usize,
}

/// Check the frame (magic, lengths, checksum) of a spec chunk and return what it holds.
pub fn read_chunk_head(bytes: &[u8]) -> Option<ChunkHead> {
    if bytes.len() < CHUNK_HEAD + 4 || bytes[..8] != CHUNK_MAGIC || bytes[18] != 0 || bytes[19] != 0 {
        return None;
    }
    let mut r = Reader::new(&bytes[8..CHUNK_HEAD]);
    let (scene_id, specs, corpses) = (r.u32(), r.u8() as usize, r.u8() as usize);
    let _ = r.u16();
    let payload = r.u32() as usize;
    let end = CHUNK_HEAD.checked_add(payload)?;
    let stored = u32::from_le_bytes(bytes.get(end..end + 4)?.try_into().ok()?);
    (fnv(&bytes[..end]) == stored).then_some(ChunkHead { scene_id, specs, corpses, payload })
}

/// Decode a checked chunk into the pools: corpses first (specs refer to them), then the specs.
/// Returns the head, or `None` for a malformed chunk or one that does not fit the pools.
#[inline(never)]
#[cfg_attr(target_arch = "mips", optimize(size))]
pub fn decode_chunk(
    bytes: &[u8],
    corpse_pool: &'static mut [CorpseSpec],
    spec_pool: &mut [ActorSpec],
) -> Option<ChunkHead> {
    let head = read_chunk_head(bytes)?;
    if head.specs > spec_pool.len() || head.corpses > corpse_pool.len() {
        return None;
    }
    let mut r = Reader::new(&bytes[CHUNK_HEAD..CHUNK_HEAD + head.payload]);
    for slot in corpse_pool[..head.corpses].iter_mut() {
        *slot = decode_corpse(&mut r);
    }
    let corpses: &'static [CorpseSpec] = &corpse_pool[..head.corpses];
    for slot in spec_pool[..head.specs].iter_mut() {
        *slot = decode_spec(&mut r, corpses)?;
    }
    (r.ok() && r.pos() == head.payload).then_some(head)
}

/// Frame `specs` and `corpses` (each spec's corpse index into `corpses`) as a chunk body into
/// `out`; the caller pads the rest of the sector-sized chunk with zeros. Returns the bytes
/// written, or `None` when `out` is too small or a count does not fit a byte.
pub fn encode_chunk(
    scene_id: u32,
    specs: &[(&ActorSpec, u8)],
    corpses: &[&CorpseSpec],
    out: &mut [u8],
) -> Option<usize> {
    if specs.len() > 254 || corpses.len() > 254 || out.len() < CHUNK_HEAD + 4 {
        return None;
    }
    let (head, body) = out.split_at_mut(CHUNK_HEAD);
    let mut w = Writer::new(body);
    for c in corpses {
        encode_corpse(c, &mut w);
    }
    for (spec, corpse) in specs {
        let mut buf = [0u8; MAX_SPEC_BYTES];
        let mut sw = Writer::new(&mut buf);
        encode_spec(spec, *corpse, &mut sw);
        let len = sw.finish()?;
        for &b in &buf[..len] {
            w.u8(b);
        }
    }
    let payload = w.finish()?;
    let mut h = Writer::new(head);
    for b in CHUNK_MAGIC {
        h.u8(b);
    }
    h.u32(scene_id);
    h.u8(specs.len() as u8);
    h.u8(corpses.len() as u8);
    h.u16(0);
    h.u32(payload as u32);
    h.finish()?;
    let end = CHUNK_HEAD + payload;
    let hash = fnv(&out[..end]);
    out.get_mut(end..end + 4)?.copy_from_slice(&hash.to_le_bytes());
    Some(end + 4)
}

/// A spec with nothing in it, the pool's filler.
pub const EMPTY_SPEC: ActorSpec = ActorSpec {
    controller: ActorController::Gruzzer,
    bounds: [0; 4],
    health: EnemyParams { health: 0, contact_damage: 0, evasion_ticks: 0, invincible: false, damage_override: false },
    walk: WalkParams { speed: 0, turn_ticks: 0, turn_cooldown_ticks: 0 },
    walk_clip: 0,
    turn_clip: 0,
    corpse: None,
    recoil_speed: 0,
    recoil_ticks: 0,
    dream_soul: 0,
};
/// A corpse with nothing in it, the pool's filler.
pub const EMPTY_CORPSE: CorpseSpec = CorpseSpec {
    air_clip: 0,
    land_clip: 0,
    bounds: [0; 4],
    spawn_offset: [0; 2],
    bounce_factor: 0,
    fling_speed: 0,
    gravity: 0,
    breaker: false,
    smash_bounces: 0,
    remove_after_land: 0,
    hold_ticks: 0,
};

/// The enemy types of the admitted scene: room for the widest scene's `S` specs and `C` corpses. One scene is in it
/// at a time; `load` replaces the previous scene's, and references taken from the previous scene's `specs` then read
/// the new scene's bytes, so whoever holds them must rebuild them for the new scene (the world's `sync_region` does
/// whenever the admitted region changes).
pub struct SpecPool<const S: usize, const C: usize> {
    specs: [ActorSpec; S],
    corpses: [CorpseSpec; C],
    /// The scene (catalogue id) the pool holds and how many specs, `usize::MAX` for none.
    loaded: (usize, usize),
}
impl<const S: usize, const C: usize> SpecPool<S, C> {
    pub const fn new() -> Self {
        Self { specs: [EMPTY_SPEC; S], corpses: [EMPTY_CORPSE; C], loaded: (usize::MAX, 0) }
    }
    /// Decode `scene_id`'s chunk into the pool. False, and an empty pool, when the chunk is malformed, is
    /// another scene's or does not fit.
    #[inline(never)]
    #[cfg_attr(target_arch = "mips", optimize(size))]
    pub fn load(&'static mut self, scene_id: usize, chunk: &[u8]) -> bool {
        self.loaded = (usize::MAX, 0);
        let corpses: &'static mut [CorpseSpec] = &mut self.corpses;
        match decode_chunk(chunk, corpses, &mut self.specs) {
            Some(head) if head.scene_id as usize == scene_id => {
                self.loaded = (scene_id, head.specs);
                true
            }
            _ => false,
        }
    }
    /// The scene's enemy types in placement order; empty for any scene but the loaded one.
    pub fn specs(&'static self, scene_id: usize) -> &'static [ActorSpec] {
        match self.loaded {
            (loaded, count) if loaded == scene_id => &self.specs[..count],
            _ => &[],
        }
    }
}
impl<const S: usize, const C: usize> Default for SpecPool<S, C> {
    fn default() -> Self {
        Self::new()
    }
}
