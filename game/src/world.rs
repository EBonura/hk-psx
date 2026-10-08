//! Cooked region indices and bounded, persistent-in-session authored state.
use hk_format::Room;
#[path = "debris.rs"]
pub mod debris;
/// A gate, the False Knight's floor or a Blocker's block changed this tick.
/// Their terrain is the scripted-edge set the `apply` chain rebuilds; the
/// simulation re-runs it at once (frame::simulate) instead of leaving it to
/// the next drawn frame, so what collides does not depend on the frame rate.
static mut SCRIPTED_TERRAIN_CHANGED: bool = false;
pub fn scripted_terrain_changed() { unsafe { SCRIPTED_TERRAIN_CHANGED = true; } }
pub fn take_scripted_terrain_changed() -> bool { unsafe { core::mem::take(&mut *(&raw mut SCRIPTED_TERRAIN_CHANGED)) } }

#[path = "particles.rs"]
pub mod particles;
use hk_sim::{AttackParams, Grass, Nail, Params, Player, ONE};

pub const BREAKABLES_PER_SCENE: usize = 128;
pub const GRASS_PER_SCENE: usize = 1024;
use hk_format::world_meta::{self as meta, KIND_ACTOR, KIND_BENCH, KIND_BREAKABLE, KIND_CAMERA_LOCK, KIND_GATE, KIND_GEO_ENEMY, KIND_GRASS, KIND_MASK_FADE, KIND_NPC, KIND_POGO, KIND_REGION_STATICS, KIND_REMOTE_MASK, KIND_REVEAL_BINDINGS, KIND_REVEAL_MASK, KIND_SHROOM};

/// The admitted scene bank. Its bytes are immutable until the next exclusive
/// metadata admission, which `State::begin_world_admission` retires views for.
#[cfg(not(test))]
fn admitted_bank() -> Option<hk_format::WorldMeta<'static>> {
    crate::disc::admitted_world_metadata()
}
#[cfg(test)]
extern crate std;
/// Host tests select the bank per thread; the native harness runs tests in parallel.
#[cfg(test)]
std::thread_local! {
    pub static TEST_BANK: core::cell::Cell<&'static [u8]> = const { core::cell::Cell::new(&[]) };
}
#[cfg(test)]
fn admitted_bank() -> Option<hk_format::WorldMeta<'static>> {
    let bytes = TEST_BANK.with(|bank| bank.get());
    (!bytes.is_empty()).then(|| hk_format::WorldMeta::parse(bytes).expect("test bank"))
}
/// Bank region of a catalogue region. The last resolution is remembered
/// because uncached edge reads ask once per edge.
#[inline(never)]
pub fn bank_region(region: &Region) -> Option<meta::Region<'static>> {
    let bank = admitted_bank()?;
    #[cfg(not(test))]
    {
        static mut LAST: (usize, usize) = (usize::MAX, 0);
        let last = unsafe { LAST };
        if last.0 == region.global_id {
            if let Some(found) = bank.region(last.1) {
                if found.global_id() as usize == region.global_id {
                    return Some(found);
                }
            }
        }
        let index = (0..bank.region_count())
            .find(|&i| bank.region(i).is_some_and(|r| r.global_id() as usize == region.global_id))?;
        unsafe { LAST = (region.global_id, index) };
        bank.region(index)
    }
    #[cfg(test)]
    bank.region_by_global_id(region.global_id as u32)
}

