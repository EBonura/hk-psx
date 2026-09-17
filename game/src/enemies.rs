//! Source WalkLeftRight / HealthManager subset. States survive cooked grid swaps.
//! Actors use their own resident collision view, independently of the camera.
//! Actors outside all loaded views are suspended. Full Unity activation, Box2D
//! solver parity and global FSM events remain outside this bounded runtime.
use crate::world::{Region, State};
use hk_format::{i32_at, u32_at, Room};
use hk_sim::{
    ActorController, ActorHealth, ActorSpec, AttackParams, Hit, Hurt, Nail, Params, Player, Vitals, WalkState,
    MAX_ACTORS, ONE,
};

/// Source AudioSource/Charge Dust actions. The caller must own these events even
/// before a bank/effect is bound; never silently discard them for enabled actors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunnerEventKind {
    AudioStop,
    AudioPlay,
    ChaseSound { pitch_q16: i32, variant: u8 },
    DustStart,
    DustStop,
    /// GameObject lifetime cleanup, not a fabricated Swipe FSM transition.
    Destroy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunnerEvent {
    pub scene: usize,
    pub source_id: u32,
    pub position: [i32; 2],
    pub facing: i32,
    pub kind: RunnerEventKind,
}
#[derive(Clone, Copy)]
struct RunnerRuntime {
    controller: hk_sim::runner::Runner,
    rng: u32,
    vx: i32,
    animation_tick: u32,
}
impl RunnerRuntime {
    fn draw(rng: &mut u32) -> u32 {
        *rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
        *rng
    }
    fn choose(rng: &mut u32, wait: hk_sim::runner::Wait) -> u16 {
        let [hi, lo] = wait.endpoints();
        lo + (Self::draw(rng) % (hi - lo + 1) as u32) as u16
    }
    fn clip(&self, spec: &ActorSpec) -> u16 {
        let ActorController::Runner { idle_clip, anticipate_clip, lunge_clip, cooldown_clip } = spec.controller
            else { panic!("Runner runtime with Crawler metadata") };
        use hk_sim::runner::Clip;
        match self.controller.animation().map(|a| a.clip).unwrap_or(Clip::Idle) {
            Clip::Idle => idle_clip,
            Clip::Walk => spec.walk_clip,
            Clip::Turn => spec.turn_clip,
            Clip::Anticipate => anticipate_clip,
            Clip::Lunge => lunge_clip,
            Clip::Cooldown => cooldown_clip,
        }
    }
    fn completed(&self, spec: &ActorSpec, room: &Room) -> Option<hk_sim::runner::Animation> {
        let token = self.controller.animation()?;
        let id = self.clip(spec) as usize;
        assert!(id < room.counts[4], "Runner clip not resident");
        let clip = room.clip(id);
        assert!(clip[1] != 0 && clip[2] != 0, "empty Runner clip");
        // Once clips complete at their actual cooked frames/fps boundary.
        // Loop/LoopSection clips never manufacture a completion callback.
        if clip[3] & 65535 == 2
            && self.animation_tick as u64 * clip[2] as u64 >= clip[1] as u64 * 60 * 65536 {
            Some(token)
        } else { None }
    }
}
#[derive(Clone, Copy)]
struct RunnerContext {
    camera: [i32; 3],
    hero: [i32; 2],
    hero_body: [i32; 4],
}

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
    runner: Option<RunnerRuntime>,
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
            runner: if matches!(spec.controller, ActorController::Runner { .. }) {
                assert!(spec.initial_direction == -1 && !spec.random_start_direction,
                    "unsupported Runner initial facing variant");
                Some(RunnerRuntime { controller: hk_sim::runner::Runner::new(), rng: seed,
                    vx: 0, animation_tick: 0 })
            } else { None },
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
    // Kept out of the main loop's frame: 2KiB temporary terrain is reused for
    // each actor/corpse and discarded before combat mutates world state.
    #[inline(never)]
    fn advance_in_view(&mut self,spec:&ActorSpec,state:&State,active:&Region,
        active_room:&Room,target:&Region,target_room:&Room, context:RunnerContext, emit:&mut impl FnMut(RunnerEvent)) {
        const EDGES:usize=128;
        let count=target_room.counts[5];
        let mut edges=[[0;4];EDGES];
        if count<=EDGES {
            state.fill_edges_in_view(active,active_room,target,target_room,&mut edges[..count]);
            self.advance_body(spec,active_room,count,|i|edges[i],context,emit);
        } else {
            self.advance_body(spec,active_room,count,|i|state.edge_in_view(active,active_room,target,target_room,i),context,emit);
        }
    }
    fn advance_body(&mut self,spec:&ActorSpec,room:&Room,count:usize,edge:impl Fn(usize)->[i32;4],context:RunnerContext,emit:&mut impl FnMut(RunnerEvent)) {
        if self.health.dead {
            if let (Some(corpse),Some(corpse_spec))=(&mut self.corpse,spec.corpse) {
                corpse.tick(corpse_spec,count,edge);
            }
        } else {self.advance(spec,room,count,edge,context,emit);}
    }
    fn advance(&mut self, spec: &ActorSpec, room:&Room, count: usize, edge: impl Fn(usize) -> [i32; 4], context:RunnerContext,emit:&mut impl FnMut(RunnerEvent)) {
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
        // runner-crossroads70/enemy-trace.csv: first Runner settles y=2.45249987.
        // crossroads68/landing/room.hk edge118 is floor y=1 at x37..49;
        // body bottom=-1.4375 gives .015 separation (Q16 983). Preserve that
        // grounded clearance in the bounded solver; contact/Sweep/draw bounds
        // remain the source box. Otherwise its lowest horizontal Sweep lies
        // exactly on Terrain and the segment model invents a perpetual wall.
        let foot_skin=if self.runner.is_some() {983}else{0};
        let mut p = Params {
            speed: 0,
            jump: 0,
            gravity: 60 * ONE,
            fall: 100 * ONE,
            hold_ticks: 0,
            min_ticks: 0,
            half_width: (b[2] - b[0]) / 2,
            bottom: b[1]-foot_skin,
            top: b[3],
        };
        if self.spawn_state == SpawnState::Pending {
            // The source Rigidbody2D may begin slightly inside Terrain. Resolve
            // once when its collision apron is resident, never on grid swaps or
            // later jump/recoil motion. Unsupported overlapping geometry leaves
            // this actor suspended until a scene reset, without retrying per tick.
            if !hk_sim::resolve_actor_spawn(&mut body, p, count, &edge) {
                self.spawn_state = SpawnState::Blocked;
                return;
            }
            self.x = body.x - offset;
            self.y = body.y;
            self.spawn_state = SpawnState::Ready;
        }
        let vx = if let Some(mut runner) = self.runner.take() {
            runner.animation_tick = runner.animation_tick.saturating_add(1);
            let senses = self.runner_senses(&runner, spec, room, context, count, &edge);
            let actions = runner.controller.step(senses, |w| RunnerRuntime::choose(&mut runner.rng, w));
            self.apply_runner_actions(&mut runner, actions, emit);
            let vx = runner.vx;
            self.runner = Some(runner);
            vx
        } else {
            let senses = hk_sim::walker_senses(self.bounds(spec), self.walk.direction, count, &edge);
            self.walk.tick(spec.walk, senses.0, senses.1, senses.2)
        };
        // A completed source turn can mirror the asymmetric collider offset.
        let b = self.local_bounds(spec);
        let offset = b[0] + (b[2] - b[0]) / 2;
        body = Player::spawn(self.x + offset, self.y);
        body.vy = self.vy;
        body.grounded = self.grounded;
        p.half_width = (b[2] - b[0]) / 2;
        p.bottom = b[1]-foot_skin;
        p.top = b[3];
        p.speed = vx.abs();
        body.step(p, vx.signum(), false, count, &edge);
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
            body.step(p, self.recoil[0].signum(), false, count, &edge);
        }
        self.x = body.x - offset;
        self.y = body.y;
        assert!(
            self.x.abs() < 512 * ONE && self.y.abs() < 512 * ONE,
            "enemy left validated Q16 scene bounds"
        );
    }
    fn runner_senses(&self, runner:&RunnerRuntime, spec:&ActorSpec, room:&Room,
        context:RunnerContext, count:usize, edge:impl Fn(usize)->[i32;4]) -> hk_sim::runner::Senses {
        use hk_sim::runner_senses as q;
        let pos = [self.x,self.y];
        // Both checked level37 variants have the same body shape and z=.001.
        let expected = q::body_bounds(pos, runner.controller.facing()).expect("Runner bounds");
        assert_eq!(self.bounds(spec), expected, "unsupported Runner collider variant");
        let in_alert_range = q::alert_overlap(pos,context.hero_body).expect("Runner alert bounds");
        let can_see_hero = match q::line_of_sight(pos,context.hero,in_alert_range,count,&edge) {
            Ok(value) => value,
            // Coincident transform points have no proven Physics2D equivalent;
            // conservatively suppress sight (TOOK DAMAGE still reacts).
            Err(q::QueryError::ZeroLengthSight) => false,
            Err(_) => panic!("Runner LOS outside validated coordinates"),
        };
        let probes = q::walker_queries(pos,runner.controller.facing(),count,edge)
            .expect("Runner Sweep outside validated coordinates");
        hk_sim::runner::Senses {
            camera_in_start_range: hk_sim::runner::camera_in_start_range(context.camera,[self.x,self.y,66]),
            hero_x:context.hero[0], actor_x:self.x, in_alert_range, can_see_hero,
            wall:probes.wall, floor_ahead:probes.floor_ahead,
            completed:runner.completed(spec,room),
        }
    }
    fn emit_runner(&self, kind:RunnerEventKind, emit:&mut impl FnMut(RunnerEvent)) {
        emit(RunnerEvent { scene:self.scene,source_id:self.source_id,
            position:[self.x,self.y],facing:self.walk.direction,kind });
    }
    fn apply_runner_actions(&mut self, runner:&mut RunnerRuntime, actions:hk_sim::runner::Actions,
        emit:&mut impl FnMut(RunnerEvent)) {
        use hk_sim::runner::Action;
        self.walk.direction = runner.controller.facing();
        for action in actions.iter() {
            let kind = match action {
                Action::Play(_) => { runner.animation_tick=0; continue; }
                Action::Velocity { x,y } => {
                    if let Some(x)=x {runner.vx=x;}
                    if let Some(y)=y {self.vy=y;}
                    continue;
                }
                Action::AudioStop => RunnerEventKind::AudioStop,
                Action::AudioPlay => RunnerEventKind::AudioPlay,
                Action::ChaseSound => {
                    // Preserve the first overwritten pitch draw, then the actual
                    // .85..1.15 pitch and equal-weight clip selection. This is a
                    // deterministic per-actor RNG, not Unity global RNG parity.
                    let _overwritten_pitch = RunnerRuntime::draw(&mut runner.rng);
                    let pitch_q16 = 55706 + (RunnerRuntime::draw(&mut runner.rng) % 19661) as i32;
                    let variant = (RunnerRuntime::draw(&mut runner.rng) >> 31) as u8;
                    RunnerEventKind::ChaseSound { pitch_q16,variant }
                }
                Action::DustStart => RunnerEventKind::DustStart,
                Action::DustStop => RunnerEventKind::DustStop,
                Action::Turn(_) => continue, // controller/clip/facing own the turn
            };
            self.emit_runner(kind,emit);
        }
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
        for spec in crate::world::region_actors(region) {
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
    pub fn tick<'a>(
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
        cheats: crate::cheats::Settings,
        camera: [i32;3],
        mut runner_event: impl FnMut(RunnerEvent),
        resident: impl Fn(usize, i32, i32) -> Option<(Region, Room<'a>)>,
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
        let context=RunnerContext {camera,hero:[player.x,player.y],hero_body:body};
        for actor in self
            .actors
            .iter_mut()
            .flatten()
            .filter(|a| a.scene == region.scene)
        {
            let Some(spec) = crate::world::region_actors(region)
                .find(|s| s.source_id == actor.source_id)
            else {
                continue;
            };
            // WalkLeftRight keeps running independently of the Knight/camera.
            // A cooked view boundary is not a source scene or FSM boundary.
            let (x,y)=actor.corpse.map_or((actor.x,actor.y),|c|(c.x,c.y));
            let other=if crate::world::contains(region.bounds,x,y) {None}
                else {resident(region.scene,x,y)};
            let (terrain_region,terrain_room)=other.as_ref()
                .map_or((region,room),|(r,room)|(r,room));
            let available=terrain_region.scene==region.scene
                && crate::world::contains(terrain_region.collision_bounds,x,y);
            if !actor.health.dead {actor.health.tick();}
            if available {
                actor.advance_in_view(spec,state,region,room,terrain_region,terrain_room,context,&mut runner_event);
            }
            if actor.health.dead {continue;}
            if actor.runner.is_some() && (!available || actor.spawn_state==SpawnState::Blocked) {
                continue; // no attack/recoil sensing against an unavailable terrain owner
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
                    .hit(spec.health, cheats.params(crate::VITAL_PARAMS).nail_damage)
                {
                    Hit::Damaged | Hit::Killed => {
                        events.hits += 1;
                        if actor.health.dead {
                            events.kills += 1;
                            if let Some(runner)=&mut actor.runner {
                                runner.controller.die();
                                actor.emit_runner(RunnerEventKind::Destroy,&mut runner_event);
                            }
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
                        // HealthManager TOOK DAMAGE precedes Recoil's global
                        // horizontal event. RecoilByDirection ignores a request
                        // while already recoiling; accepted damage still reacts.
                        let begin_recoil = actor.runner.is_none() || actor.recoil_left == 0;
                        if !actor.health.dead {
                            if let Some(mut runner)=actor.runner.take() {
                                let actions=runner.controller.took_damage(player.x,actor.x);
                                actor.apply_runner_actions(&mut runner,actions,&mut runner_event);
                                if nail.kind<2 && begin_recoil {
                                    let senses=actor.runner_senses(&runner,spec,room,context,
                                        terrain_room.counts[5],|i|state.edge_in_view(region,room,terrain_region,terrain_room,i));
                                    let actions=runner.controller.horizontal_recoil(senses,|w|RunnerRuntime::choose(&mut runner.rng,w));
                                    actor.apply_runner_actions(&mut runner,actions,&mut runner_event);
                                }
                                actor.runner=Some(runner);
                            }
                        }
                        if begin_recoil {
                        actor.recoil_left = spec.recoil_ticks;
                        actor.recoil = match nail.kind {
                            2 => [0, spec.recoil_speed],
                            3 => [0, -spec.recoil_speed],
                            _ => [player.facing * spec.recoil_speed, 0],
                        };
                        }
                    }
                    _ => {}
                }
            }
            let damage = actor.health.contact_damage(spec.health);
            if damage != 0 && overlap(body, bounds) {
                let direction = if player.x < actor.x { -1 } else { 1 };
                let hurt = cheats.hurt(vitals,crate::VITAL_PARAMS, damage, direction, false);
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
            let Some(spec) = crate::world::region_actors(region)
                .find(|s| s.source_id == actor.source_id)
            else {
                continue;
            };
            let (clip_id,animation_tick,position,facing)=if actor.health.dead {
                let (Some(corpse),Some(corpse_spec))=(actor.corpse,spec.corpse) else {continue;};
                if !corpse.visible(){continue;}
                (corpse.clip(corpse_spec),corpse.animation_tick,(corpse.x,corpse.y),corpse.facing)
            } else if let Some(runner)=actor.runner {
                (runner.clip(spec),runner.animation_tick,(actor.x,actor.y),runner.controller.facing())
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
    for target in crate::world::pogo_targets(region) {
        if nail.kind != 3 && !target.horizontal_and_up() {
            continue;
        }
        if target.breakable().is_some_and(|id| state.broken(id)) {
            continue;
        }
        if overlap(bounds, target.bounds()) && target.hit_by(&points[..src.len()]) {
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

#[cfg(test)]
mod runner_runtime_tests {
    use super::*;
    use hk_sim::runner::{Swipe, Walker};
    const SPEC: ActorSpec = ActorSpec {
        controller: ActorController::Runner {
            idle_clip: 2, anticipate_clip: 3, lunge_clip: 4, cooldown_clip: 5,
        },
        source_id: 5196, x: 0, y: 94208,
        bounds: [-51200,-94208,28672,12288],
        health: hk_sim::EnemyParams {health:15,contact_damage:1,evasion_ticks:12,
            invincible:false,damage_override:false},
        walk:hk_sim::WalkParams {speed:ONE+ONE/2,turn_ticks:10,turn_cooldown_ticks:60},
        initial_direction:-1,random_start_direction:false,walk_clip:0,turn_clip:1,
        corpse:Some(hk_sim::CorpseSpec {air_clip:2,land_clip:2,bounds:[-ONE/2,-ONE,ONE/2,0],
            spawn_offset:[0,ONE/2],bounce_factor:13107}),
        recoil_speed:10*ONE,recoil_ticks:9,
    };
    // Source-free six-clip bank with the real Runner rates/lengths. Each clip's
    // frames reference a distinct texture so draw selection is independently visible.
    fn bytes() -> Vec<u8> {
        let lengths=[7u32,2,6,5,8,1];
        let mut b=Vec::from(*b"HKROOM02");
        for n in [1,6,0,29,6,1,0,0] {b.extend((n as u32).to_le_bytes());}
        for _ in 0..6 {for n in [0u16,0,0,4,4,0] {b.extend(n.to_le_bytes());}
            b.extend(0u32.to_le_bytes());}
        for (clip,len) in lengths.iter().enumerate() {
            for _ in 0..*len {for n in [clip as i32,-ONE/2,-ONE,ONE/2,0] {b.extend(n.to_le_bytes());}}
        }
        let mut first=0;
        for (i,len) in lengths.iter().enumerate() {
            let rate=if i==0||i==2 {10}else{12};
            let wrap=if i==0||i==2 {0}else{2};
            for n in [first,*len,rate*65536,wrap] {b.extend(n.to_le_bytes());}
            first+=len;
        }
        for n in [-100*ONE,0,100*ONE,0] {b.extend(n.to_le_bytes());}
        b.resize(b.len()+6*32+32768,0); b
    }
    fn context(hero_x:i32) -> RunnerContext {
        let y=SPEC.y;
        RunnerContext {camera:[0,2*ONE,-2496922],hero:[hero_x,y],
            hero_body:[hero_x-crate::PARAMS.half_width,y+crate::PARAMS.bottom,
                hero_x+crate::PARAMS.half_width,y+crate::PARAMS.top]}
    }
    fn advance(actor:&mut Actor,room:&Room,c:RunnerContext,events:&mut Vec<RunnerEvent>) {
        actor.advance(&SPEC,room,room.counts[5],|i|room.edge(i),c,&mut |e|events.push(e));
    }
    fn phase(actor:&Actor)->Swipe {actor.runner.unwrap().controller.swipe()}
    fn region()->Region {
        Region {scene:0,bounds:[-100*ONE,-10*ONE,100*ONE,30*ONE],
            collision_bounds:[-100*ONE,-10*ONE,100*ONE,30*ONE],actors:&[&SPEC]}
    }
    #[test]
    fn actual_runner_clock_motion_clips_and_ordered_effects() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut actor=Actor::new(0,&SPEC);let mut events=Vec::new();let c=context(3*ONE);
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(phase(&actor),Swipe::Anticipate);
        assert_eq!(actor.walk.direction,1);
        assert_eq!(actor.runner.unwrap().clip(&SPEC),3);
        assert_eq!(events.iter().map(|e|e.kind).take(3).collect::<Vec<_>>(),
            [RunnerEventKind::AudioStop,RunnerEventKind::AudioPlay,RunnerEventKind::AudioStop]);
        assert!(matches!(events[3].kind,RunnerEventKind::ChaseSound {pitch_q16:55706..=75366,variant:0..=1}));
        for _ in 0..24 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(phase(&actor),Swipe::Anticipate);
        let before=actor.x;
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(phase(&actor),Swipe::Lunge);
        assert_eq!(actor.runner.unwrap().clip(&SPEC),4);
        for _ in 0..39 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(phase(&actor),Swipe::Lunge);
        assert_eq!(actor.x-before,40*(6*ONE/60));
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(phase(&actor),Swipe::Cooldown);
        let stopped=actor.x;
        for _ in 0..5 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(phase(&actor),Swipe::Idle);
        assert_eq!(actor.x,stopped);
        let mut far=context(30*ONE);far.camera=[0,2*ONE,-2496922];
        for _ in 0..15 {advance(&mut actor,&room,far,&mut events);}
        assert_eq!(phase(&actor),Swipe::Ready);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::DustStart).count(),1);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::DustStop).count(),1);
        let mut world=EnemyWorld::new();world.actors[0]=Some(actor);
        let draws=world.prepare_draws(&region(),&room,(actor.x,actor.y));
        assert_eq!(draws.entries[0].unwrap().texture,0); // actual Walk clip
        assert!(core::mem::size_of::<RunnerRuntime>()<=64);
        assert!(core::mem::size_of::<Actor>()<=192);
        println!("RunnerRuntime={} Actor={} EnemyWorld={}",core::mem::size_of::<RunnerRuntime>(),
            core::mem::size_of::<Actor>(),core::mem::size_of::<EnemyWorld>());
    }
    #[test]
    fn true_camera_depth_gates_start_and_live_terrain_blocks_los() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut actor=Actor::new(0,&SPEC);let mut events=Vec::new();
        let mut c=context(30*ONE);c.camera=[50*ONE,SPEC.y,-40*ONE];
        advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner.unwrap().controller.walker(),Walker::Waiting);
        assert_eq!(actor.x,SPEC.x);assert!(events.is_empty());
        c.camera=[0,SPEC.y,-40*ONE];advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner.unwrap().controller.walker(),Walker::Walking);
        assert_eq!(actor.y,-SPEC.bounds[1]+983);
        let before=actor.x;
        for _ in 0..40 {advance(&mut actor,&room,c,&mut events);}
        assert_eq!(actor.runner.unwrap().controller.walker(),Walker::Walking);
        assert!(actor.x<before);
        assert_eq!(actor.y,-SPEC.bounds[1]+983);
        c.camera=[200*ONE,200*ONE,-40*ONE];advance(&mut actor,&room,c,&mut events);
        assert_eq!(actor.runner.unwrap().controller.walker(),Walker::Walking);
        let actor=Actor::new(0,&SPEC);let runner=actor.runner.unwrap();
        let floor=room.edge(0);let wall=[ONE,0,ONE,5*ONE];
        let blocked=actor.runner_senses(&runner,&SPEC,&room,context(3*ONE),2,
            |i|if i==0 {floor}else{wall});
        assert!(blocked.in_alert_range);assert!(!blocked.can_see_hero);
        let clear=actor.runner_senses(&runner,&SPEC,&room,context(3*ONE),2,
            |i|if i==0 {floor}else{[0;4]});
        assert!(clear.can_see_hero); // destroyed/disabled live terrain sentinel
    }
    #[test]
    fn accepted_hits_reset_in_order_and_death_owns_single_corpse_geo_event() {
        let b=bytes();let room=Room::parse(&b).unwrap();let r=region();
        let mut world=EnemyWorld::new();let mut player=Player::spawn(0,SPEC.y);
        let mut vitals=Vitals::new(crate::VITAL_PARAMS);let mut nail=Nail::new();
        let attack=AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8};
        let poly=&[[-3*ONE,-3*ONE],[3*ONE,-3*ONE],[3*ONE,3*ONE],[-3*ONE,3*ONE]][..];
        let mut events=Vec::new();let mut state=State;let mut response=hk_sim::NailResponse::new();
        let cheats=crate::cheats::Settings {invincible:true,..crate::cheats::Settings::new()};
        // Coincident origin has conservatively unknown LOS. Accepted damage must
        // nevertheless enter anticipation BEFORE its horizontal reset restarts Walk.
        world.sync_region(&r);nail.active=true;nail.age=1;
        let hit=world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&mut response,
            attack,[poly;4],cheats,[0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
        assert_eq!((hit.hits,hit.kills),(1,0));
        assert_eq!(world.actors[0].unwrap().health.hp,10);
        let chase=events.iter().position(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})).unwrap();
        assert!(events[chase+1..].iter().any(|e|e.kind==RunnerEventKind::AudioPlay));
        assert_eq!(world.actors[0].unwrap().recoil_left,9);
        let after_first=events.len();
        world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&mut response,
            attack,[poly;4],cheats,[0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
        assert_eq!(world.actors[0].unwrap().health.hp,10); // evaded overlap
        assert!(!events[after_first..].iter().any(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})));
        // Isolate two further accepted health events from traversal/recoil timing.
        for expected in [5,0] {
            let actor=world.actors[0].as_mut().unwrap();
            actor.health.evasion_ticks=0;actor.recoil_left=0;player.x=actor.x;player.y=actor.y;
            let result=world.tick(&r,&room,&mut state,&mut player,&mut vitals,&nail,&mut response,
                attack,[poly;4],cheats,[0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
            assert_eq!(world.actors[0].unwrap().health.hp,expected);
            assert_eq!(result.kills,u16::from(expected==0));
        }
        let actor=world.actors[0].unwrap();assert!(actor.corpse.is_some());
        assert_eq!(phase(&actor),Swipe::Dead);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::Destroy).count(),1);
        let mut paid=Vec::new();
        world.take_geo_deaths(0,|id,_,_|{paid.push(id);true});
        world.take_geo_deaths(0,|id,_,_|{paid.push(id);true});
        assert_eq!(paid,[SPEC.source_id]);
        assert_eq!(vitals.soul,33);
    }
    #[test]
    fn source_seed_and_effect_pitch_sequence_are_deterministic() {
        let b=bytes();let room=Room::parse(&b).unwrap();
        let mut a=Actor::new(0,&SPEC);let mut b=Actor::new(0,&SPEC);
        let mut ea=Vec::new();let mut eb=Vec::new();
        for _ in 0..180 {
            advance(&mut a,&room,context(3*ONE),&mut ea);
            advance(&mut b,&room,context(3*ONE),&mut eb);
            assert_eq!((a.x,a.y,a.walk.direction),(b.x,b.y,b.walk.direction));
        }
        assert_eq!(ea,eb);
        assert_eq!(a.runner.unwrap().rng,b.runner.unwrap().rng);
        assert!(ea.iter().any(|e|matches!(e.kind,RunnerEventKind::ChaseSound {..})));
    }
    #[test]
    fn runner_vertical_hit_does_not_emit_horizontal_restart() {
        let b=bytes();let room=Room::parse(&b).unwrap();let r=region();
        let mut world=EnemyWorld::new();let mut player=Player::spawn(0,SPEC.y);
        let mut vitals=Vitals::new(crate::VITAL_PARAMS);let mut nail=Nail::new();
        nail.active=true;nail.age=1;nail.kind=2;
        let attack=AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8};
        let poly=&[[-3*ONE,-3*ONE],[3*ONE,-3*ONE],[3*ONE,3*ONE],[-3*ONE,3*ONE]][..];
        let mut events=Vec::new();
        let hit=world.tick(&r,&room,&mut State,&mut player,&mut vitals,&nail,
            &mut hk_sim::NailResponse::new(),attack,[poly;4],
            crate::cheats::Settings {invincible:true,..crate::cheats::Settings::new()},
            [0,SPEC.y,-2496922],|e|events.push(e),|_,_,_|None);
        assert_eq!(hit.hits,1);
        let actor=world.actors[0].unwrap();
        assert_eq!(phase(&actor),Swipe::Anticipate);
        assert_eq!(actor.recoil,[0,10*ONE]);
        assert_eq!(events.iter().filter(|e|e.kind==RunnerEventKind::AudioPlay).count(),1);
    }
    #[test]
    fn unavailable_runner_terrain_suspends_clock_and_resumes_same_actor() {
        let b=bytes();let room=Room::parse(&b).unwrap();let r=region();
        let missing=Region {bounds:[110*ONE,0,120*ONE,5*ONE],
            collision_bounds:[110*ONE,0,120*ONE,5*ONE],..region()};
        let mut world=EnemyWorld::new();let mut events=Vec::new();
        let tick=|world:&mut EnemyWorld,r:&Region,events:&mut Vec<RunnerEvent>| {
            world.tick(r,&room,&mut State,&mut Player::spawn(30*ONE,SPEC.y),
                &mut Vitals::new(crate::VITAL_PARAMS),&Nail::new(),
                &mut hk_sim::NailResponse::new(),
                AttackParams {duration:20,cooldown:24,alternate_reset:30,hit_start:1,hit_end:8},
                [&[];4],crate::cheats::Settings::new(),[0,SPEC.y,-2496922],
                |e|events.push(e),|_,_,_|None);
        };
        tick(&mut world,&r,&mut events);
        let before=world.actors[0].unwrap();events.clear();
        for _ in 0..120 {tick(&mut world,&missing,&mut events);}
        let suspended=world.actors[0].unwrap();
        assert_eq!((before.x,before.y),(suspended.x,suspended.y));
        assert_eq!(before.runner.unwrap().animation_tick,suspended.runner.unwrap().animation_tick);
        assert_eq!(before.runner.unwrap().rng,suspended.runner.unwrap().rng);
        assert!(events.is_empty());
        tick(&mut world,&r,&mut events);
        let resumed=world.actors[0].unwrap();
        assert_eq!(resumed.source_id,before.source_id);
        assert_eq!(resumed.runner.unwrap().animation_tick,before.runner.unwrap().animation_tick+1);
    }

}
