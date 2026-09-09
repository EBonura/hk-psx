//! Source WalkLeftRight / HealthManager subset. States survive cooked grid swaps.
//! Actors outside the loaded collision window are suspended; Unity's full scene
//! activation, full Box2D solver, corpse particles, Geo drops and global FSM events are
//! still outside this bounded runtime.
use crate::world::{Region, State};
use hk_format::{i32_at, u32_at, Room};
use hk_sim::{
    ActorHealth, ActorSpec, AttackParams, Hit, Hurt, Nail, Params, Player, Vitals, WalkState,
    MAX_ACTORS, ONE,
};

pub const MAX_VISIBLE: usize = 6;
#[derive(Clone, Copy, PartialEq, Eq)]
enum SpawnState {
    Pending,
    Ready,
    Blocked,
}
#[derive(Clone, Copy)]
struct Actor {
    scene: usize,
    source_id: u32,
    x: i32,
    y: i32,
    vy: i32,
    grounded: bool,
    spawn_state: SpawnState,
    health: ActorHealth,
    walk: WalkState,
    recoil_left: u16,
    recoil: [i32; 2],
    corpse: Option<hk_sim::Corpse>,
    geo_paid: bool,
}
impl Actor {
    fn new(scene: usize, spec: &ActorSpec) -> Self {
        // Stable per-source seed, not a claim to reproduce Unity's global RNG.
        let seed = spec
            .source_id
            .wrapping_mul(1664525)
            .wrapping_add(1013904223);
        Self {
            scene,
            source_id: spec.source_id,
            x: spec.x,
            y: spec.y,
            vy: 0,
            grounded: false,
            spawn_state: SpawnState::Pending,
            health: ActorHealth::new(spec.health),
            walk: WalkState::new(
                spec.initial_direction,
                spec.random_start_direction && seed & 0x80000000 != 0,
            ),
            recoil_left: 0,
            recoil: [0; 2],
            corpse: None,
            geo_paid: false,
        }
    }
    fn local_bounds(&self, spec: &ActorSpec) -> [i32; 4] {
        let b = spec.bounds;
        if self.walk.direction == spec.initial_direction {
            b
        } else {
            [-b[2], b[1], -b[0], b[3]]
        }
    }
    fn bounds(&self, spec: &ActorSpec) -> [i32; 4] {
        let b = self.local_bounds(spec);
        [self.x + b[0], self.y + b[1], self.x + b[2], self.y + b[3]]
    }
    fn advance(&mut self, spec: &ActorSpec, region: &Region, room: &Room, state: &State) {
        if self.spawn_state == SpawnState::Blocked {
            return;
        }
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        let mut body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        // Installed Physics2DSettings: gravity.y=-60, max translation speed=100;
        // supported Crawler Rigidbody2D: gravityScale=1, linearDamping=0.
        let mut p = Params {
            speed: 0,
            jump: 0,
            gravity: 60 * ONE,
            fall: 100 * ONE,
            hold_ticks: 0,
            min_ticks: 0,
            half_width: (b[2] - b[0]) / 2,
            bottom: b[1],
            top: b[3],
        };
        if self.spawn_state == SpawnState::Pending {
            // The source Rigidbody2D may begin slightly inside Terrain. Resolve
            // once when its collision apron is resident, never on grid swaps or
            // later jump/recoil motion. Unsupported overlapping geometry leaves
            // this actor suspended until a scene reset, without retrying per tick.
            if !hk_sim::resolve_actor_spawn(&mut body, p, room.counts[5], |i| {
                state.edge(region, room, i)
            }) {
                self.spawn_state = SpawnState::Blocked;
                return;
            }
            self.x = body.x - offset;
            self.y = body.y;
            self.spawn_state = SpawnState::Ready;
        }
        let senses = hk_sim::walker_senses(
            self.bounds(spec),
            self.walk.direction,
            room.counts[5],
            |i| state.edge(region, room, i),
        );
        let vx = self.walk.tick(spec.walk, senses.0, senses.1, senses.2);
        // A completed source turn can mirror the asymmetric collider offset.
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        p.half_width = (b[2] - b[0]) / 2;
        p.bottom = b[1];
        p.top = b[3];
        p.speed = vx.abs();
        body.step(p, vx.signum(), false, room.counts[5], |i| {
            state.edge(region, room, i)
        });
        self.vy = body.vy;
        self.grounded = body.grounded;
        if self.recoil_left != 0 {
            // Recoil.UpdatePhysics sweeps a separate transform displacement;
            // it does not replace the walking rigidbody velocity. Reuse the
            // bounded terrain solver for that displacement, preserving gravity.
            self.recoil_left -= 1;
            p.speed = self.recoil[0].abs();
            p.gravity = 0;
            body.vy = self.recoil[1];
            body.step(p, self.recoil[0].signum(), false, room.counts[5], |i| {
                state.edge(region, room, i)
            });
        }
        self.x = body.x - offset;
        self.y = body.y;
        assert!(
            self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE,
            "enemy left validated Q16 scene bounds"
        );
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Events {
    pub hits: u16,
    pub kills: u16,
    pub hurt: Hurt,
}
pub struct EnemyWorld {
    actors: [Option<Actor>; MAX_ACTORS],
}
impl EnemyWorld {
    /// Drain each source actor's death event once, even across local views.
    pub fn take_geo_deaths(&mut self,scene:usize,mut emit:impl FnMut(u32,i32,i32)->bool) {
        for actor in self.actors.iter_mut().flatten().filter(|a|a.scene==scene) {
            if actor.health.dead && !actor.geo_paid {
                actor.geo_paid=emit(actor.source_id,actor.x,actor.y);
            }
        }
    }
    pub const fn new() -> Self {
        Self {
            actors: [None; MAX_ACTORS],
        }
    }
    /// Compact read-only state for route validation/telemetry.
    pub fn actor_state(&self, scene: usize, source_id: u32) -> Option<(i32, i32, i16)> {
        self.actors
            .iter()
            .flatten()
            .find(|a| a.scene == scene && a.source_id == source_id)
            .map(|a| (a.x, a.y, a.health.hp))
    }
    pub fn reset_scene(&mut self, scene: usize) {
        for slot in &mut self.actors {
            if slot.as_ref().is_some_and(|a| a.scene == scene) {
                *slot = None;
            }
        }
    }
    pub fn sync_region(&mut self, region: &Region) {
        for spec in region.actors {
            if self
                .actors
                .iter()
                .flatten()
                .any(|a| a.scene == region.scene && a.source_id == spec.source_id)
            {
                continue;
            }
            let slot = self
                .actors
                .iter_mut()
                .find(|a| a.is_none())
                .expect("enemy source pool exceeds 32");
            *slot = Some(Actor::new(region.scene, spec));
        }
    }
    /// Call only on a running 60 Hz simulation tick, after Knight movement/nail
    /// state and before applying transitions. The caller owns hit-stop timing.
    pub fn tick(
        &mut self,
        region: &Region,
        room: &Room,
        state: &mut State,
        player: &mut Player,
        vitals: &mut Vitals,
        nail: &Nail,
        response: &mut hk_sim::NailResponse,
        attack: AttackParams,
        polygons: [&[[i32; 2]]; 4],
    ) -> Events {
        self.sync_region(region);
        let mut events = Events {
            hits: 0,
            kills: 0,
            hurt: Hurt::Ignored,
        };
        let mut points = [[0; 2]; 16];
        let polygon = if nail.hitting(attack) {
            let src = polygons[nail.kind as usize];
            assert!((3..=16).contains(&src.len()));
            for (dst, p) in points.iter_mut().zip(src) {
                *dst = [player.x - p[0] * player.facing, player.y + p[1]];
            }
            &points[..src.len()]
        } else {
            &points[..0]
        };
        let body = [
            player.x - crate::PARAMS.half_width,
            player.y + crate::PARAMS.bottom,
            player.x + crate::PARAMS.half_width,
            player.y + crate::PARAMS.top,
        ];
        for actor in self
            .actors
            .iter_mut()
            .flatten()
            .filter(|a| a.scene == region.scene)
        {
            let Some(spec) = region
                .actors
                .iter()
                .find(|s| s.source_id == actor.source_id)
            else {
                continue;
            };
            if actor.health.dead {
                if let (Some(corpse),Some(corpse_spec))=(&mut actor.corpse,spec.corpse) {
                    if crate::world::contains(region.collision_bounds,corpse.x,corpse.y) {
                        corpse.tick(corpse_spec,room.counts[5],|i|state.edge(region,room,i));
                    }
                }
                continue;
            }
            // Use the resident collision apron, not the Knight's activation
            // rectangle. A visible actor crossing a grid boundary must not
            // become intangible. Outside that apron movement pauses, but its
            // existing collider still receives nail and contact checks.
            actor.health.tick();
            if crate::world::contains(region.collision_bounds, actor.x, actor.y) {
                actor.advance(spec, region, room, state);
            }
            let bounds = actor.bounds(spec);
            if !polygon.is_empty() && hk_sim::polygon_hits_box(polygon, bounds) {
                if vitals.can_control() {
                    response.contact(
                        crate::NAIL_RESPONSE_PARAMS,
                        nail.kind,
                        player.facing,
                        player,
                    );
                }
                match actor
                    .health
                    .hit(spec.health, crate::VITAL_PARAMS.nail_damage)
                {
                    Hit::Damaged | Hit::Killed => {
                        events.hits += 1;
                        if actor.health.dead {
                            events.kills += 1;
                            state.enemy_death_particles(region,actor.source_id,[actor.x,actor.y,0]);
                            if let Some(corpse_spec)=spec.corpse {
                                let mut corpse=hk_sim::Corpse::spawn(corpse_spec,actor.x,actor.y,nail.kind,
                                    player.facing,actor.source_id);
                                // EmitCorpse chooses world X sign from the hit cardinal,
                                // cancelling the parent actor sign for horizontal strikes.
                                corpse.facing=if nail.kind<2 {-player.facing}else{actor.walk.direction};
                                actor.corpse=Some(corpse);
                            }
                        }
                        vitals.gain_soul_on_nail_hit(crate::VITAL_PARAMS);
                        actor.recoil_left = spec.recoil_ticks;
                        actor.recoil = match nail.kind {
                            2 => [0, spec.recoil_speed],
                            3 => [0, -spec.recoil_speed],
                            _ => [player.facing * spec.recoil_speed, 0],
                        };
                    }
                    _ => {}
                }
            }
            let damage = actor.health.contact_damage(spec.health);
            if damage != 0 && overlap(body, bounds) {
                let direction = if player.x < actor.x { -1 } else { 1 };
                let hurt = vitals.hurt(crate::VITAL_PARAMS, damage, direction, false);
                if hurt != Hurt::Ignored {
                    events.hurt = hurt;
                }
            }
        }
        events
    }
    /// Create the immutable visible working set before preparing the GPU cache.
    pub fn prepare_draws(&self, region: &Region, room: &Room, camera: (i32, i32)) -> Draws {
        let mut draws = Draws {
            entries: [None; MAX_VISIBLE],
            len: 0,
        };
        for actor in self
            .actors
            .iter()
            .flatten()
            .filter(|a| a.scene == region.scene)
        {
            let Some(spec) = region
                .actors
                .iter()
                .find(|s| s.source_id == actor.source_id)
            else {
                continue;
            };
            let (clip_id,animation_tick,position,facing)=if actor.health.dead {
                let (Some(corpse),Some(corpse_spec))=(actor.corpse,spec.corpse) else {continue;};
                if !corpse.visible(){continue;}
                (corpse.clip(corpse_spec),corpse.animation_tick,(corpse.x,corpse.y),corpse.facing)
            } else {
                (if actor.walk.turn_remaining!=0 {spec.turn_clip}else{spec.walk_clip},
                    actor.walk.animation_tick,(actor.x,actor.y),actor.walk.direction)
            };
            assert!((clip_id as usize) < room.counts[4]);
            let clip = room.clip(clip_id as usize);
            let elapsed = (animation_tick as u64 * clip[2] as u64 / (60 * 65536)) as u32;
            let mode = clip[3] & 65535;
            let start = clip[3] >> 16;
            let frame = if mode == 0 {
                elapsed % clip[1]
            } else if mode == 1 && elapsed >= clip[1] {
                start + (elapsed - start) % (clip[1] - start)
            } else {
                elapsed.min(clip[1] - 1)
            };
            let f = room.frame((clip[0] + frame) as usize);
            let b = core::array::from_fn::<_, 4, _>(|i| i32_at(f, 4 + i * 4));
            let corners = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
            let mut verts = [(0i16, 0i16); 4];
            for (v, &(x, y)) in verts.iter_mut().zip(corners.iter()) {
                let x = position.0 - x * facing - camera.0;
                let y = position.1 + y - camera.1;
                *v = (
                    (160 + (((x as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
                    (120 - (((y as i64 >> 8) * crate::KNIGHT_SCALE as i64) >> 20)) as i16,
                );
            }
            if verts.iter().all(|v| v.0 < 0)
                || verts.iter().all(|v| v.0 >= 320)
                || verts.iter().all(|v| v.1 < 0)
                || verts.iter().all(|v| v.1 >= 240)
            {
                continue;
            }
            assert!(
                draws.len < MAX_VISIBLE,
                "visible enemy cache budget exceeded"
            );
            draws.entries[draws.len] = Some(Draw {
                texture: u32_at(f, 0) as u16,
                verts,
            });
            draws.len += 1;
        }
        draws
    }
}
fn overlap(a: [i32; 4], b: [i32; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
#[derive(Clone, Copy)]
struct Draw {
    texture: u16,
    verts: [(i16, i16); 4],
}
pub struct Draws {
    entries: [Option<Draw>; MAX_VISIBLE],
    len: usize,
}
impl Draws {
    pub fn append_needed(&self, needed: &mut [u16], len: &mut usize) {
        for entry in self.entries.iter().flatten() {
            if needed[..*len].contains(&entry.texture) {
                continue;
            }
            assert!(*len < needed.len(), "animation working set exceeded");
            needed[*len] = entry.texture;
            *len += 1;
        }
    }
    pub fn draw(&self) -> u32 {
        for entry in self.entries.iter().flatten() {
            crate::render::texture(entry.texture as usize, entry.verts, (128, 128, 128));
        }
        self.len as u32
    }
}

struct PogoTarget {
    bounds: [i32; 4],
    polygons: &'static [&'static [[i32; 2]]],
    breakable: Option<usize>,
    horizontal_and_up: bool,
}
#[cfg(not(test))]
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/pogo.rs"));
#[cfg(test)]
static POGO_SCENES: &[&[PogoTarget]] = &[&[PogoTarget {
    bounds: [-hk_sim::ONE, -hk_sim::ONE, hk_sim::ONE, 0],
    polygons: &[&[
        [-hk_sim::ONE, -hk_sim::ONE],
        [hk_sim::ONE, -hk_sim::ONE],
        [hk_sim::ONE, 0],
        [-hk_sim::ONE, 0],
    ]],
    breakable: Some(0),
    horizontal_and_up: false,
}]];
/// Call before world::State::strike so a prop's first destruction hit can still
/// bounce. Already-destroyed colliders are rejected by their stable source state.
pub fn pogo_contact(
    region: &Region,
    state: &State,
    player: &mut Player,
    nail: &Nail,
    attack: AttackParams,
    polygons: [&[[i32; 2]]; 4],
    response: &mut hk_sim::NailResponse,
) -> bool {
    if !nail.hitting(attack) {
        return false;
    }
    let src = polygons[nail.kind as usize];
    assert!((3..=16).contains(&src.len()));
    let mut points = [[0; 2]; 16];
    let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for (dst, p) in points.iter_mut().zip(src) {
        *dst = [player.x - p[0] * player.facing, player.y + p[1]];
        bounds[0] = bounds[0].min(dst[0]);
        bounds[1] = bounds[1].min(dst[1]);
        bounds[2] = bounds[2].max(dst[0]);
        bounds[3] = bounds[3].max(dst[1]);
    }
    let targets = POGO_SCENES
        .get(region.scene)
        .expect("pogo scene metadata missing");
    for target in *targets {
        if nail.kind != 3 && !target.horizontal_and_up {
            continue;
        }
        if target.breakable.is_some_and(|id| state.broken(id)) {
            continue;
        }
        if overlap(bounds, target.bounds)
            && target
                .polygons
                .iter()
                .any(|p| crate::world::polygons_overlap(&points[..src.len()], p))
        {
            response.contact(
                crate::NAIL_RESPONSE_PARAMS,
                nail.kind,
                player.facing,
                player,
            );
            return true;
        }
    }
    false
}