/// One mask fade record: a draw list and its linear fade-to-zero duration.
#[derive(Clone, Copy)]
pub struct MaskFade<'a> {
    object: meta::Object<'a>,
}
impl<'a> MaskFade<'a> {
    pub fn draws(&self) -> meta::Indices<'a> {
        self.object.indices(0)
    }
    pub fn ticks(&self) -> u16 {
        self.object.extra(1) as u16
    }
}
/// A breakable read from the bank: its object plus the mask fades that follow it.
#[derive(Clone, Copy)]
pub struct Breakable<'a> {
    region: meta::Region<'a>,
    object: meta::Object<'a>,
    index: usize,
    fades: usize,
}
impl<'a> Breakable<'a> {
    /// Scene-global state id (scene * BREAKABLES_PER_SCENE + state).
    pub fn id(&self) -> usize {
        self.object.state_id() as usize
    }
    pub fn bounds(&self) -> [i32; 4] {
        self.object.bounds()
    }
    pub fn persistent(&self) -> bool {
        self.object.flags() & 1 != 0
    }
    pub fn door_sound(&self) -> bool {
        self.object.flags() & 4 != 0
    }
    pub fn fade_ticks(&self) -> u16 {
        self.object.flags() >> 6
    }
    pub fn off(&self) -> meta::Indices<'a> {
        self.object.indices(0)
    }
    pub fn on(&self) -> meta::Indices<'a> {
        self.object.indices(1)
    }
    pub fn edges(&self) -> meta::Indices<'a> {
        self.object.indices(2)
    }
    /// A hidden wall's or cracked floor's hit counter (flag 8), which the cook
    /// places right after its breakable.
    pub fn secret(&self) -> Option<crate::secret_breaks::Spec<'a>> {
        if self.object.flags() & 8 == 0 {
            return None;
        }
        crate::secret_breaks::Spec::after(self.region, self.index)
    }
    pub fn fades(&self) -> impl Iterator<Item = MaskFade<'a>> + 'a {
        let region = self.region;
        (self.index + 1..self.index + 1 + self.fades)
            .filter_map(move |i| region.object(i).map(|object| MaskFade { object }))
    }
    /// Exact transformed source hit polygons against a nail polygon.
    #[inline(never)]
    pub fn hit_by(&self, polygon: &[[i32; 2]]) -> bool {
        for target in self.object.polygons().flatten() {
            let mut points = [[0i32; 2]; 16];
            let mut count = 0;
            for point in target.points().take(16).flatten() {
                points[count] = point;
                count += 1;
            }
            if polygons_overlap(polygon, &points[..count]) {
                return true;
            }
        }
        false
    }
}
/// Breakables of a bank region in cooked order, each with its trailing fades.
pub struct Breakables<'a> {
    region: Option<meta::Region<'a>>,
    index: usize,
}
impl<'a> Iterator for Breakables<'a> {
    type Item = Breakable<'a>;
    #[inline(never)]
    fn next(&mut self) -> Option<Breakable<'a>> {
        let region = self.region?;
        while let Some(object) = region.object(self.index) {
            let index = self.index;
            self.index += 1;
            if object.kind() != KIND_BREAKABLE {
                continue;
            }
            let mut fades = 0;
            while region.object(index + 1 + fades).is_some_and(|o| o.kind() == KIND_MASK_FADE) {
                fades += 1;
            }
            return Some(Breakable { region, object, index, fades });
        }
        None
    }
}
pub fn breakables(region: meta::Region<'_>) -> Breakables<'_> {
    Breakables { region: Some(region), index: 0 }
}
/// Masks in this region faded by a breakable owned elsewhere:
/// (owner state id, owner total fade ticks, fade).
pub fn remote_masks<'a>(region: meta::Region<'a>) -> impl Iterator<Item = (usize, u16, MaskFade<'a>)> + 'a {
    region.objects().flatten().filter(|o| o.kind() == KIND_REMOTE_MASK)
        .map(|object| (object.state_id() as usize, object.extra(2) as u16, MaskFade { object }))
}
/// A grass patch read from the bank: scene-global state id plus the shared
/// `Grass` record (bounds, off draw, on draw).
#[derive(Clone, Copy)]
pub struct Patch {
    pub state: usize,
    pub grass: Grass,
}
pub fn grass<'a>(region: meta::Region<'a>) -> impl Iterator<Item = Patch> + 'a {
    region.objects().flatten().filter(|o| o.kind() == KIND_GRASS).map(|object| Patch {
        state: object.state_id() as usize,
        grass: Grass { bounds: object.bounds(), off_draw: object.extra(0) as usize, on_draw: object.extra(1) as usize },
    })
}
/// Grass patches of a catalogue region through the admitted bank.
pub fn region_grass(region: &Region) -> impl Iterator<Item = Patch> {
    bank_region(region).into_iter().flat_map(grass)
}
/// Supported actor placements of a catalogue region, each with the
/// `SCENE_ACTORS[scene]` type it places. The type is linked once per scene and
/// the placement comes out of the bank object, so a scene pays for the enemy
/// types it introduces rather than for the enemies it stands up.
pub fn region_actors(
    region: &Region,
) -> impl Iterator<Item = (hk_sim::ActorPlacement, &'static hk_sim::ActorSpec)> {
    let specs: &'static [hk_sim::ActorSpec] = SCENE_ACTORS[region.scene];
    bank_region(region).into_iter().flat_map(|bank| bank.objects().flatten())
        .filter(|o| o.kind() == KIND_ACTOR && o.flags() & 64 != 0)
        .map(move |o| (actor_placement(&o), &specs[o.extra(0) as usize]))
}
/// `region_actors` with each placement's object index in its bank region,
/// which `region_actor` reads back without a scan.
pub fn region_actors_indexed(
    region: &Region,
) -> impl Iterator<Item = (usize, hk_sim::ActorPlacement, &'static hk_sim::ActorSpec)> {
    let specs: &'static [hk_sim::ActorSpec] = SCENE_ACTORS[region.scene];
    bank_region(region).into_iter().flat_map(|bank| (0..bank.object_count()).filter_map(move |i| Some((i, bank.object(i)?))))
        .filter(|(_, o)| o.kind() == KIND_ACTOR && o.flags() & 64 != 0)
        .map(move |(i, o)| (i, actor_placement(&o), &specs[o.extra(0) as usize]))
}
/// The placement `region_actors_indexed` listed at `index`.
pub fn region_actor(region: &Region, index: usize) -> Option<(hk_sim::ActorPlacement, &'static hk_sim::ActorSpec)> {
    let o = bank_region(region)?.object(index)?;
    (o.kind() == KIND_ACTOR && o.flags() & 64 != 0)
        .then(|| (actor_placement(&o), &SCENE_ACTORS[region.scene][o.extra(0) as usize]))
}
/// What a table built from `region_actors` is valid for: the catalogue
/// region, whose bank bytes are the same every time its scene is admitted.
/// None while its bank is not the admitted one (it then lists nothing).
pub fn actors_key(region: &Region) -> Option<usize> {
    bank_region(region).map(|_| region.global_id)
}
/// The placement words of one `KIND_ACTOR` object (hk-format world_meta.rs).
fn actor_placement(object: &meta::Object<'_>) -> hk_sim::ActorPlacement {
    let flags = object.flags();
    hk_sim::ActorPlacement {
        source_id: object.state_id(),
        x: object.extra(1),
        y: object.extra(2),
        initial_direction: if flags & 1 != 0 { 1 } else { -1 },
        random_start_direction: flags & 2 != 0,
        start_alert: flags & 4 != 0,
        start_right: flags & 8 != 0,
        rotation_q16: ((flags >> 8) & 3) as i32 * 90 * hk_sim::ONE,
    }
}
/// Reveal mask draw bindings of a catalogue region: (controller, draw) pairs
/// from the region's single `KIND_REVEAL_BINDINGS` object.
pub fn reveal_bindings(region: &Region) -> impl Iterator<Item = crate::reveal_masks::RevealMaskBinding> {
    let mut pairs = bank_region(region).into_iter().flat_map(|bank| bank.objects().flatten())
        .filter(|o| o.kind() == KIND_REVEAL_BINDINGS).flat_map(|o| o.indices(0));
    core::iter::from_fn(move || {
        let controller = pairs.next()?;
        let draw = pairs.next()?;
        Some(crate::reveal_masks::RevealMaskBinding { controller: controller as u8, draw })
    })
}
/// A static NailSlash target read from the bank.
#[derive(Clone, Copy)]
pub struct PogoTarget<'a> {
    object: meta::Object<'a>,
}
impl<'a> PogoTarget<'a> {
    pub fn bounds(&self) -> [i32; 4] {
        self.object.bounds()
    }
    /// The owning breakable's state id; a broken owner no longer bounces.
    pub fn breakable(&self) -> Option<usize> {
        (self.object.state_id() != u32::MAX).then_some(self.object.state_id() as usize)
    }
    /// Horizontal and up slashes bounce too, not only the down slash.
    pub fn horizontal_and_up(&self) -> bool {
        self.object.flags() & 1 != 0
    }
    #[inline(never)]
    pub fn hit_by(&self, polygon: &[[i32; 2]]) -> bool {
        for target in self.object.polygons().flatten() {
            let mut points = [[0i32; 2]; 16];
            let mut count = 0;
            for point in target.points().take(16).flatten() {
                points[count] = point;
                count += 1;
            }
            if polygons_overlap(polygon, &points[..count]) {
                return true;
            }
        }
        false
    }
}
/// Source CameraLockArea touching a catalogue region: its trigger, its
/// camera-centre limits as `ValidateBounds` leaves them, and its flags
/// (`camera::LOCK_*`).
#[derive(Clone, Copy)]
pub struct CameraLock {
    /// The lock's scene-unique source id, the same in every view that lists it.
    pub id: u32,
    pub bounds: [i32; 4],
    /// [xmin, xmax, ymin, ymax] of the camera centre.
    pub limits: [i32; 4],
    pub flags: u16,
    /// The secret whose break takes this lock away (a cracked floor's), if any.
    pub owner: Option<usize>,
    /// Ticks after the scene loads that a lifetime FSM disables the lock; 0 never.
    pub expires: u16,
    object: meta::Object<'static>,
}
impl CameraLock {
    /// OnTriggerEnter2D's overlap for the Knight's body box: the bounds when
    /// they are the trigger's one box, else a body corner inside any of its
    /// trigger polygons (the body is far smaller than every lock).
    #[inline(never)]
    #[optimize(size)]
    pub fn touches(&self, body: [i32; 4]) -> bool {
        let b = self.bounds;
        if !(b[0] <= body[2] && b[2] >= body[0] && b[1] <= body[3] && b[3] >= body[1]) {
            return false;
        }
        if self.object.polygon_count() <= 1 {
            return true;
        }
        for polygon in self.object.polygons().skip(1).flatten() {
            let mut points = [[0i32; 2]; 16];
            let mut count = 0;
            for point in polygon.points().take(16).flatten() {
                points[count] = point;
                count += 1;
            }
            for corner in [[body[0], body[1]], [body[2], body[1]], [body[2], body[3]], [body[0], body[3]]] {
                if inside(&points[..count], corner) {
                    return true;
                }
            }
        }
        false
    }
}
/// The admitted scene's CameraLockAreas, which ride once in its bank's first
/// region: each one's object index there and trigger bounds, in bank order.
#[inline(never)]
#[optimize(size)]
pub fn camera_lock_objects(mut each: impl FnMut(u16, [i32; 4])) {
    let Some(region) = admitted_bank().and_then(|bank| bank.region(0)) else { return };
    for local in 0..region.object_count() {
        if let Some(o) = region.object(local).filter(|o| o.kind() == KIND_CAMERA_LOCK) {
            each(local as u16, o.bounds());
        }
    }
}
/// One of those locks by its object index.
#[inline(never)]
#[optimize(size)]
pub fn camera_lock(local: u16) -> Option<CameraLock> {
    let object = admitted_bank()?.region(0)?.object(local as usize)?;
    let polygon = object.polygons().next()?.ok()?;
    let mut points = polygon.points().flatten();
    let (low, high) = (points.next()?, points.next()?);
    Some(CameraLock { id: object.source_id(), bounds: object.bounds(), limits: [low[0], high[0], low[1], high[1]],
                      flags: object.flags(), owner: (object.state_id() != u32::MAX).then_some(object.state_id() as usize),
                      expires: object.extra(0) as u16, object })
}
/// Source RestBench touching a catalogue region.
#[derive(Clone, Copy)]
pub struct Bench {
    pub bounds: [i32; 4],
    pub seat: [i32; 2],
    /// Knight clips Sit, Sit Idle, Get Off cooked into this view.
    pub clip_base: u16,
}
pub fn benches(region: &Region) -> impl Iterator<Item = Bench> {
    bank_region(region).into_iter().flat_map(|bank| bank.objects().flatten())
        .filter(|o| o.kind() == KIND_BENCH)
        .map(|o| Bench { bounds: o.bounds(), seat: [o.extra(0), o.extra(1)], clip_base: o.extra(2) as u16 })
}
/// Source BounceShroom touching a catalogue region. The cooker admits only an
/// axis-aligned box collider, so these bounds are the trigger's exact shape.
#[derive(Clone, Copy)]
pub struct Shroom {
    pub bounds: [i32; 4],
}
pub fn shrooms(region: &Region) -> impl Iterator<Item = Shroom> {
    bank_region(region).into_iter().flat_map(|bank| bank.objects().flatten())
        .filter(|o| o.kind() == KIND_SHROOM)
        .map(|o| Shroom { bounds: o.bounds() })
}
/// A talkable NPC standing in a catalogue region: the npc_control talk trigger
/// the hero body has to be inside, where the NPC stands and the view's clip
/// base for its Idle, Talk Left and Talk Right clips. `source_id` is what binds
/// it to its cooked conversation pages.
#[derive(Clone, Copy)]
pub struct Npc {
    pub bounds: [i32; 4],
    pub position: [i32; 2],
    pub clip_base: u16,
    pub source_id: u32,
}
/// Every NPC of the admitted scene with the catalogue slot of the view holding
/// it (its clips ride in that view's room), for drawing one the camera shows
/// from a neighbouring view. Walks every view's objects: call once per scene,
/// with `step` (the pad checkpoint in the guest) run between views.
pub fn scene_npcs(scene: usize, mut step: impl FnMut(), mut found: impl FnMut(usize, Npc)) {
    let Some(bank) = admitted_bank() else { return };
    if bank.scene_id() as usize != scene { return; }
    for region in bank.regions() {
        step();
        let Some(slot) = (region.global_id() as usize).checked_sub(1) else { continue };
        for o in region.objects().flatten().filter(|o| o.kind() == KIND_NPC) {
            found(slot, Npc { bounds: o.bounds(), position: [o.extra(0), o.extra(1)],
                              clip_base: o.extra(2) as u16, source_id: o.source_id() });
        }
    }
}
pub fn npcs(region: &Region) -> impl Iterator<Item = Npc> {
    bank_region(region).into_iter().flat_map(|bank| bank.objects().flatten())
        .filter(|o| o.kind() == KIND_NPC)
        .map(|o| Npc { bounds: o.bounds(), position: [o.extra(0), o.extra(1)],
                       clip_base: o.extra(2) as u16, source_id: o.source_id() })
}
/// Static targets touching a catalogue region, through the admitted bank.
pub fn pogo_targets(region: &Region) -> impl Iterator<Item = PogoTarget<'static>> {
    bank_region(region).into_iter().flat_map(|bank| bank.objects().flatten())
        .filter(|o| o.kind() == KIND_POGO).map(|object| PogoTarget { object })
}
/// True when a broken breakable of this region owns room edge `index`.
///
/// Walks the region's objects by index rather than through `Breakables`, whose
/// trailing-fade count this does not need: it is the deepest frame of the
/// Knight's sweep, which runs on the 1 KiB scratchpad stack.
#[inline(never)]
fn broken_edge(broken: &[u32], region: &Region, index: usize) -> bool {
    let Some(bank) = bank_region(region) else { return false };
    for local in 0..bank.object_count() {
        let Some(object) = bank.object(local) else { continue };
        if object.kind() != KIND_BREAKABLE {
            continue;
        }
        let id = object.state_id() as usize;
        if broken[id / 32] & (1 << (id % 32)) != 0 && object.indices(2).any(|e| e as usize == index) {
            return true;
        }
    }
    false
}
fn region_breakables(region: &Region) -> Breakables<'static> {
    Breakables { region: bank_region(region), index: 0 }
}
/// The selected region as a runtime value: identity and bounds from the
/// admitted bank, effect tables from the per-scene statics and the variant
/// catalogues the bank's `KIND_REGION_STATICS` object indexes. No region
/// *content* is linked. Three small tables are still indexed by catalogue
/// slot, and they are the whole per-slot cost of the catalogue, seven bytes:
/// `REGION_SCENES` here (1), `disc::REGION_SCENE_LOCAL` (4) and
/// `render::SCENERY_PACKET_BUDGETS` (2).
#[derive(Clone, Copy)]
pub struct Region {
    pub global_id: usize,
    pub scene: usize,
    pub bounds: [i32; 4],
    pub collision_bounds: [i32; 4],
    pub camera: [i32; 4],
    pub grass_impact: Option<crate::impact::Spec>,
    pub particle_bank: Option<particles::Bank>,
    pub grass_emitters: &'static [particles::EmitterSpec],
    pub door_debris: &'static [debris::Spec],
}
/// The scene the False Knight is placed in, for carrying an HKS4 save's won
/// arena into the SceneData store.
pub fn false_knight_scene() -> Option<usize> {
    SCENE_ACTORS.iter().position(|specs| specs.iter()
        .any(|spec| matches!(spec.controller, hk_sim::ActorController::FalseKnight { .. })))
}
/// Scene owner of a catalogue slot.
pub fn scene_of(region_id: usize) -> usize {
    REGION_SCENES[region_id] as usize
}
/// Build the Region value for a catalogue slot from the admitted bank.
/// None while that slot's scene bank is not the admitted one.
#[inline(never)]
pub fn resident(region_id: usize) -> Option<Region> {
    let scene = *REGION_SCENES.get(region_id)? as usize;
    let bank = admitted_bank()?;
    if bank.scene_id() as usize != scene {
        return None;
    }
    let region = bank.region_by_global_id(region_id as u32 + 1)?;
    let statics = region.objects().flatten().find(|o| o.kind() == KIND_REGION_STATICS)
        .expect("region statics object in the admitted bank");
    let catalogue = |index: i32, len: usize| (index >= 0 && (index as usize) < len).then_some(index as usize);
    Some(Region {
        global_id: region_id + 1,
        scene,
        bounds: region.bounds(),
        collision_bounds: region.collision_bounds(),
        camera: region.camera(),
        grass_impact: catalogue(statics.extra(2), IMPACT_CATALOG.len()).map(|i| IMPACT_CATALOG[i]),
        particle_bank: catalogue(statics.extra(1), PARTICLE_BANKS.len()).map(|i| PARTICLE_BANKS[i]),
        grass_emitters: SCENE_EMITTERS[scene],
        door_debris: DEBRIS_CATALOG[statics.extra(0) as usize],
    })
}
#[derive(Clone, Copy)]
pub struct Gate {
    pub scene: usize,
    pub target_scene: usize,
    /// Catalogue slot of the destination region containing `spawn`.
    pub target_region: usize,
    pub bounds: [i32; 4],
    pub spawn: [i32; 2],
    /// Initial vertical velocity at the destination (Q16 units per second).
    pub entry_vy: i32,
    /// Source gate side: 1 left, 2 right, 3 top, 4 bottom, 5 a door, 0 other.
    pub side: u8,
    /// `Delay Collider` FSM: ticks after the scene starts before this gate's
    /// collider exists (top gates, 3 s).
    pub delay_ticks: u16,
}
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/regions.rs"));
#[cfg(not(test))]
const _: () = assert!(SCENES == crate::disc::SCENE_COUNT, "state tables must cover every disc scene");

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Strike {
    pub broken: u16,
    pub door_sounds: u16,
    pub grass_cut: u16,
    pub impact_dropped: u16,
    /// A hidden wall or cracked floor this swing hit or broke.
    pub secret: Option<SecretEvent>,
}
/// One accepted hit on a secret: which, its family, the hits it has taken and
/// whether that one broke it, and where its strike effect spawns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecretEvent {
    pub id: usize,
    pub family: u16,
    pub taken: u8,
    pub broke: bool,
    pub origin: [i32; 2],
}
/// Filtered terrain retained until its source region or exclusions change.
/// Coins, Scuttlers, enemies and effects otherwise re-decode and re-filter every
/// edge per body per tick, which exceeded a VBlank with 16 coins and 7 broken objects.
///
/// A room with more edges than this caches its first `EDGE_CACHE` and leaves the
/// rest to `edge_uncached`, rather than losing the table altogether. That cliff
/// was reachable: Crossroads_10's view below and right of the False Knight arena
/// (region slot 236, chunk 237) cooks 129 edges, one over, and it is the only
/// one of the 693 that passes 128 at all. Walking into it made every terrain
/// query rescan the bank's breakable objects: each simulation tick then lost two
/// VBlanks of pad service, the catch-up loop in main could never drain the
/// queue, and `Fault::QueueFull` panicked the guest into `psx_rt::halt`.
const EDGE_CACHE:usize=128;
/// Slots in the scratch the Lifeblood cocoons, the Great Door and the arena
/// gates share for their exclusions. Eight held two of these; Crossroads_10's
/// first view alone binds eight gate edges, so a third controller in that view
/// would have tripped `append_script_edges`'s assertion on the disc rather
/// than in a test. `host/battle_gates.py` reads this number and refuses a
/// binding that would not fit, which is the check that keeps it honest.
/// Twenty since the False Knight's floor joined them: a broken `Break Floor`
/// is ten edges in the arena's right-hand views, and on a won arena slot 236
/// lifts those beside two gates' seven, seventeen at once.
pub const SCRIPT_EDGE_SLOTS: usize = 20;
pub struct State {
    edge_cache:[[i32;4];EDGE_CACHE],
    edge_cache_count:usize,
    edge_cache_generation:u32,
    edge_cache_region_id:usize,
    edge_cache_room_key:usize,
    edge_cache_valid:bool,
    /// Bumped by every exclusion or source change; actor terrain scratch
    /// copies (enemies::ActorEdges) compare it instead of refilling per tick.
    pub edge_epoch:u32,
    world_generation:u32,
    geo_edges:[u16;32],
    geo_edge_count:usize,
    lifeblood_edges:[u16;SCRIPT_EDGE_SLOTS],
    lifeblood_edge_count:usize,
    debris: debris::Pool,
    pub effect_draw_dropped: u32,
    impacts: crate::impact::Pool,
    broken: [u32; SCENES * BREAKABLES_PER_SCENE / 32],
    // Grass cuts and mask fades are transient: every scene leave resets its
    // scene, and exactly one scene is resident, so these tables hold the
    // resident scene only and are indexed by the state within the scene.
    // `broken` keeps the session-persistent bit of every scene's objects.
    grass: [u32; GRASS_PER_SCENE / 32],
    pub fade_left: [u16; BREAKABLES_PER_SCENE],
    fade_active:[u32;BREAKABLES_PER_SCENE/32],
    triggers: Triggers,
    gates: Gates,
    pub secrets: crate::secret_breaks::Hits,
}
/// Most gates one scene's first region may list before `State::gate` falls
/// back to reading them from the bank. The largest on the disc lists 6.
const GATE_SLOTS: usize = 8;
/// The admitted scene's gates, decoded once, in bank order.
struct Gates {
    generation: u32,
    /// Catalogue scene id + 1; 0 before the first bind.
    scene: usize,
    count: u8,
    overflow: bool,
    list: [Gate; GATE_SLOTS],
}
const NO_GATE: Gate = Gate { scene: 0, target_scene: 0, target_region: 0, bounds: [0; 4], spawn: [0; 2],
    entry_vy: 0, side: 0, delay_ticks: 0 };
/// Most hazard and checkpoint objects one bank region may list before the
/// trigger tests fall back to scanning every object. The largest region on the
/// disc lists 17: sixteen hazards and a checkpoint.
const TRIGGER_SLOTS: usize = 24;
/// The hazard and checkpoint objects of the region the Knight stands in, in
/// cooked order with their bounds, so the two per-tick trigger tests read a
/// short table instead of decoding every bank object. Hazards come first.
struct Triggers {
    generation: u32,
    /// Bank region global id; 0 before the first bind.
    region: u32,
    hazards: u8,
    count: u8,
    overflow: bool,
    object: [u8; TRIGGER_SLOTS],
    bounds: [[i32; 4]; TRIGGER_SLOTS],
}
const fn local(id: usize) -> usize {
    id % BREAKABLES_PER_SCENE
}
fn fill_edges(geo:&[u16],life:&[u16],broken:&[u32],region:&Region,room:&Room,cache:&mut [[i32;4]]) {
    let count=cache.len();
    for(i,dst)in cache.iter_mut().enumerate(){*dst=room.edge(i);}
    let mut blank=|i:u16|{if(i as usize)<count{cache[i as usize]=[0;4];}};
    for &i in geo.iter().chain(life){blank(i);}
    for b in region_breakables(region).filter(|b|{let id=b.id();broken[id/32]&(1<<(id%32))!=0}){for i in b.edges(){blank(i);}}
}
impl State {
    pub const fn new() -> Self {
        Self {
            edge_cache:[[0;4];EDGE_CACHE],edge_cache_count:0,
            edge_cache_generation:0,edge_cache_region_id:0,edge_cache_room_key:0,edge_cache_valid:false,edge_epoch:0,
            world_generation:0,
            geo_edges:[0;32],geo_edge_count:0,
            lifeblood_edges:[0;SCRIPT_EDGE_SLOTS],lifeblood_edge_count:0,
            impacts: crate::impact::Pool::new(),
            debris: debris::Pool::new(),
            effect_draw_dropped: 0,
            broken: [0; SCENES * BREAKABLES_PER_SCENE / 32],
            grass: [0; GRASS_PER_SCENE / 32],
            fade_left: [0; BREAKABLES_PER_SCENE],
            fade_active:[0;BREAKABLES_PER_SCENE/32],
            triggers: Triggers { generation: 0, region: 0, hazards: 0, count: 0, overflow: false,
                object: [0; TRIGGER_SLOTS], bounds: [[0; 4]; TRIGGER_SLOTS] },
            gates: Gates { generation: 0, scene: 0, count: 0, overflow: false, list: [NO_GATE; GATE_SLOTS] },
            secrets: crate::secret_breaks::Hits::new(),
        }
    }
    pub fn broken(&self, id: usize) -> bool {
        self.broken[id / 32] & (1 << (id % 32)) != 0
    }
    /// Save Game restore of a persistent breakable: broken, its fade long
    /// finished. An id past this catalogue (a record from a larger one) is
    /// ignored rather than trusted.
    pub fn restore_broken(&mut self, id: usize) {
        if id < SCENES * BREAKABLES_PER_SCENE {
            self.broken[id / 32] |= 1 << (id % 32);
            self.invalidate_edges();
        }
    }
    /// Every broken breakable whose break the source would save: its full
    /// `PersistentBoolItem`. Outside the resident scene that is every set bit,
    /// because leaving a scene clears the others (`reset_scene`); inside it the
    /// admitted bank's flag tells them apart. A breakable listed by two regions
    /// is reported twice, which the store's overwrite makes harmless.
    pub fn for_each_persistent_broken(&self, scene: usize, mut found: impl FnMut(usize)) {
        for (word, &bits) in self.broken.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                let id = word * 32 + bits.trailing_zeros() as usize;
                bits &= bits - 1;
                if id / BREAKABLES_PER_SCENE != scene {
                    found(id);
                }
            }
        }
        let Some(bank) = admitted_bank() else { return };
        if bank.scene_id() as usize != scene {
            return;
        }
        // By object rather than through `Breakables`, which also counts each
        // one's trailing fades: this only needs the flag and the id.
        for region in bank.regions() {
            for object in region.objects().flatten() {
                let id = object.state_id() as usize;
                if object.kind() == KIND_BREAKABLE && object.flags() & 1 != 0 && self.broken(id) {
                    found(id);
                }
            }
        }
    }
    /// Returns false for an already broken source object, including another
    /// copy of its metadata after loading an overlapping region.
    pub fn break_object(&mut self, id: usize, fade_ticks: u16) -> bool {
        if self.broken(id) {
            return false;
        }
        self.broken[id / 32] |= 1 << (id % 32);
        let slot = local(id);
        self.fade_left[slot] = fade_ticks;
        if fade_ticks!=0 {self.fade_active[slot/32]|=1<<(slot%32);}
        self.invalidate_edges();
        true
    }
    pub fn cut(&self, id: usize) -> bool {
        let id = id % GRASS_PER_SCENE;
        self.grass[id / 32] & (1 << (id % 32)) != 0
    }
    pub fn cut_grass(&mut self, id: usize) {
        let id = id % GRASS_PER_SCENE;
        self.grass[id / 32] |= 1 << (id % 32);
    }
    /// A nail hit on an enemy: HealthManager's `slashImpactPrefab` is the
    /// same `Slash Impact R` GrassCut spawns, so the views of a scene with
    /// grass already hold its clips; elsewhere nothing is drawn. It faces
    /// away from the Knight, by the side the nail is on, as GrassCut's does.
    pub fn hit_impact(&mut self, region: &Region, source: usize, bounds: [i32; 4], player_x: i32) -> bool {
        let sign = if (bounds[0] as i64 + bounds[2] as i64) / 2 >= player_x as i64 { 1 } else { -1 };
        region.grass_impact.is_some() && self.impacts.spawn(region.scene, source, bounds, bounds, sign)
    }
    /// One call per 60 Hz simulation tick, including GPU-wait VBlanks.
    pub fn draw_impacts(&mut self, region: &Region, room: &Room, camera: (i32, i32)) -> u32 {
        let debris = self
            .debris
            .draw(region.scene, region.door_debris, room, camera);
        let (impacts, skipped) =
            self.impacts
                .draw_limited(region.scene, region.grass_impact, room, camera, 32 - debris);
        self.effect_draw_dropped = self.effect_draw_dropped.saturating_add(skipped);
        debris
            + impacts
            + particles::pool().draw(region.scene, region.particle_bank, room, camera)
    }
    #[inline(never)]
    pub fn tick_debris(&mut self, region: &Region, room: &Room) {
        particles::pool().tick(region.scene, region.particle_bank);
        self.refresh_edges(region,room);
        let count=room.counts[5];
        let broken = &self.broken;
        let geo=&self.geo_edges[..self.geo_edge_count];
        let life=&self.lifeblood_edges[..self.lifeblood_edge_count];
        // Future larger rooms retain the direct path rather than dropping terrain.
        let cached=&self.edge_cache[..self.edge_cache_count];
        let edge=|i:usize| if i<cached.len() {cached[i]} else if geo.contains(&(i as u16))||life.contains(&(i as u16))||broken_edge(broken,region,i) {[0;4]}else{room.edge(i)};
        particles::pool().tick_break(region.scene,region.collision_bounds,count,&edge);
        self.debris.tick(region.scene,region.door_debris,region.collision_bounds,count,edge);
    }
    /// The resident scene payload is immutable. Explicit world generation plus
    /// global region ID and the borrowed room view select the local edge
    /// reference list. The view address only disambiguates two immutable room
    /// slices during one generation; world_generation invalidates it whenever
    /// the reusable arena is replaced. The count is how far the table reaches
    /// into this room, not an identity: a room past `EDGE_CACHE` matches at the
    /// cap and answers the rest uncached.
    #[inline(always)]
    fn edge_cache_matches(&self,region:&Region,room:&Room)->bool {
        self.edge_cache_valid
            && self.edge_cache_count==room.counts[5].min(EDGE_CACHE)
            && self.edge_cache_generation==self.world_generation
            && self.edge_cache_region_id==region.global_id
            && self.edge_cache_room_key==room as *const Room as usize
    }
    /// Retire every view before the reusable world/geometry arena can be
    /// overwritten. Call once for every metadata admission, including a retry
    /// of the same scene at the same address.
    pub fn begin_world_admission(&mut self) {
        self.world_generation=self.world_generation.wrapping_add(1);
        self.invalidate_edges();
    }
    fn invalidate_edges(&mut self) {
        self.edge_cache_count=0;
        self.edge_cache_valid=false;
        self.edge_epoch=self.edge_epoch.wrapping_add(1);
    }
    /// Rebuild only after an exclusion mutation or source change. Returns true
    /// when the table was rebuilt; a room past `EDGE_CACHE` caches its first
    /// `EDGE_CACHE` edges and leaves the tail to `edge_uncached`.
    pub fn refresh_edges(&mut self,region:&Region,room:&Room)->bool {
        if self.edge_cache_matches(region,room){return false;}
        let count=room.counts[5].min(EDGE_CACHE);
        let Self{edge_cache,edge_cache_count,geo_edges,geo_edge_count,lifeblood_edges,lifeblood_edge_count,broken,..}=self;
        fill_edges(&geo_edges[..*geo_edge_count],&lifeblood_edges[..*lifeblood_edge_count],broken,region,room,&mut edge_cache[..count]);
        *edge_cache_count=count;
        self.edge_cache_generation=self.world_generation;
        self.edge_cache_region_id=region.global_id;
        self.edge_cache_room_key=room as *const Room as usize;
        self.edge_cache_valid=true;
        true
    }
    pub fn tick(&mut self) {
        self.impacts.tick();
        self.secrets.tick();
        // Only broken source objects with an unfinished fade need a timer
        // update; retain scene-global countdowns and exact simultaneous expiry.
        for (word,active) in self.fade_active.iter_mut().enumerate() {
            let mut bits=*active;
            while bits!=0 {
                let bit=bits.trailing_zeros()as usize;bits&=bits-1;
                let time=&mut self.fade_left[word*32+bit];
                *time=time.saturating_sub(1);
                if *time==0 {*active&=!(1<<bit);}
            }
        }
    }
    pub fn enemy_death_particles(&mut self, region: &Region, source: u32, position: [i32; 3]) {
        if let Some(bank) = region.particle_bank {
            particles::pool().spawn_death(region.scene, source, position, bank);
        }
    }
    #[cfg_attr(not(test),optimize(size))]
    pub fn reset_scene(&mut self, scene: usize) {
        self.begin_world_admission();
        self.impacts.clear_scene(scene);
        self.debris.clear_scene(scene);
        particles::pool().clear_scene(scene);
        assert!(scene < SCENES);
        self.grass.fill(0);
        self.secrets.reset();
        // The scene being reset is the admitted one: both callers pass the
        // current region's scene before the next admission.
        let bank = admitted_bank().expect("world metadata admitted for scene reset");
        assert!(bank.scene_id() as usize == scene, "scene reset needs that scene's bank");
        for object in bank.regions().flat_map(breakables) {
            if !object.persistent() {
                let id = object.id();
                self.broken[id / 32] &= !(1 << (id % 32));
                let slot = local(id);
                self.fade_left[slot] = 0;
                self.fade_active[slot/32]&=!(1<<(slot%32));
            }
        }
    }
    /// Source nail active windows and exact transformed source target polygons.
    /// Breakable.Hit breaks on the first hit; health counters are not invented.
    /// A hidden wall or cracked floor counts its hits instead (secret_breaks):
    /// `swing` names the nail swing, which lands once per target, and `body`
    /// is the Knight's box for a floor's `Hero Range`.
    pub fn strike(
        &mut self,
        region: &Region,
        nail: &Nail,
        params: AttackParams,
        polygons: [&[[i32; 2]]; 4],
        player: &Player,
        swing: u32,
        body: [i32; 4],
    ) -> Strike {
        let mut result = Strike::default();
        if !nail.hitting(params) {
            return result;
        }
        let Some(points) = polygons.get(nail.kind as usize) else {
            return result;
        };
        if !(3..=16).contains(&points.len()) {
            return result;
        }
        let mut transformed = [[0; 2]; 16];
        let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
        for (dst, src) in transformed.iter_mut().zip(*points) {
            *dst = [player.x - src[0] * player.facing, player.y + src[1]];
            bounds[0] = bounds[0].min(dst[0]);
            bounds[1] = bounds[1].min(dst[1]);
            bounds[2] = bounds[2].max(dst[0]);
            bounds[3] = bounds[3].max(dst[1]);
        }
        let polygon = &transformed[..points.len()];
        for object in region_breakables(region) {
            let id = object.id();
            if object.object.flags() & 8 != 0 {
                if let Some(e) = self.strike_secret(object, bounds, polygon, swing, body) {
                    result.broken += u16::from(e.broke);
                    result.secret = Some(e);
                }
                continue;
            }
            if !self.broken(id)
                && overlap(bounds, object.bounds())
                && object.hit_by(polygon)
                && self.break_object(id, object.fade_ticks())
            {
                particles::pool().spawn_break(region.scene,id,nail.kind as u8,player.facing);
                self.debris.spawn(
                    region.scene,
                    region.door_debris,
                    id,
                    nail.kind as u8,
                    player.facing,
                );
                result.broken += 1;
                result.door_sounds += u16::from(object.door_sound());
            }
        }
        for patch in region_grass(region) {
            if !self.cut(patch.state)
                && overlap(bounds, patch.grass.bounds)
                && hk_sim::polygon_hits_box(polygon, patch.grass.bounds)
            {
                self.cut_grass(patch.state);
                result.grass_cut += 1;
                if let Some(bank) = region.particle_bank {
                    if let Some(&emitter) = region
                        .grass_emitters
                        .iter()
                        .find(|e| e.state as usize == patch.state % GRASS_PER_SCENE)
                    {
                        particles::pool().spawn_grass(region.scene, emitter, bank);
                    }
                }
                if region.grass_impact.is_some() {
                    let sign = if (patch.grass.bounds[0] as i64 + patch.grass.bounds[2] as i64) / 2
                        >= player.x as i64
                    {
                        1
                    } else {
                        -1
                    };
                    if !self.impacts.spawn(
                        region.scene,
                        patch.state,
                        patch.grass.bounds,
                        bounds,
                        sign,
                    ) {
                        result.impact_dropped += 1;
                    }
                }
            }
        }
        result
    }
    /// A secret's particle bursts: its per-hit set, or its break. The effect
    /// cook keys a secret's emitters by owner `id | stage << 13` (stage 0 the
    /// break, 1 and 2 a floor's hits, 3 a wall's every hit).
    pub fn secret_particles(&mut self, region: &Region, e: &SecretEvent, facing: i32) {
        let stage = if e.broke {
            0
        } else if matches!(e.family, crate::secret_breaks::FAMILY_WALL | crate::secret_breaks::FAMILY_WALL_TK2D) {
            3
        } else {
            e.taken.min(2) as usize
        };
        particles::pool().spawn_break(region.scene, e.id | stage << 13, 2, facing);
    }
    /// One nail swing against one secret whose object carries flag 8.
    #[inline(never)]
    fn strike_secret(&mut self, object: Breakable, bounds: [i32; 4], polygon: &[[i32; 2]], swing: u32, body: [i32; 4]) -> Option<SecretEvent> {
        let spec = object.secret()?;
        let id = object.id();
        if self.broken(id) || !overlap(bounds, object.bounds()) || !object.hit_by(polygon) {
            return None;
        }
        let (taken, broke) = match self.secrets.nail(id, &spec, swing, body) {
            crate::secret_breaks::Outcome::Broken => {
                if !self.break_object(id, 0) {
                    return None;
                }
                (spec.hits(), true)
            }
            crate::secret_breaks::Outcome::Hit(taken) => (taken, false),
            crate::secret_breaks::Outcome::Refused => return None,
        };
        Some(SecretEvent { id, family: spec.family(), taken, broke, origin: spec.origin() })
    }
    /// Vengeful Spirit's ball against the region's secrets: `Check If Nail`
    /// sends attackType 2 to `Spell Destroy`, which breaks a hidden wall at
    /// once. A cracked floor refuses spells.
    #[inline(never)]
    pub fn spell_strike(&mut self, region: &Region, ball: [i32; 4]) -> Option<SecretEvent> {
        let square = [[ball[0], ball[1]], [ball[2], ball[1]], [ball[2], ball[3]], [ball[0], ball[3]]];
        for object in region_breakables(region) {
            let Some(spec) = object.secret() else { continue };
            let id = object.id();
            if !spec.spell() || self.broken(id) || !overlap(ball, object.bounds()) {
                continue;
            }
            if object.hit_by(&square) && self.break_object(id, 0) {
                unsafe { crate::secret_breaks::HK_SECRET_BREAKS = crate::secret_breaks::HK_SECRET_BREAKS.wrapping_add(1) };
                return Some(SecretEvent { id, family: spec.family(), taken: spec.hits(), broke: true, origin: spec.origin() });
            }
        }
        None
    }
    /// Draw the secrets of the drawn view whose art is off its cooked place (a
    /// wall recoiling, a floor sagging); `apply` hid their cooked draws. Front
    /// draws go with the front pass so each stays in its own layer.
    #[inline(never)]
    pub fn draw_secrets(&self, view: &Region, camera: (i32, i32), front: bool) -> u32 {
        let mut n = 0;
        let Some(bank) = bank_region(view) else { return 0 };
        for index in 0..bank.object_count() {
            let Some(object) = bank.object(index) else { continue };
            if object.kind() != KIND_BREAKABLE || object.flags() & 8 == 0 {
                continue;
            }
            let Some(spec) = crate::secret_breaks::Spec::after(bank, index) else { continue };
            let id = object.state_id() as usize;
            if self.broken(id) || !self.secrets.displaced(id, &spec) {
                continue;
            }
            let recoil = self.secrets.recoil(id, &spec);
            let stage = self.secrets.taken(id);
            for (part, draw) in spec.moving() {
                if crate::render::draw_is_front(draw) != front {
                    continue;
                }
                n += match recoil {
                    Some(offset) => crate::render::draw_scenery_offset(draw, offset, camera),
                    None => spec.quad(stage, part).map_or(0, |quad| crate::render::draw_scenery_quad(draw, quad, camera)),
                };
            }
        }
        n
    }
    /// One pass over the region's bank objects: grass and breakable
    /// visibility, then every mask fade (own and remote). Runs every frame.
    #[inline(never)]
    pub fn apply(&self, region: &Region) {
        crate::render::reset_visibility();
        let Some(bank) = bank_region(region) else { return };
        let mut index = 0;
        let mut owner = (0usize, 0u16);
        while let Some(object) = bank.object(index) {
            index += 1;
            match object.kind() {
                KIND_GRASS => {
                    let cut = self.cut(object.state_id() as usize);
                    crate::render::set_visible(object.extra(0) as usize, !cut);
                    crate::render::set_visible(object.extra(1) as usize, cut);
                }
                KIND_BREAKABLE => {
                    let id = object.state_id() as usize;
                    let broken = self.broken(id);
                    for draw in object.indices(0) {
                        crate::render::set_visible(draw as usize, !broken);
                    }
                    for draw in object.indices(1) {
                        crate::render::set_visible(draw as usize, broken);
                    }
                    owner = (id, object.flags() >> 6);
                    // A secret whose art is displaced draws it itself.
                    if !broken {
                        if let Some(spec) = crate::secret_breaks::Spec::after(bank, index - 1) {
                            if self.secrets.displaced(id, &spec) {
                                for (_, draw) in spec.moving() {
                                    crate::render::set_visible(draw, false);
                                }
                            }
                        }
                    }
                }
                KIND_MASK_FADE => self.apply_mask(owner.0, owner.1, MaskFade { object }),
                KIND_REMOTE_MASK => {
                    self.apply_mask(object.state_id() as usize, object.extra(2) as u16, MaskFade { object })
                }
                _ => {}
            }
        }
    }
    fn apply_mask(&self, owner: usize, total_ticks: u16, fade: MaskFade) {
        let elapsed = total_ticks.saturating_sub(self.fade_left[local(owner)]);
        let ticks = fade.ticks();
        let gain = if !self.broken(owner) {
            128
        } else if ticks == 0 || elapsed >= ticks {
            0
        } else {
            ((ticks - elapsed) as u32 * 128 / ticks as u32) as u8
        };
        for draw in fade.draws() {
            // Gain scales the vertex colour, which leaves a black mask black
            // until it drops out at zero; a binary black mask fades through the
            // subtractive CLUT the reveal controllers use (Tutorial_01's
            // `Tut_msk_03` and `Secret Mask Top`, which the doors uncover).
            if crate::render::black_mask(draw as usize) {
                crate::render::set_opacity(draw as usize, gain);
            } else {
                crate::render::set_gain(draw as usize, gain);
            }
        }
    }

    /// Only the exact source colliders owned by a broken object disappear.
    pub fn set_geo_edges(&mut self,edges:&[u16]) {
        assert!(edges.len()<=self.geo_edges.len());
        if self.geo_edge_count==edges.len()&&self.geo_edges[..edges.len()].iter().zip(edges).all(|(a,b)|a==b){return;}
        self.geo_edges[..edges.len()].copy_from_slice(edges);self.geo_edge_count=edges.len();self.invalidate_edges();
    }
    /// Refresh local cocoon collider bindings on every region activation.
    pub fn set_lifeblood_edges(&mut self,edges:&[u16]) {
        assert!(edges.len()<=self.lifeblood_edges.len());
        if self.lifeblood_edge_count==edges.len()&&self.lifeblood_edges[..edges.len()].iter().zip(edges).all(|(a,b)|a==b){return;}
        self.lifeblood_edges[..edges.len()].copy_from_slice(edges);self.lifeblood_edge_count=edges.len();self.invalidate_edges();
    }
    /// Append bounded scripted exclusions after the Lifeblood refresh. The two
    /// controllers share this eight-entry scratch; no second terrain cache.
    pub fn append_script_edges(&mut self,edges:&[u16]) {
        for &edge in edges {
            if self.lifeblood_edges[..self.lifeblood_edge_count].contains(&edge) {continue;}
            assert!(self.lifeblood_edge_count<self.lifeblood_edges.len());
            self.lifeblood_edges[self.lifeblood_edge_count]=edge;
            self.lifeblood_edge_count+=1;
            self.invalidate_edges();
        }
    }
    /// Every index below `cache.len()` with the same exclusions as edge(),
    /// built in one pass over the short exclusion lists instead of scanning
    /// them once per edge.
    pub fn fill_edges(&self,region:&Region,room:&Room,cache:&mut [[i32;4]]) {
        fill_edges(&self.geo_edges[..self.geo_edge_count],&self.lifeblood_edges[..self.lifeblood_edge_count],&self.broken,region,room,cache);
    }
    /// Validate the source once for a whole physics query. The returned reader
    /// immutably borrows state/room, so exclusions cannot change while it lives.
    /// Indices past the table and invalidated caches retain exact uncached filtering.
    #[inline(always)]
    pub fn edge_reader<'a>(&'a self,region:&'a Region,room:&'a Room<'a>)
        ->impl Fn(usize)->[i32;4]+'a {
        let cached=if self.edge_cache_matches(region,room) {
            &self.edge_cache[..self.edge_cache_count]
        } else {&[]};
        move |index| {
            if index<cached.len(){cached[index]}
            else{self.edge_uncached(region,room,index)}
        }
    }
    // The cached read must stay in the caller: returning this four-word value
    // through an outlined MIPS call costs more than decoding the original edge.
    #[inline(always)]
    pub fn edge(&self, region: &Region, room: &Room, index: usize) -> [i32; 4] {
        if index<self.edge_cache_count&&self.edge_cache_matches(region,room) {return self.edge_cache[index];}
        self.edge_uncached(region,room,index)
    }
    /// Materialize a resident actor's terrain once per physics tick. The edge
    /// order and zero sentinels match edge_in_view(), but short exclusion lists
    /// are traversed once rather than once per solver edge query.
    pub fn fill_edges_in_view(&self, active:&Region, active_room:&Room,
        target:&Region, target_room:&Room, cache:&mut [[i32;4]]) {
        assert!(cache.len()<=target_room.counts[5]);
        if active.global_id==target.global_id {
            // The table stops at EDGE_CACHE, so a longer request refills rather
            // than reading past what was cached for this room.
            if self.edge_cache_matches(active,active_room) && cache.len()<=self.edge_cache_count {
                cache.copy_from_slice(&self.edge_cache[..cache.len()]);
            } else {self.fill_edges(active,active_room,cache);}
            return;
        }
        for (i,edge) in cache.iter_mut().enumerate() {*edge=target_room.edge(i);}
        for b in region_breakables(target).filter(|b|self.broken(b.id())) {
            for i in b.edges() {if let Some(edge)=cache.get_mut(i as usize) {*edge=[0;4];}}
        }
        if active.scene==target.scene {
            for &i in self.geo_edges[..self.geo_edge_count].iter()
                .chain(&self.lifeblood_edges[..self.lifeblood_edge_count]) {
                if (i as usize)>=active_room.counts[5] {continue;}
                let excluded=active_room.edge(i as usize);
                let reverse=[excluded[2],excluded[3],excluded[0],excluded[1]];
                for edge in cache.iter_mut() {
                    if hk_sim::same_edge(edge,&excluded) || hk_sim::same_edge(edge,&reverse) {*edge=[0;4];}
                }
            }
        }
    }
    /// Read another resident collision view without applying this view's local
    /// exclusion indices to unrelated edges. Breakables have stable scene IDs;
    /// transient Geo/cocoon/script exclusions are remapped by source segment.
    pub fn edge_in_view(&self, active:&Region, active_room:&Room,
        target:&Region, target_room:&Room, index:usize)->[i32;4] {
        if active.global_id==target.global_id {
            return self.edge(active,active_room,index);
        }
        let edge=target_room.edge(index);
        if broken_edge(&self.broken,target,index) {
            return [0;4];
        }
        if active.scene==target.scene {
            for &i in self.geo_edges[..self.geo_edge_count].iter()
                .chain(&self.lifeblood_edges[..self.lifeblood_edge_count]) {
                if (i as usize)>=active_room.counts[5] {continue;}
                let excluded=active_room.edge(i as usize);
                if hk_sim::same_edge(&edge,&excluded) || hk_sim::same_edge(&edge,&[excluded[2],excluded[3],excluded[0],excluded[1]]) {
                    return [0;4];
                }
            }
        }
        edge
    }
    #[inline(never)]
    fn edge_uncached(&self, region:&Region,room:&Room,index:usize)->[i32;4] {
        if self.geo_edges[..self.geo_edge_count].contains(&(index as u16)) {return [0;4];}
        if self.lifeblood_edges[..self.lifeblood_edge_count].contains(&(index as u16)) {return [0;4];}
        if broken_edge(&self.broken,region,index) {return [0;4];}
        room.edge(index)
    }
    /// List the region's hazards, then its checkpoints, once per region and
    /// world admission. The bank is immutable between admissions.
    #[inline(never)]
    fn bind_triggers(&mut self, region: meta::Region) {
        let id = region.global_id();
        let t = &mut self.triggers;
        if t.region == id && t.generation == self.world_generation {
            return;
        }
        t.region = id;
        t.generation = self.world_generation;
        t.count = 0;
        t.overflow = false;
        for kind in [META_HAZARD, META_CHECKPOINT] {
            for local in 0..region.object_count() {
                let Some(object) = region.object(local) else { continue };
                if object.kind() == kind {
                    if t.count as usize == TRIGGER_SLOTS || local > u8::MAX as usize {
                        t.overflow = true;
                        return;
                    }
                    t.object[t.count as usize] = local as u8;
                    t.bounds[t.count as usize] = object.bounds();
                    t.count += 1;
                }
            }
            if kind == META_HAZARD {
                t.hazards = t.count;
            }
        }
    }
    /// `bank_trigger` through the bound table: the first object of `kind` in
    /// cooked order whose bounds and polygons reach the body.
    /// `skip` passes over objects by source id (a hazard whose damage another
    /// module runs, props::World::owns_hazard).
    fn trigger<'a>(&mut self, region: meta::Region<'a>, kind: u16, body: [i32; 4], skip: &dyn Fn(u32) -> bool) -> Option<meta::Object<'a>> {
        self.bind_triggers(region);
        let t = &self.triggers;
        if t.overflow {
            return bank_trigger(region, kind, body, skip);
        }
        let (first, end) = if kind == META_HAZARD { (0, t.hazards) } else { (t.hazards, t.count) };
        for slot in first as usize..end as usize {
            if !overlap(body, t.bounds[slot]) {
                continue;
            }
            if let Some(object) = region.object(t.object[slot] as usize) {
                if !skip(object.source_id()) && object.polygons().flatten().any(|p| bank_polygon_hits(p, body)) {
                    return Some(object);
                }
            }
        }
        None
    }
    /// The first of the scene's gates that takes the Knight this tick (see
    /// `gate_takes`), from a list decoded once per scene and world
    /// admission. `gates` lists nothing while another scene's bank is
    /// admitted, so nothing is kept then.
    #[inline(never)]
    pub fn gate(&mut self, scene: usize, player: &Player, body: [i32; 4], up: bool, recoiling: bool,
                scene_ticks: u32) -> Option<Gate> {
        if !admitted_bank().is_some_and(|bank| bank.scene_id() as usize == scene) {
            return None;
        }
        if self.gates.scene != scene + 1 || self.gates.generation != self.world_generation {
            self.bind_gates(scene);
        }
        let g = &self.gates;
        if g.overflow {
            return gate(scene, player, body, up, recoiling, scene_ticks);
        }
        g.list[..g.count as usize].iter().find(|g| gate_takes(g, scene, player, body, up, recoiling, scene_ticks)).copied()
    }
    #[inline(never)]
    fn bind_gates(&mut self, scene: usize) {
        let g = &mut self.gates;
        g.scene = scene + 1;
        g.generation = self.world_generation;
        g.count = 0;
        g.overflow = false;
        // `gates(scene)`, walked by index: the caller checked the bank is this scene's.
        let Some(region) = admitted_bank().and_then(|bank| bank.region(0)) else { return };
        for local in 0..region.object_count() {
            let Some(o) = region.object(local) else { continue };
            if o.kind() != KIND_GATE {
                continue;
            }
            if g.count as usize == GATE_SLOTS {
                g.overflow = true;
                return;
            }
            g.list[g.count as usize] = gate_of(&o, scene);
            g.count += 1;
        }
    }
    /// The gate the Knight is most likely to take next, as (list index, gate):
    /// the nearest trigger box, holding back gates behind him (against his
    /// facing) by BEHIND so a fresh entry does not pick the gate it came in
    /// through. `keep` is the previous answer, kept unless another gate is
    /// clearly nearer, so a Knight between two gates does not flip the
    /// prefetch back and forth. None when the scene lists no gates.
    /// `entry` is the gate the Knight came in by, excluded until he has been
    /// `release` units away from it: standing in it just after the entry says
    /// nothing about going back. `Some(usize::MAX)` asks this call to find it.
    pub fn predict_gate(&mut self, scene: usize, player: &Player, keep: Option<usize>,
                        entry: &mut Option<usize>, release: i32) -> Option<(usize, Gate, i32)> {
        const BEHIND: i32 = 24; // units
        const HOLD: i32 = 8;
        if !admitted_bank().is_some_and(|bank| bank.scene_id() as usize == scene) {
            return None;
        }
        if self.gates.scene != scene + 1 || self.gates.generation != self.world_generation {
            self.bind_gates(scene);
        }
        let g = &self.gates;
        let score = |gate: &Gate| {
            let b = gate.bounds;
            let dx = (b[0] - player.x).max(player.x - b[2]).max(0) >> 16;
            let dy = (b[1] - player.y).max(player.y - b[3]).max(0) >> 16;
            let centre = (b[0] >> 1) + (b[2] >> 1);
            dx + dy + if dx > 0 && (centre - player.x).signum() == -player.facing.signum() { BEHIND } else { 0 }
        };
        let list = &g.list[..g.count as usize];
        let distance = |gate: &Gate| {
            let b = gate.bounds;
            ((b[0] - player.x).max(player.x - b[2]).max(0) >> 16) + ((b[1] - player.y).max(player.y - b[3]).max(0) >> 16)
        };
        if *entry == Some(usize::MAX) {
            *entry = list.iter().enumerate().min_by_key(|(_, gate)| distance(gate)).map(|(i, _)| i);
        }
        if let Some(e) = *entry {
            if e >= list.len() || distance(&list[e]) > release { *entry = None; }
        }
        let skip = *entry;
        let (best, gate) = list.iter().enumerate().filter(|(i, _)| Some(*i) != skip).min_by_key(|(_, gate)| score(gate))?;
        let keep = keep.filter(|&k| Some(k) != skip);
        match keep {
            Some(k) if k < list.len() && score(&list[k]) <= score(gate) + HOLD => Some((k, list[k], score(&list[k]))),
            _ => Some((best, *gate, score(gate))),
        }
    }
    /// Exact authored static DamageHero polygons against the Knight body box,
    /// read from the admitted scene bank. Direction points away from the source
    /// object's transform, for normal recoil.
    /// `skip` names hazards, by source id, whose damage is run elsewhere.
    pub fn hazard_contact(&mut self, region: meta::Region, player: &Player, params: Params, skip: impl Fn(u32) -> bool) -> Option<(u16, bool, i32)> {
        hazard_of(self.trigger(region, META_HAZARD, body_box(player, params), &skip)?, player)
    }
    /// Authored hazard checkpoint trigger, distinct from a bench or death save:
    /// spawn point and facing from the admitted scene bank.
    pub fn checkpoint(&mut self, region: meta::Region, player: &Player, params: Params) -> Option<([i32; 2], i32)> {
        Some(checkpoint_of(self.trigger(region, META_CHECKPOINT, body_box(player, params), &|_| false)?))
    }
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
// World coordinates fit +/-512 and nail-local coordinates +/-16 Q16 units;
// i64 cross products remain below 2^54. Supports concave source polygons.
fn cross(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> i64 {
    (b[0] as i64 - a[0] as i64) * (p[1] as i64 - a[1] as i64)
        - (b[1] as i64 - a[1] as i64) * (p[0] as i64 - a[0] as i64)
}
fn on_segment(a: [i32; 2], b: [i32; 2], p: [i32; 2]) -> bool {
    cross(a, b, p) == 0
        && (a[0].min(b[0])..=a[0].max(b[0])).contains(&p[0])
        && (a[1].min(b[1])..=a[1].max(b[1])).contains(&p[1])
}
fn inside(poly: &[[i32; 2]], point: [i32; 2]) -> bool {
    let mut inside = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if on_segment(a, b, point) {
            return true;
        }
        if (a[1] > point[1]) != (b[1] > point[1]) {
            let side = cross(a, b, point);
            if (b[1] > a[1] && side > 0) || (b[1] < a[1] && side < 0) {
                inside = !inside;
            }
        }
    }
    inside
}
pub(crate) fn polygons_overlap(a: &[[i32; 2]], b: &[[i32; 2]]) -> bool {
    if !(3..=16).contains(&a.len()) || !(3..=16).contains(&b.len()) {
        return false;
    }
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        for j in 0..b.len() {
            let (r, s) = (b[j], b[(j + 1) % b.len()]);
            let (pr, ps, rp, rq) = (
                cross(p, q, r),
                cross(p, q, s),
                cross(r, s, p),
                cross(r, s, q),
            );
            if ((pr > 0 && ps < 0 || pr < 0 && ps > 0) && (rp > 0 && rq < 0 || rp < 0 && rq > 0))
                || on_segment(p, q, r)
                || on_segment(p, q, s)
                || on_segment(r, s, p)
                || on_segment(r, s, q)
            {
                return true;
            }
        }
    }
    inside(a, b[0]) || inside(b, a[0])
}
/// Kind tags of the HKWMTA01 object records this module consumes.
pub const META_HAZARD: u16 = 3;
pub const META_CHECKPOINT: u16 = 4;
/// Copy one bank polygon into a bounded stack buffer for the shared polygon
/// test. Admission already bounded every polygon to 3..16 points.
fn bank_polygon_hits(polygon: hk_format::world_meta::Polygon, body: [i32; 4]) -> bool {
    let mut points = [[0i32; 2]; 16];
    let mut count = 0;
    for point in polygon.points().take(16) {
        let Ok(point) = point else { return false };
        points[count] = point;
        count += 1;
    }
    hk_sim::polygon_hits_box(&points[..count], body)
}
/// The whole-region scan: `State::trigger`'s answer for a region too big for
/// its table, kept out of line so the table path stays small.
#[inline(never)]
fn bank_trigger<'a>(
    region: hk_format::world_meta::Region<'a>,
    kind: u16,
    body: [i32; 4],
    skip: &dyn Fn(u32) -> bool,
) -> Option<hk_format::world_meta::Object<'a>> {
    region.objects().flatten().find(|object| {
        object.kind() == kind
            && !skip(object.source_id())
            && overlap(body, object.bounds())
            && object.polygons().flatten().any(|p| bank_polygon_hits(p, body))
    })
}
fn body_box(player: &Player, params: Params) -> [i32; 4] {
    [
        player.x - params.half_width,
        player.y + params.bottom,
        player.x + params.half_width,
        player.y + params.top,
    ]
}
fn hazard_of(hazard: meta::Object, player: &Player) -> Option<(u16, bool, i32)> {
    let word = hazard.extra(1);
    let damage = (word & 0xFFFF) as u16;
    if damage == 0 {
        return None;
    }
    let direction = if player.x < hazard.extra(0) { -1 } else { 1 };
    Some((damage, word >> 16 != 0, direction))
}
fn checkpoint_of(checkpoint: meta::Object) -> ([i32; 2], i32) {
    ([checkpoint.extra(0), checkpoint.extra(1)], checkpoint.extra(2))
}
/// `State::hazard_contact` without the table: the whole-region scan the
/// tests hold the cached answer to.
#[cfg(test)]
pub fn hazard_contact(region: meta::Region, player: &Player, params: Params) -> Option<(u16, bool, i32)> {
    hazard_of(bank_trigger(region, META_HAZARD, body_box(player, params), &|_| false)?, player)
}
/// `State::checkpoint` without the table, for the tests.
#[cfg(test)]
pub fn checkpoint(region: meta::Region, player: &Player, params: Params) -> Option<([i32; 2], i32)> {
    Some(checkpoint_of(bank_trigger(region, META_CHECKPOINT, body_box(player, params), &|_| false)?))
}

pub fn contains(bounds: [i32; 4], x: i32, y: i32) -> bool {
    x >= bounds[0] && x <= bounds[2] && y >= bounds[1] && y <= bounds[3]
}
/// Two world boxes touching, which is what a source trigger asks of the hero
/// body rather than of the single point `contains` takes.
pub fn intersects(bounds: [i32; 4], body: [i32; 4]) -> bool {
    bounds[0] <= body[2] && bounds[2] >= body[0] && bounds[1] <= body[3] && bounds[3] >= body[1]
}
/// Predict only the current vertical leg. The same swept terrain solver used
/// by gameplay stops the look-ahead at floors, ceilings and the jump apex.
/// No horizontal movement is assumed; the caller retains its cheap fallback.
pub(crate) fn vertical_exit(
    bounds: [i32; 4],
    player: &Player,
    params: Params,
    edge_count: usize,
    edge: impl Fn(usize) -> [i32; 4],
) -> Option<i32> {
    let direction = player.vy.signum();
    if direction == 0 {
        return None;
    }
    let mut predicted = *player;
    let jump = predicted.was_jump;
    for _ in 0..30 {
        predicted.step(params, 0, jump, edge_count, &edge);
        if direction > 0 && predicted.y > bounds[3] {
            return Some(bounds[3] + ONE / 16);
        }
        if direction < 0 && predicted.y < bounds[1] {
            return Some(bounds[1] - ONE / 16);
        }
        if predicted.grounded || predicted.vy.signum() != direction {
            break;
        }
    }
    None
}
/// Next region the Knight is about to enter, from the admitted bank's
/// neighbour lists (global chunk ids). Neighbours in another scene's bank
/// are not resident and are left to the scene gates.
pub fn upcoming(
    index: usize,
    player: &Player,
    room: &Room,
    state: &State,
    params: Params,
) -> Option<usize> {
    let region = resident(index)?;
    let bounds = region.bounds;
    let bank = bank_region(&region)?;
    let neighbour = |x, y| {
        bank.neighbours().flatten().find_map(|n| {
            let found = admitted_bank()?.region_by_global_id(n)?;
            contains(found.bounds(), x, y).then_some(n as usize - 1)
        })
    };
    let vertical_candidate = (player.vy > ONE && bounds[3] - player.y < 6 * ONE)
        || (player.vy < -ONE && player.y - bounds[1] < 4 * ONE);
    if vertical_candidate {
        if let Some(y) = vertical_exit(bounds, player, params, room.counts[5], |i| {
            state.edge(&region, room, i)
        }) {
            if let Some(next) = neighbour(player.x, y) {
                return Some(next);
            }
        }
    }
    let x = if player.facing > 0 {
        bounds[2] + ONE / 16
    } else {
        bounds[0] - ONE / 16
    };
    neighbour(x, player.y)
}

/// TransitionPoint.TryDoTransition: the Knight's origin inside the trigger,
/// never while recoiling (the source zeroes the velocity and pushes the body
/// back out of the trigger instead), a side gate only while facing into it,
/// and a delayed collider only once `scene_ticks` has passed its delay.
/// `Gate::side` for a TransitionPoint that carries a `Door Control` FSM.
pub const DOOR: u8 = 5;
/// The scene's reveal-mask controllers, in binding order, from the first
/// region of the admitted bank. Empty while another scene's bank is admitted.
pub fn reveal_masks(scene: usize) -> impl Iterator<Item = crate::reveal_masks::RevealMask> {
    reveal_objects(scene).map(|o| crate::reveal_masks::RevealMask {
        source_id: o.source_id(),
        bounds: o.bounds(),
        fade_ticks: o.extra(0) as u16,
        initial_opacity: if o.flags() & 2 != 0 { 128 } else { 0 },
        one_way: o.flags() & 1 != 0,
        flags: o.flags(),
        driver: o.extra(1),
        slot: if o.flags() & 32 != 0 { o.extra(2) } else { -1 },
    })
}
/// The authored Idle opacity of one controller, for the frames that render
/// before a scene is bound. Zero when the bank does not cover it.
pub fn reveal_initial_opacity(scene: usize, controller: usize) -> u8 {
    reveal_objects(scene).find(|o| o.state_id() as usize == controller)
        .map_or(0, |o| if o.flags() & 2 != 0 { 128 } else { 0 })
}
/// The exact trigger test the reveal tick falls through to once its own AABB
/// has admitted the hero box. Each polygon is copied into a bounded local
/// because the bank hands out points rather than a slice; admission already
/// held every reveal polygon to three through sixteen of them. An owner with
/// several trigger boxes answers any of them, as Trigger2dEvent does.
pub fn reveal_trigger_reaches(scene: usize, controller: usize, body: [i32; 4]) -> bool {
    let Some(object) = reveal_objects(scene).find(|o| o.state_id() as usize == controller) else {
        return false;
    };
    object.polygons().flatten().any(|trigger| {
        let mut points = [[0i32; 2]; 16];
        let mut count = 0;
        for point in trigger.points().flatten() {
            if count == points.len() {
                return false;
            }
            points[count] = point;
            count += 1;
        }
        count >= 3 && hk_sim::polygon_hits_box(&points[..count], body)
    })
}
fn reveal_objects(scene: usize) -> impl Iterator<Item = meta::Object<'static>> {
    admitted_bank()
        .filter(move |bank| bank.scene_id() as usize == scene)
        .into_iter()
        .flat_map(|bank| {
            bank.region(0).into_iter().flat_map(|region| {
                region.objects().flatten().filter(|o| o.kind() == KIND_REVEAL_MASK)
            })
        })
}
/// The Geo an enemy of this scene drops, by its source id. The fling profile
/// is not carried: `mega` selects one of the guest's own `geo::FLING` pair,
/// which is the whole reason this costs one bank object instead of six more
/// constant Q16 words per enemy.
#[derive(Clone, Copy)]
pub struct GeoEnemy {
    pub source_id: u32,
    pub drops: [u16; 3],
    pub offset: [i32; 2],
    pub mega: bool,
}
/// The payout the admitted bank records for one enemy, or None when this scene
/// has no Geo for that source. Reads the first region, where the cooker puts
/// every scene-level object.
pub fn geo_enemy(scene: usize, source: u32) -> Option<GeoEnemy> {
    let bank = admitted_bank().filter(|bank| bank.scene_id() as usize == scene)?;
    let object = bank.region(0)?.objects().flatten()
        .find(|o| o.kind() == KIND_GEO_ENEMY && o.source_id() == source)?;
    let drops = object.state_id();
    Some(GeoEnemy {
        source_id: source,
        drops: [drops as u16, (drops >> 16) as u16, object.extra(2) as u16],
        offset: [object.extra(0), object.extra(1)],
        mega: object.flags() & 1 != 0,
    })
}
/// Every admitted gate of a scene, read from the first region of that scene's
/// metadata bank. Empty while another scene's bank is the admitted one, which
/// is the same answer the linked table gave for a scene it did not cover.
pub fn gates(scene: usize) -> impl Iterator<Item = Gate> {
    admitted_bank()
        .filter(move |bank| bank.scene_id() as usize == scene)
        .into_iter()
        .flat_map(move |bank| {
            bank.region(0).into_iter().flat_map(move |region| {
                region.objects().flatten().filter(|o| o.kind() == KIND_GATE).map(move |o| gate_of(&o, scene))
            })
        })
}
/// One `KIND_GATE` object of `scene`'s bank as a `Gate`.
fn gate_of(o: &meta::Object, scene: usize) -> Gate {
    let destination = o.state_id();
    Gate {
        scene,
        target_scene: (destination >> 16 & 0xFF) as usize,
        target_region: (destination & 0xFFFF) as usize,
        bounds: o.bounds(),
        spawn: [o.extra(0), o.extra(1)],
        entry_vy: o.extra(2),
        side: (destination >> 24 & 0xF) as u8,
        delay_ticks: o.flags(),
    }
}
/// The scan `State::gate` answers from its list, and its fallback for a
/// scene with more gates than the list holds.
#[inline(never)]
pub fn gate(scene: usize, player: &Player, body: [i32; 4], up: bool, recoiling: bool,
            scene_ticks: u32) -> Option<Gate> {
    gates(scene).find(|g| gate_takes(g, scene, player, body, up, recoiling, scene_ticks))
}
/// `TransitionPoint.TryDoTransition` for one gate (see `DOOR` above).
fn gate_takes(g: &Gate, scene: usize, player: &Player, body: [i32; 4], up: bool, recoiling: bool,
              scene_ticks: u32) -> bool {
    g.scene == scene
        && !recoiling
        && scene_ticks >= g.delay_ticks as u32
        && match g.side {
            // `Door Control` is never walked through: `In Range` is the
            // door's own trigger against the hero body and `Change Scene`
            // waits on `ListenForUp`. Both matter here, because a door
            // collider is a strip of floor about a quarter of a unit tall,
            // so the player point a side gate is tested with sits well
            // above it and would never enter. Town's door_sly and
            // Crossroads_06's door1 are the two in the catalogue.
            DOOR => up && intersects(g.bounds, body),
            1 => contains(g.bounds, player.x, player.y) && player.facing < 0,
            2 => contains(g.bounds, player.x, player.y) && player.facing > 0,
            _ => contains(g.bounds, player.x, player.y),
        }
}
/// `broken_edge`'s index walk against the `Breakables` iteration it replaced,
/// on every cooked disc bank, with every breakable broken, none, and a
/// scattering in between.
#[cfg(test)]
mod broken_edge_tests {
    use super::*;
    #[test]
    fn index_walk_matches_breakable_iteration() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.hkpsx/world-metadata-packed");
        let mut banks: std::vec::Vec<_> = std::fs::read_dir(&dir).expect("cooked disc banks").flatten()
            .map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "hkwm")).collect();
        banks.sort();
        let (mut probes, mut owned) = (0u32, 0u32);
        for path in &banks {
            let bytes: &'static [u8] = std::boxed::Box::leak(std::fs::read(path).unwrap().into_boxed_slice());
            TEST_BANK.with(|bank| bank.set(bytes));
            let meta = hk_format::WorldMeta::parse(bytes).unwrap();
            for bank_region in meta.regions() {
                let Some(region) = resident(bank_region.global_id() as usize - 1) else { continue };
                let top = breakables(bank_region).flat_map(|b| b.edges()).max().map_or(4, |e| e as usize + 2);
                for pattern in 0..3u32 {
                    let broken: std::vec::Vec<u32> = (0..SCENES * BREAKABLES_PER_SCENE / 32)
                        .map(|i| match pattern { 0 => 0, 1 => u32::MAX, _ => (i as u32).wrapping_mul(0x9e37_79b9) }).collect();
                    for index in 0..top {
                        let want = region_breakables(&region).any(|b| {
                            let id = b.id();
                            broken[id / 32] & (1 << (id % 32)) != 0 && b.edges().any(|e| e as usize == index)
                        });
                        assert_eq!(broken_edge(&broken, &region, index), want, "{} region {} edge {index}", path.display(), region.global_id);
                        probes += 1;
                        owned += u32::from(want);
                    }
                }
            }
        }
        assert!(owned > 0, "{probes} probes");
    }
}
