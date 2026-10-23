//! The frame loop's stages, and the state they carry between them.
//!
//! These left `main` because one function's generated code has to fit inside
//! what a MIPS PC16 branch reaches. Past that the integrated assembler refuses
//! with "out of range PC16 fixup", which names no file, no function and no
//! line, and it only refuses when a branch pair happens to span too far: `main`
//! was over the limit and linking by luck, so every feature touching the frame
//! loop was another roll of the same die. `#[inline(never)]` on the four stage
//! functions below is what keeps them out of `main`, and it is the only place
//! in this port where that attribute has a reason of its own.
use super::*;

/// What the frame loop carries from one tick to the next.
///
/// The stages touch upwards of thirty of these between them, so a struct is
/// what lets them be functions at all: passing the locals would have replaced
/// one unreadable function with several unreadable signatures. Grouped by what
/// owns the value, not by type.
pub struct Game {
    // The Knight and the moves that take his body.
    pub player: Player,
    pub vitals: hk_sim::Vitals,
    pub nail: Nail,
    pub nail_response: hk_sim::NailResponse,
    pub focus: hk_sim::Focus,
    pub dream: hk_sim::DreamNail,
    pub cast: hk_sim::Cast,
    // The world he stands in. `geo` and `life` are the two arenas that outlive
    // a scene load, so they stay the statics they were and are held by
    // reference; every other world is owned here.
    pub state: world::State,
    pub reveals: reveal_masks::State,
    pub enemies: enemies::EnemyWorld,
    pub door: great_door::World,
    pub shade: shade::World,
    pub bench: bench::State,
    pub camera: camera::Camera,
    /// The look clip the Knight plays while he stands (look_pose.rs).
    pub look: crate::look_pose::LookPose,
    pub geo: &'static mut geo::World,
    pub life: &'static mut lifeblood::World,
    pub props: &'static mut props::World,
    pub pickups: &'static mut pickups::World,
    // Screens and the input edges they read.
    pub settings: menu::Settings,
    pub pause_menu: pause::State,
    pub shop_screen: shop::Screen,
    pub paused: bool,
    pub start_held: bool,
    pub inventory_held: bool,
    pub prev_pad: ButtonState,
    // The seated save prompt and what follows it: the notice while the card is
    // written, the result, and a failure the player has to see (save_ui.rs).
    pub save_ui: save_ui::Ui,
    // Pending Save Game record, written at the frame boundary once the player
    // accepts the bench prompt. Nothing else ever writes the card.
    pub save_requested: Option<save::Save>,
    // Bench respawn marker: scene, seat, facing, catalogue slot. None means the
    // new-game spawn.
    pub respawn: Option<(usize, [i32; 2], i32, usize)>,
    // The hazard respawn marker, which a checkpoint volume moves.
    pub safe: (i32, i32, usize),
    pub safe_facing: i32,
    pub attacks: u32,
    // Ticks since the current scene started (source Delay Collider gates).
    pub scene_ticks: u32,
    pub gate_cooldown: u16,
    pub region_id: usize,
    /// The view being drawn: the scene view whose cooked camera range holds
    /// the camera (disc::Cache::camera_view), usually `region_id`'s own.
    pub view: usize,
    /// A gate taken and fading out (source CameraFade FadingOut) before the
    /// scene is left; see `EXIT_FADE_TICKS`.
    pub exit: Option<Exit>,
    /// The spell orb's `Check Fall` has reached black: move the Knight to where
    /// he wakes, on the next simulation tick (shaman.rs).
    pub wake: bool,
    /// Ticks the screen has been black after a death, before the respawn load: see `DEATH_HOLD_TICKS`.
    pub death_hold: u16,
}
/// `GameManager.PlayerDead`: after `DEATH_WAIT` (the vitals' 171 ticks) the camera fades out, waits
/// 0.8 s and only then calls `ReadyForRespawn`. The fade is the camera's ordinary one (0.33 s, 20
/// ticks, `EXIT_FADE_TICKS`), so the screen is black for the 28 ticks that remain.
pub const DEATH_HOLD_TICKS: u16 = 48;
/// Total health (masks and Lifeblood) at or below which a hit can kill, so the Death clip is carried:
/// the most any one hit takes is two masks, doubled by Fragile and Unbreakable overcharming.
const LOW_HEALTH: u32 = 4;
/// Frames the Death clip was due and its pack had not arrived, for the route pins (none expected).
#[no_mangle]
pub static mut HK_STREAM_ART_LATE: u32 = 0;
/// Ticks spent in that fade and hold across the run, for the route pins.
#[no_mangle]
pub static mut HK_DEATH_HOLD_TICKS: u32 = 0;
/// Source `CameraFade` FSM, state FadingOut: CameraFadeOut over 0.33 s.
/// The camera holds and a side-gate Knight keeps running out while it fades.
pub const EXIT_FADE_TICKS: u16 = 20;
/// The first ticks of that fade are ordinary frames (the Knight runs on); the
/// rest runs on the last of them while the next scene loads (exit_fade).
pub const EXIT_LIVE_TICKS: u16 = match option_env!("HK_EXIT_LIVE_TICKS") {
    Some(s) => {
        let b = s.as_bytes();
        let mut i = 0;
        let mut v = 0u16;
        while i < b.len() {
            v = v * 10 + (b[i] - b'0') as u16;
            i += 1;
        }
        v
    }
    None => 6,
};
#[derive(Clone, Copy)]
pub struct Exit {
    pub gate: world::Gate,
    pub door_exit: bool,
    pub age: u16,
    pub dir: i32,
}
impl Game {
    /// Ticks since the Knight died: the vitals' own countdown, then the hold of the fade.
    pub fn death_age(&self) -> u32 {
        (VITAL_PARAMS.death_ticks - self.vitals.death_ticks) as u32 + self.death_hold as u32
    }
    /// The Knight's `Death` clip and its age while he is dead (the Hero Death object plays it where
    /// he fell, the renderer being off).
    pub fn death_pose(&self) -> Option<(usize, u32)> {
        // The body goes when the clip ends (`End` turns the mesh renderer off).
        let age = self.death_age();
        (self.vitals.dead
            && age < ability_art::clip_ticks(ability_art::DEATH)
            && ability_art::ready(ability_art::DEATH))
        .then_some((ability_art::DEATH, age))
    }
    /// Keep the streamed Knight art (ability_art.rs `STREAMS`) in the room pool for the moments that
    /// need it, and let it go otherwise (modules.rs `carry`; a gate reads what a scene carries, the
    /// pool's background fetch reads what the current scene carries).
    ///
    /// The Death clip is wanted from the time a hit could kill (`LOW_HEALTH`, or any boss fight) until
    /// the clip ends. A boss song takes the drive for the whole fight, so the song waits for the fetch
    /// (modules.rs `fetch_pending`) and the respawn clips are fetched with the Death clip, in the boss
    /// room; otherwise they are fetched once the Death clip is over, in the room he died in. When the
    /// respawn load starts they are carried into the scene he wakes in (`frame::respawn`), which keeps
    /// them through that gate, until he is up.
    #[cfg(not(test))]
    #[inline(never)]
    pub fn carry_streamed_art(&self, current: usize) {
        // Nearly every frame nothing is wanted and nothing was: two compares, no table work.
        static mut CARRYING: bool = false;
        let quiet = !self.vitals.dead
            && u32::from(self.vitals.health) + u32::from(self.vitals.blue_health) > LOW_HEALTH
            && !self.bench.respawning()
            && !music::boss_active();
        if quiet && unsafe { !CARRYING } {
            return;
        }
        unsafe {
            CARRYING = !quiet;
        }
        let here = crate::disc::manifest_index(current);
        let dying = self.vitals.dead;
        let clip_over = dying && self.death_age() >= ability_art::clip_ticks(ability_art::DEATH);
        let respawning = self.bench.respawning();
        let boss = music::boss_active();
        let low = u32::from(self.vitals.health) + u32::from(self.vitals.blue_health) <= LOW_HEALTH
            || boss;
        let death = (dying && !clip_over) || (!dying && !respawning && low);
        let wake = if respawning {
            crate::disc::manifest_index(world::scene_of(self.respawn.map_or(0, |r| r.3)))
        } else if boss || clip_over {
            here
        } else {
            None
        };
        crate::modules::carry(crate::modules::ART_DEATH, here.filter(|_| death));
        crate::modules::carry(crate::modules::ART_WAKE, wake);
        if dying && !clip_over && !ability_art::ready(ability_art::DEATH) {
            unsafe {
                HK_STREAM_ART_LATE = HK_STREAM_ART_LATE.saturating_add(1);
            }
        }
    }
    /// Carry the ability effects (`art_fx`) into the scene while the Knight owns an ability that
    /// draws one, so every gate reads them with the scene's own art. Asked again only when the scene
    /// or the answer changes: nearly every frame it is one compare.
    #[cfg(not(test))]
    #[inline(never)]
    pub fn carry_ability_art(&self, current: usize) {
        static mut ASKED: usize = usize::MAX;
        let wants = ability_fx::wanted(&self.player, self.cast.has_fireball);
        let key = current << 1 | usize::from(wants);
        if unsafe { ASKED } == key {
            return;
        }
        unsafe {
            ASKED = key;
        }
        crate::modules::carry(
            crate::modules::ART_FX,
            crate::disc::manifest_index(current).filter(|_| wants),
        );
    }
    /// Screen darkness for the death fade, 0 (none) to 255 (black).
    pub fn death_shade(&self) -> u8 {
        if !self.vitals.dead {
            return 0;
        }
        let into = self.death_age().saturating_sub(VITAL_PARAMS.death_ticks as u32);
        (into.min(EXIT_FADE_TICKS as u32) * 255 / EXIT_FADE_TICKS as u32) as u8
    }
    /// Screen darkness for the exit fade, 0 (none) to 255 (black).
    pub fn exit_shade(&self) -> u8 {
        self.exit.map_or(0, |e| {
            ((e.age.min(EXIT_FADE_TICKS) as u32 * 255) / EXIT_FADE_TICKS as u32) as u8
        })
    }
}

/// Frames that drew an NPC of another view than the Knight's (one the camera shows).
#[no_mangle]
pub static mut HK_VIEW_NPC_DRAWS: u32 = 0;
/// RENDER: the visual frame for the simulation state as it stands.
///
/// `camera` is the frame's camera, read once in main for the view bind and
/// the other-view NPC test as well.
#[inline(never)]
///
/// `view` is the drawn view (`game.view`), whose draw indices every scenery
/// visibility below is keyed to; `r` and `room` stay the Knight's own view for
/// actors, clips and colliders. Enemies need nothing more: every view of a
/// scene lists all of its actors and the pool is scene-wide (world::region_actors),
/// so an actor draws wherever the camera shows it. An NPC is cooked into the one
/// view holding it, with its clips in that view's room, so `other_npc` is one
/// the camera shows from another view than the Knight's, with that view's room.
pub fn render(
    game: &mut Game,
    r: &world::Region,
    view: &world::Region,
    room: &Room,
    other_npc: Option<(world::Npc, &Room)>,
    fb: &mut FrameBuffer,
    npc: Option<world::Npc>,
    camera: (i32, i32),
    sim_clock: u32,
) -> u32 {
    game.state.apply(view);
    apply_reveal_masks(&game.reveals, view);
    geo_render::apply(game.geo, &mut game.state, game.region_id, game.view);
    lifeblood::apply(game.life, &mut game.state, game.region_id, game.view);
    props::apply(game.view);
    pickups::apply(game.view);
    decor::apply(game.view, game.scene_ticks);
    great_door::apply(&game.door, &mut game.state, game.region_id);
    // After the Lifeblood refresh, which is what clears the exclusion list, and
    // beside the door's for the same reason. A gate that lifts inside a
    // simulation tick reaches the terrain on the next frame's rebuild, which is
    // two seconds after the boss dies with nothing standing in the doorway.
    battle_gates::apply(&mut game.state, game.region_id, game.view);
    blocker_terrain::apply(&mut game.state, game.region_id);
    // Build and start kicking the back pass now, before the actors, the
    // working set and its uploads below, so the GPU starts drawing while the
    // CPU does that work. Only where nothing bound below can change one of its
    // packets (render::early_back), with this view's own room and no other
    // view's NPC; the uploads then wait for its DMA. The packet order is the
    // same either way.
    // The Shaman's veil at full strength subtracts white from everything drawn
    // before it (the world and the HUD): every one of those pixels ends black
    // whatever it was, so the scenery passes under it are not drawn.
    let veiled = shaman::shade() == 255;
    // The bench owns the body first, then an ability, then the room clip.
    let kneel = pickups::kneel_frame(game.pickups, game.region_id);
    let pose = if game.bench.animation().is_some() || kneel.is_some() {
        None
    } else {
        game.death_pose()
            .or_else(|| game.bench.ability_animation())
            .or_else(|| ability_pose(&game.player, &game.dream, &game.cast))
            .or_else(|| game.look.pose())
    };
    let body_frame = if let Some(f) = kneel {
        f
    } else if let Some((clip, age)) = game.bench.animation() {
        clip_frame(room, clip, age)
    } else {
        knight_frame(room, &game.player, &game.focus)
    };
    let effect_frame = nail_frame(room, &game.nail);
    let ability_frame = pose.map(|(clip, age)| ability_art::frame_index(clip, age));
    // The Knight's own art is the request that most often misses (his frame
    // advances), and a miss makes the uploads wait for the early back pass's
    // whole DMA with the CPU idle: such a frame draws in the ordinary order.
    let knight_key = match ability_frame {
        Some(i) => ability_art::KEY_BASE + i as u16,
        None => u32_at(room.frame(body_frame), 0) as u16,
    };
    let early = other_npc.is_none()
        && game.view == game.region_id
        && render::early_back(room)
        && animation_cache::resident(knight_key)
        && effect_frame.is_none_or(|f| animation_cache::resident(u32_at(room.frame(f), 0) as u16));
    let mut prims = 0;
    if early {
        render::begin_frame(fb.buffer_y(fb.drawing));
        // The early back pass draws with the vignette words too, so they
        // refresh here as on any other frame (the bound view is this room's).
        #[cfg(feature = "hero-vignette")]
        render::set_vignette(
            (crate::hero_light::vignette_scene(r.scene) && !game.vitals.dead).then(|| {
                (
                    160 + (((game.player.x - camera.0) >> 8) * crate::KNIGHT_SCALE >> 20),
                    120 - (((game.player.y - 38011 - camera.1) >> 8) * crate::KNIGHT_SCALE >> 20),
                )
            }),
            camera,
        );
        // Both scenery passes run with their frames in the scratchpad (spstack.rs).
        fb.clear(9, 13, 20);
        if !veiled {
            prims += unsafe { spstack::sim(|| render::scenery(camera, false)) };
            prims += game.state.draw_secrets(view, camera, false);
        }
        unsafe {
            render::HK_EARLY_BACK_FRAMES = render::HK_EARLY_BACK_FRAMES.wrapping_add(1);
        }
    }
    let door_frame = game.door.frame(game.region_id, camera);
    let enemy_draws = game.enemies.prepare_draws(r, room, camera);
    // Vengeful Spirit's projectile carries its own frame and key.
    let ball_frame = game
        .cast
        .ball_bounds(FIREBALL_PARAMS)
        .map(|_| ability_art::frame_index(ability_art::BALL, game.cast.ball.life as u32));
    // The view's NPC, whose clips ride in this view's own room bank.
    let npc_frame = npc.map(|npc| {
        // Conversation Control reaches Talk Right through Hero Is Right
        // and Talk Left through Hero Is Left, because the NPC's own
        // localScale.x is positive; Idle is what it stands in.
        let slot = if dialogue::npc_speaking() == Some(npc.source_id) {
            if game.player.x >= npc.position[0] {
                2
            } else {
                1
            }
        } else {
            0
        };
        crate::scene_sfx::myla_sing(slot != 0);
        let frame = clip_frame(room, npc.clip_base as usize + slot, sim_clock);
        // Myla's pickaxe: the strike frame of her Idle clip (scene_sfx::myla_strike).
        if slot == 0 {
            let first = room.clip(npc.clip_base as usize)[0] as usize;
            crate::scene_sfx::myla_strike(camera, npc.position, frame - first);
        }
        (npc.position, frame)
    });
    // An NPC of another view the camera shows: it idles (it can only talk to a
    // Knight standing in its own view, which would make it `npc` above).
    let view_npc_frame = other_npc
        .filter(|(v, _)| npc.is_none_or(|n| n.source_id != v.source_id))
        .map(|(v, vroom)| {
            (
                v.position,
                clip_frame(vroom, v.clip_base as usize, sim_clock),
                vroom,
            )
        });
    // One key per slot the cache can be asked for in a frame. A tiled
    // actor or NPC contributes one key per tile, so twelve no longer
    // bounds the working set.
    let mut needed = [0u16; hk_cache::MAX_REQUESTS];
    needed[0] = match ability_frame {
        Some(i) => ability_art::KEY_BASE + i as u16,
        None => u32_at(room.frame(body_frame), 0) as u16,
    };
    let mut needed_len = if effect_frame.is_some() { 2 } else { 1 };
    if let Some(f) = effect_frame {
        needed[1] = u32_at(room.frame(f), 0) as u16;
    }
    enemy_draws.append_needed(&mut needed, &mut needed_len);
    game.shade.append_needed(&mut needed, &mut needed_len);
    if let Some(i) = ball_frame {
        assert!(
            needed_len < needed.len(),
            "Vengeful Spirit animation working set"
        );
        needed[needed_len] = ability_art::KEY_BASE + i as u16;
        needed_len += 1;
    }
    let fx_frames = if game.vitals.dead {
        [None, None, None]
    } else {
        focus_fx::frames()
    };
    let flash = if game.vitals.dead {
        None
    } else {
        focus_fx::flash()
    };
    for i in fx_frames.into_iter().flatten().chain(flash.map(|(i, _)| i)) {
        assert!(
            needed_len < needed.len(),
            "Focus effect animation working set"
        );
        needed[needed_len] = ability_art::KEY_BASE + i as u16;
        needed_len += 1;
    }
    if let Some(f) = door_frame {
        // A door pose is a rectangle of slots (host/great_door.py DOOR_MAX_AXIS).
        render::append_frame_keys(room, f, &mut needed, &mut needed_len);
    }
    if let Some((_, f, vroom)) = view_npc_frame {
        render::append_frame_keys(vroom, f, &mut needed, &mut needed_len);
    }
    // Arena gates, before anything that may yield its slots: a gate that drops
    // out of a frame would be pop-in (HK_GATE_ART_DROPPED).
    let gate_draws = battle_gate_art::prepare(
        room,
        game.region_id,
        r.scene,
        camera,
        &mut needed,
        &mut needed_len,
    );
    let drip_draws = drip::prepare(
        room,
        game.region_id,
        r.scene,
        camera,
        &mut needed,
        &mut needed_len,
    );
    if let Some((_, f)) = npc_frame {
        // An NPC frame larger than one 64x64 slot binds a rectangle of
        // them, so it can contribute several keys.
        render::append_frame_keys(room, f, &mut needed, &mut needed_len);
    }
    // Last, because the charm board is the one caller that yields when
    // the view behind it has already claimed the frame's whole cache.
    charms::append_needed(
        game.paused,
        &game.pause_menu.charms,
        &mut needed,
        &mut needed_len,
    );
    // The text panel's ornaments: like the board, they yield to the view.
    crate::dialogue_art::append_needed(
        if !game.paused {
            if crate::dialogue::panel_up(game.save_ui) {
                crate::dialogue_art::Art::Panel
            } else if crate::dialogue::prompt_up() {
                crate::dialogue_art::Art::Prompt
            } else {
                crate::dialogue_art::Art::None
            }
        } else if game.pause_menu.charms.open {
            crate::dialogue_art::Art::Board
        } else {
            crate::dialogue_art::Art::Pause
        },
        &mut needed,
        &mut needed_len,
    );
    // After the board: a prop that finds no room is left out of the frame.
    game.props
        .append_needed(camera, game.geo, &mut needed, &mut needed_len);
    // Chests and pickups draw from the Knight's view's own actor bank.
    let pickup_draws =
        game.pickups
            .append_needed(room, game.region_id, &mut needed, &mut needed_len);
    // The ability effects and then the slashes are the things that may go undrawn when the cache is
    // spoken for.
    let fx = !game.vitals.dead && !game.door.hidden() && ability_fx::request(&mut needed, &mut needed_len);
    let slashes = hit_fx::request(&mut needed, &mut needed_len);
    if early {
        render::bind_animation(room, &needed[..needed_len], true);
    } else {
        render::begin(room, &needed[..needed_len], fb.buffer_y(fb.drawing));
        // The hero vignette darkens scenery through its own colour modulation;
        // its centre is the Knight's (Vignette local y -0.58).
        #[cfg(feature = "hero-vignette")]
        render::set_vignette(
            (crate::hero_light::vignette_scene(r.scene) && !game.vitals.dead).then(|| {
                (
                    160 + (((game.player.x - camera.0) >> 8) * crate::KNIGHT_SCALE >> 20),
                    120 - (((game.player.y - 38011 - camera.1) >> 8) * crate::KNIGHT_SCALE >> 20),
                )
            }),
            camera,
        );
        fb.clear(9, 13, 20);
        if !veiled {
            prims += unsafe { spstack::sim(|| render::scenery(camera, false)) };
            prims += game.state.draw_secrets(view, camera, false);
        }
    }
    prims += enemy_draws.draw();
    prims += game.props.draw(camera);
    prims += pickup_draws.draw(room, camera);
    render::kick_ready();
    prims += game.state.draw_impacts(r, room, camera);
    if let Some(f) = door_frame {
        let away = if game.player.x < great_door::POSITION[0] { 1 } else { -1 };
        prims += great_door::draw(room, f, camera, game.door.flinch_x(away));
    }
    prims += gate_draws.draw(camera);
    prims += drip_draws.draw(camera);
    prims += game.shade.draw(camera);
    prims += geo_render::draw(game.geo, game.view, camera);
    prims += lifeblood::draw(game.life, game.region_id, camera);
    render::kick_ready();
    // Behind the Knight: the NPC sits a hair further from the camera.
    if let Some((position, f)) = npc_frame {
        prims += draw_at(room, f, position, camera);
    }
    if let Some((position, f, vroom)) = view_npc_frame {
        prims += draw_at(vroom, f, position, camera);
        unsafe {
            HK_VIEW_NPC_DRAWS = HK_VIEW_NPC_DRAWS.wrapping_add(1);
        }
    }
    if crate::modules::loaded(crate::modules::SHAMAN) {
        prims += shaman::draw(r.scene, sim_clock, camera);
    }
    shaman::prepare_veil();
    prims += blocker_roller::draw(camera);
    // The original's HeroLight sits just behind the Knight: everything drawn
    // so far brightens, the Knight and the front scenery do not.
    #[cfg(feature = "hero-light")]
    {
        if !game.vitals.dead && !game.door.hidden() {
            prims += crate::hero_light::draw_light(r.scene, game.player.x, game.player.y, camera);
        }
    }
    // InvulnerablePulse darkens the Knight toward black and back on simulation
    // time (Vitals::invulnerable_pulse), whatever the frame rate.
    let pulse = game.vitals.invulnerable_pulse();
    let tint = (128 - u32::from(pulse) * 128 / u32::from(hk_sim::PULSE_TICKS)) as u8;
    // The Dash Burst and the wings sit a hair behind the Knight (local z 0.001).
    if fx {
        prims += ability_fx::draw(&game.player, camera);
    }
    if (!game.vitals.dead || ability_frame.is_some()) && !game.door.hidden() {
        prims += match ability_frame {
            Some(i) => ability_art::draw(
                i,
                game.player.x,
                game.player.y,
                game.player.facing,
                camera,
                tint,
            ),
            None => {
                draw_frame(room, body_frame, &game.player, camera, tint);
                1
            }
        };
    }
    if slashes {
        prims += hit_fx::draw(camera);
    }
    // Lines Anim and Heal Anim sit just in front of the Knight, additive.
    for i in fx_frames.into_iter().flatten() {
        prims += ability_art::draw_additive(
            i,
            game.player.x,
            game.player.y,
            game.player.facing,
            camera,
            128,
        );
    }
    if let Some(f) = effect_frame {
        draw_frame(room, f, &game.player, camera, 128);
        prims += 1;
    }
    if let Some(i) = ball_frame {
        // `draw` mirrors for facing +1 because the Knight's atlas faces left; the
        // Fireball's own sprite faces right, so the cast direction is its mirror.
        prims += ability_art::draw(
            i,
            game.cast.ball.x,
            game.cast.ball.y,
            -game.cast.ball.facing,
            camera,
            128,
        );
    }
    // Hoisted because the HUD needs three fields of it and `params` composes
    // the shop's PlayerData, the equipped charms and the cheats on each call.
    let vitals = game.settings.cheats.params(VITAL_PARAMS);
    if !veiled {
        prims += unsafe { spstack::sim(|| render::scenery(camera, true)) };
        prims += game.state.draw_secrets(view, camera, true);
    }
    // The heal's white flash: over the world, under the HUD.
    if let Some((i, tint)) = flash {
        prims += ability_art::draw_additive(
            i,
            game.player.x,
            game.player.y,
            game.player.facing,
            camera,
            tint,
        );
    }
    // The Geo coin is a cell of the panel font now (dialogue::prepare draws it
    // with the count); it still counts as the frame's one HUD primitive.
    prims += 1;
    dialogue::prepare(
        camera,
        game.geo.wallet(),
        game.paused.then_some((&game.pause_menu, &game.settings)),
        vitals.max_health.saturating_add(game.vitals.blue_health),
        game.save_ui,
    );
    game_map::prepare(r.scene, [game.player.x, game.player.y]);
    if crate::modules::loaded(crate::modules::SHOP) {
        shop::prepare(&game.shop_screen, camera, game.paused);
    }
    title_card::prepare(game.paused);
    render::submit(
        game.vitals.health,
        vitals.max_health,
        game.vitals.soul,
        soul_cap(&game.shade, vitals.max_soul),
        game.paused,
        game.vitals.blue_health,
        game.door
            .shade()
            .max(game.exit_shade())
            .max(game.death_shade())
            .max(game.bench.fade_shade()),
    );
    prims
}

/// The half of a tick that runs whether or not the Knight is under control:
/// conversations, the shop counter, the cooked scripts, ambience and the pause
/// menu. Returns whether a screen took this tick's input.
#[inline(never)]
pub fn interact(
    game: &mut Game,
    r: &world::Region,
    npc: Option<world::Npc>,
    pad: ButtonState,
    crossing: bool,
) -> bool {
    let can_inspect = !crossing
        && !game.paused
        && !game.door.pending()
        && !game.vitals.dead
        && game.player.grounded
        // CanInteract and CanQuickMap want cState.onGround, which a hard landing leaves false.
        && !game.player.hard_landing()
        && game.vitals.can_control()
        && !game.nail.active
        && !game.focus.locks_control();
    // `Map Control`: the map button held opens the quick map when nothing else
    // owns the Knight (`CanQuickMap`, not at a bench, no panel, no shop).
    game_map::tick(
        pad.bits(),
        can_inspect
            && !game.bench.locks_control()
            && !game.shop_screen.open
            && !dialogue::open()
            && !game.save_ui.modal(),
        game.player.grounded,
    );
    let can_inspect = can_inspect && !game_map::open();
    dialogue::tick(r.scene, &game.player, PARAMS, can_inspect, pad.bits());
    // npc_control's own range trigger, cooked into the view's
    // bank: the hero body inside it is what UP answers.
    let body = [
        game.player.x - PARAMS.half_width,
        game.player.y + PARAMS.bottom,
        game.player.x + PARAMS.half_width,
        game.player.y + PARAMS.top,
    ];
    let npc_in_range = npc
        .filter(|n| {
            n.bounds[0] <= body[2]
                && n.bounds[2] >= body[0]
                && n.bounds[1] <= body[3]
                && n.bounds[3] >= body[1]
        })
        .map(|n| n.source_id);
    // Cornifer's map is paid for here, like Sly's stock below.
    let paid = dialogue::npc_tick(
        r.scene,
        npc_in_range,
        // `Inert` while the first-visit area title is up (title_card::npcs_inert).
        can_inspect && !title_card::npcs_inert(),
        pad.bits(),
        game.geo.wallet(),
    );
    if paid > 0 {
        game.geo.take(paid, geo::GEO_PARAMS);
    }
    // The Snail Shaman and the spell's orb, whose trigger takes the hero body
    // whatever he is doing (host/shaman.py).
    if shaman::tick(r.scene, body, can_inspect && !dialogue::open(), pad.bits()).wake {
        game.wake = true;
    }
    // Sly's `Shop Region`, whose trigger answers UP the way an
    // NPC's does. It takes the same hero body, so the shop and a
    // conversation cannot disagree about where the Knight is.
    let paid = shop::tick(
        &mut game.shop_screen,
        r.scene,
        body,
        can_inspect && !dialogue::open(),
        pad.bits(),
        game.geo.wallet(),
    );
    // Shiny items answer UP the way the shop and NPCs do; heart and vessel
    // pieces are taken on touch. Each is saved as it is taken.
    let inspect =
        can_inspect && !dialogue::open() && !game.shop_screen.open && pad.is_held(button::UP);
    game.pickups.hornet_defeated = persist::player(persist::HORNET_1_DEFEATED) && !crate::enemies::cloak_waiting();
    game.pickups.collect(
        body,
        inspect,
        persist::player(persist::FALSE_KNIGHT_DEFEATED),
        crate::enemies::arena_piece_ready(),
        |p| take_pickup(r.scene, p),
    );
    // A shiny is the Knight's at the end of his kneel; a hit before then gives it back.
    if let Some(p) = game.pickups.kneel_tick(!game.vitals.can_control()) {
        take_pickup(r.scene, p);
    }
    if paid > 0 {
        game.geo.take(paid, geo::GEO_PARAMS);
    }
    // The cooked FSM programs. Their trigger volumes take the
    // same hero body the NPC range does, so a script and an NPC
    // standing in the same place agree on where the Knight is.
    script::tick(r.scene, body);
    ambience::tick();
    let start = pad.is_held(button::START);
    let mut ui_consumed = game.paused;
    if start && !game.start_held && !dialogue::open() && !game.shop_screen.open {
        game.paused = !game.paused;
        ui_consumed = true;
        if game.paused {
            game.pause_menu.enter(pad.bits());
        }
    }
    game.start_held = start;
    // The inventory button (Select) opens the charm board, the port's inventory,
    // and closes whatever pause screen is up.
    let inventory = pad.is_held(input::bind::INVENTORY);
    if inventory && !game.inventory_held && !dialogue::open() && !game.shop_screen.open {
        game.paused = !game.paused;
        ui_consumed = true;
        if game.paused {
            game.pause_menu.enter_inventory(pad.bits());
        } else {
            game.player.was_jump = pad.is_held(button::CROSS);
        }
    }
    game.inventory_held = inventory;
    if game.paused {
        let before = (
            game.settings.sfx,
            game.settings.ambience,
            game.settings.music,
        );
        let previous_cheats = game.settings.cheats;
        let screen = (
            game.pause_menu.row,
            game.pause_menu.controls,
            game.pause_menu.cheats,
        );
        if game.pause_menu.step(
            pad.bits(),
            &mut game.settings.sfx,
            &mut game.settings.ambience,
            &mut game.settings.music,
            &mut game.settings.cheats,
        ) {
            game.paused = false;
            game.player.was_jump = pad.is_held(button::CROSS);
            audio::ui_confirm();
        }
        // A sub-screen opened or closed is the source's submit/cancel; a row
        // move is its select.
        if game.paused && (screen.1, screen.2) != (game.pause_menu.controls, game.pause_menu.cheats)
        {
            audio::ui_confirm();
        } else if game.paused
            && screen
                != (
                    game.pause_menu.row,
                    game.pause_menu.controls,
                    game.pause_menu.cheats,
                )
        {
            audio::ui_select();
        } else if game.paused
            && (before
                != (
                    game.settings.sfx,
                    game.settings.ambience,
                    game.settings.music,
                )
                || previous_cheats != game.settings.cheats)
        {
            audio::ui_slider();
        }
        game.settings.cheats.apply(
            previous_cheats,
            &mut game.vitals,
            VITAL_PARAMS,
            game.pause_menu.action,
        );
        if before
            != (
                game.settings.sfx,
                game.settings.ambience,
                game.settings.music,
            )
        {
            audio::set_volume(game.settings.sfx);
            focus_audio::set_volume(game.settings.sfx);
            geo_audio::set_volume(game.settings.sfx);
            runner_audio::set_volume(game.settings.sfx);
            ambience::set_volume(game.settings.ambience);
            music::set_volume(game.settings.music);
        }
    }
    if game.paused
        || dialogue::open()
        || game.shop_screen.open
        || (game.door.pending() && !game.door.entering())
        || game.vitals.dead
        || !game.vitals.can_control()
        || game.focus.locks_control()
    {
        audio::stop_footsteps();
    }
    game.settings
        .cheats
        .maintain(&mut game.vitals, VITAL_PARAMS);
    cheats::publish(game.settings.cheats, VITAL_PARAMS);
    // StartSoulLimiter lowers maxMP itself while a Shade is owed.
    game.vitals.soul = game.vitals.soul.min(soul_cap(
        &game.shade,
        game.settings.cheats.params(VITAL_PARAMS).max_soul,
    ));
    unsafe {
        HK_PAUSED = u32::from(game.paused);
        HK_PAUSE_CONTROLS = u32::from(game.paused && game.pause_menu.controls);
        HK_SFX_LEVEL = game.settings.sfx as u32;
        HK_AMBIENCE_LEVEL = game.settings.ambience as u32;
    }
    ui_consumed
}

/// Whether a menu, panel or prompt reads the pad this tick: the pause menu,
/// a conversation, lore tablet, map, the shop and the bench's save prompt,
/// including the hold each keeps until its closing press is released. The
/// Knight then reads the pad as polled too (input::hero_latency is for play).
pub fn menu_reads_pad(game: &Game) -> bool {
    game.paused
        || dialogue::open()
        || dialogue::consumes_actions()
        || game.shop_screen.consumes_actions()
        || game.save_ui.modal()
}

/// The Cheats page's Reset session row (it was SELECT): the development reset, back to
/// the new-game spawn with every counter the route tables read cleared.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn debug_reset(game: &mut Game, target: &mut Option<usize>) {
    game.geo.reset(geo::GEO_PARAMS);
    unsafe {
        HK_GEO_LOST = 0;
    }
    game.life.reset();
    game.door.reset();
    battle_gates::reset();
    music::boss(false);
    title_card::reset();
    blocker_roller::reset();
    dialogue::reset();
    script::reset();
    persist::reset();
    game.state = world::State::new();
    game.reveals = reveal_masks::State::new();
    game.enemies = enemies::EnemyWorld::new();
    game.vitals = game.settings.cheats.new_vitals(VITAL_PARAMS);
    // The pool is resident rather than a field of the world state, so replacing
    // that state no longer empties it. Same clearing, now said out loud.
    *world::particles::pool() = world::particles::Pool::new();
    audio::reset_movement();
    game.player = Player::spawn(SPAWN.0, SPAWN.1);
    game.safe = (SPAWN.0, SPAWN.1, 0);
    game.safe_facing = 1;
    game.nail = Nail::new();
    game.nail_response = hk_sim::NailResponse::new();
    game.focus.interrupt();
    focus_audio::interrupt();
    game.attacks = 0;
    *target = Some(0);
    game.paused = false;
    unsafe {
        HK_BREAK_COUNT = 0;
        HK_DEATHS = 0;
        HK_DEATH_HOLD_TICKS = 0;
        HK_STREAM_ART_LATE = 0;
        hit_fx::HK_HIT_FX_BURSTS = 0;
        bench::HK_BENCH_WAKES = 0;
        bench::HK_GROUND_WAKES = 0;
        HK_ENEMY_HITS = 0;
        HK_ENEMY_KILLS = 0;
        HK_HAZARD_RESPAWNS = 0;
        HK_FOCUS_STARTED = 0;
        HK_FOCUS_COMPLETED = 0;
        HK_FOCUS_HEALED = 0;
        HK_FOCUS_DRAINED = 0;
        HK_FOCUS_REFUNDED = 0;
    }
    // The boss counters and its live mirror: the actor is gone with the enemy
    // world above, so nothing would otherwise clear what the last fight left.
    unsafe {
        enemies::HK_FK_TRIGGERED = 0;
        enemies::HK_FK_DROPPED = 0;
        enemies::HK_FK_STAGGERS = 0;
        enemies::HK_FK_CONVERSIONS = 0;
        enemies::HK_FK_DEATHS = 0;
        enemies::HK_FK_HP = 0;
        enemies::HK_FK_HEAD_HP = 0;
        enemies::HK_FK_ACTIVE = 0;
        enemies::HK_FK_EXPOSED = 0;
        enemies::HK_FK_STUNNED = 0;
        enemies::HK_FK_ARENA = 0;
        enemies::HK_FK_ACTIVATED = 0;
        enemies::HK_FK_BARRELS = 0;
        enemies::HK_FK_BARRELS_BROKEN = 0;
    }
}

/// The Save Game record for a respawn at `seat`: a bench the player chose to
/// save at, or the spell's wake point, where the source saves by itself.
fn save_record(
    game: &Game,
    scene: usize,
    seat: [i32; 2],
    facing: i32,
    region: usize,
) -> save::Save {
    let (owned, equipped, notches, can_overcharm) = charms::record();
    let (script_fields, script_field_fnv) = script::record();
    let (shop_slots, shop_counters) = shop::record();
    // The walls and rocks join the SceneData list now; the cocoon, the secrets
    // and the arena wrote theirs as it happened.
    persist::snapshot(&game.state, game.geo, scene);
    let world = persist::store();
    save::Save {
        scene: scene as u32,
        seat,
        facing,
        region: region as u32,
        geo: game.geo.wallet(),
        door_hits: game.door.hits(),
        shade: game.shade.record(),
        sequence: 0,
        charms_owned: owned,
        charms_equipped: equipped,
        charm_notches: notches,
        can_overcharm,
        npc_conversations: dialogue::met_bits(),
        script_fields,
        script_field_fnv,
        shop_slots,
        shop_counters,
        player_bools: world.player,
        player_levels: world.levels,
        version: 5,
    }
}
/// `Check Fall`'s `Black` and `Set Respawns`: the Knight lies at the wake point
/// facing right with `AddMPCharge(100)`, the respawn is set there and the game
/// stays in memory. The spell itself was set by shaman.rs. Saving requires the
/// player's explicit bench Save confirmation.
fn wake(game: &mut Game, scene: usize, cache: &disc::Cache) {
    let (x, y) = (shaman::WAKE_AT[0], shaman::WAKE_AT[1] + ONE / 4);
    audio::reset_movement();
    game.player = Player::spawn(x, y);
    game.player.facing = 1;
    game.nail = Nail::new();
    game.nail_response = hk_sim::NailResponse::new();
    game.focus.interrupt();
    focus_audio::interrupt();
    game.vitals
        .add_soul(game.settings.cheats.params(VITAL_PARAMS), shaman::WAKE_SOUL);
    if let Some(id) = cache.locate(scene, x, y) {
        game.respawn = Some((scene, [x, y], 1, id));
        game.safe = (x, y, id);
        game.safe_facing = 1;
    }
}
/// What the camera reads off the Knight: position, body box (the trigger
/// collider), facing and the cState flags CameraTarget tests.
pub fn camera_hero(game: &Game) -> camera::Hero {
    let p = &game.player;
    camera::Hero {
        x: p.x,
        y: p.y,
        body: [
            p.x - PARAMS.half_width,
            p.y + PARAMS.bottom,
            p.x + PARAMS.half_width,
            p.y + PARAMS.top,
        ],
        facing: p.facing,
        dashing: p.dash_left > 0,
        super_dashing: matches!(p.super_dash, hk_sim::SuperDash::Travelling(_)),
        falling: !p.grounded && p.vy < 0 && p.dash_left == 0,
        transitioning: game.door.entering(),
    }
}
/// UPDATE: one simulation tick that the player's input drives. Death and the
/// hazard respawn take the tick instead when they are owed one.
#[inline(never)]
pub fn simulate(
    game: &mut Game,
    r: &world::Region,
    room: &Room,
    cache: &disc::Cache,
    meta_region: usize,
    pad: ButtonState,
    target: &mut Option<usize>,
    spatial_target: &mut bool,
) {
    battle_gates::tick();
    hit_fx::tick();
    ability_fx::tick();
    drip::tick(r.scene);
    // What the enemies see is the camera as the previous tick left it: in the
    // original their FSMs run in Update and read the camera transform its
    // LateUpdate moved the frame before. Ours moves in this tick's camera step
    // below, after the hero, before the enemies, so read it here. A simulation
    // fact, not the drawn view's: it must not depend on how often frames draw.
    let camera = game.camera.position();
    if let Some(mut e) = game.exit.take() {
        e.age += 1;
        if e.dir != 0 {
            // HeroController exit: run on through a side gate at RUN_SPEED.
            let p = &mut game.player;
            p.x += e.dir * (great_door::ENTRY.speed / 60);
            p.facing = e.dir;
            if p.animation != 1 {
                p.animation = 1;
                p.animation_tick = 0;
            } else {
                p.animation_tick = p.animation_tick.wrapping_add(1);
            }
        }
        if e.age >= EXIT_LIVE_TICKS {
            if e.age < EXIT_FADE_TICKS {
                let shade = ((e.age as u32 * 255) / EXIT_FADE_TICKS as u32) as u8;
                crate::exit_fade::arm(shade, (EXIT_FADE_TICKS - e.age) as u32);
            }
            take_gate(game, r, e.gate, e.door_exit, target);
        } else {
            game.exit = Some(e);
        }
        return;
    }
    game.gate_cooldown = game.gate_cooldown.saturating_sub(1);
    game.scene_ticks = game.scene_ticks.saturating_add(1);
    if core::mem::take(&mut game.wake) {
        wake(game, r.scene, cache);
    }
    game.state.tick();
    game.state.tick_debris(r, room);
    presentation::checkpoint();
    if game.vitals.needs_death_respawn() && game.death_hold < DEATH_HOLD_TICKS {
        // The camera has faded out and `PlayerDead` waits 0.8 s before `ReadyForRespawn`.
        game.death_hold += 1;
        unsafe {
            HK_DEATH_HOLD_TICKS += 1;
        }
    } else if game.vitals.needs_death_respawn() {
        game.death_hold = 0;
        hit_fx::clear();
        ability_fx::clear();
        // The screen is already black; the load carries on from there (exit_fade.rs).
        exit_fade::arm(255, 1);
        // A fight lost ends its XA song; the arena resets for the next try.
        music::boss(false);
        // Hero Death Anim: Remove Geo moves the wallet into the pool the
        // Shade carries and Set Shade records where and how strong it is.
        // The source also saves here; this port never writes the card on
        // its own, so the Shade reaches it at the next bench the player
        // chooses to save at.
        let params = game.settings.cheats.params(VITAL_PARAMS);
        game.shade.record_death(
            r.scene,
            [game.player.x, game.player.y],
            shade::death_health(params.max_health, params.nail_damage),
            game.geo.wallet(),
        );
        unsafe {
            HK_GEO_LOST = HK_GEO_LOST.saturating_add(game.geo.death());
        }
        game.geo.leave_scene(r.scene, geo::GEO_PARAMS);
        game.geo.reset_enemies(r.scene);
        // `GameManager.PlayerDead` resets every semi-persistent item: the soul
        // totems refill and the enemies killed this life are back.
        persist::reset_semi_persistent();
        game.life.reset();
        persist::restore_cocoon(game.life);
        game.props.leave();
        game.pickups.leave();
        if !game.door.opened() {
            game.door.reset();
        }
        blocker_roller::reset();
        dialogue::cancel();
        game.state.reset_scene(r.scene);
        game.reveals.reset_scene(r.scene);
        game.enemies.reset_scene(r.scene);
        runner_audio::reset();
        game.vitals = game.settings.cheats.new_vitals(VITAL_PARAMS);
        // Respawn at the last bench (standing at its seat) or the new-game spawn.
        let (spawn, facing, slot) = game
            .respawn
            .map_or(((SPAWN.0, SPAWN.1), 1, 0), |(_, seat, facing, slot)| {
                ((seat[0], seat[1]), facing, slot)
            });
        audio::reset_movement();
        game.player = Player::spawn(spawn.0, spawn.1);
        game.player.facing = facing;
        game.safe = (spawn.0, spawn.1, slot);
        game.safe_facing = facing;
        game.bench.reset();
        // A bench respawn wakes asleep on the bench (Bench Control's `Init Resting`); the new-game
        // spawn gets up from the ground (`HeroController.Respawn`'s `Wake Up Ground`).
        game.bench.respawn(game.respawn.is_some());
        game.nail = Nail::new();
        game.nail_response = hk_sim::NailResponse::new();
        game.focus.interrupt();
        focus_audio::interrupt();
        *target = Some(slot);
        // The load that starts now reads what the scene carries: the respawn clips go with it.
        #[cfg(not(test))]
        game.carry_streamed_art(r.scene);
    } else if game.vitals.hazard_pending {
        dialogue::cancel();
        unsafe {
            HK_HAZARD_RESPAWNS += 1;
        }
        audio::reset_movement();
        game.player = Player::spawn(game.safe.0, game.safe.1);
        game.player.facing = game.safe_facing;
        game.nail = Nail::new();
        game.nail_response = hk_sim::NailResponse::new();
        game.focus.interrupt();
        focus_audio::interrupt();
        game.vitals
            .finish_hazard_respawn(game.settings.cheats.params(VITAL_PARAMS));
        *target = Some(game.safe.2);
    } else if !game.vitals.dead {
        let door_exit = game.door.tick(r.scene);
        let read_consumes = dialogue::consumes_actions() || game.shop_screen.consumes_actions();
        // Source Spell Control: the cast button waits Button Down Time
        // for a release. Let go inside that and it casts; keep holding
        // and the Focus starts instead. One button, two moves.
        let cast_button =
            pad.is_held(input::bind::CAST) && !read_consumes && game.vitals.can_control();
        let cast_blocked = !game.vitals.can_control() || game.door.pending() || read_consumes;
        game.cast.has_fireball =
            game.settings.cheats.spell || persist::store().levels[persist::FIREBALL_LEVEL] > 0;
        let was_antic = game.cast.phase == hk_sim::CastPhase::Antic;
        game.cast.tick(
            FIREBALL_PARAMS,
            FIREBALL_ANTIC_TICKS,
            FIREBALL_CAST_TICKS,
            cast_button,
            &mut game.vitals.soul,
            &game.player,
            cast_blocked,
        );
        // Quick Cast: the same cast from its own button, with no hold to wait out.
        game.cast.quick(
            FIREBALL_PARAMS,
            pad.is_held(input::bind::QUICK_CAST) && !read_consumes && game.vitals.can_control(),
            &mut game.vitals.soul,
            &game.player,
            cast_blocked,
        );
        // `Fireball Top`'s `Cast Left`/`Cast Right` play the cast as the ball leaves.
        if was_antic && game.cast.phase == hk_sim::CastPhase::Recoil {
            ability_sound(audio::FIREBALL);
            ability_fx::cast(&game.player);
        }
        let focus_events = game.focus.step(
            FOCUS_PARAMS,
            game.settings.cheats.params(VITAL_PARAMS),
            hk_sim::FocusInput {
                held: cast_button
                    && game.cast.held_past_tap(FIREBALL_PARAMS)
                    && !game.cast.locks_control(),
                // CanFocus wants cState.onGround, which a hard landing leaves false.
                grounded: game.player.grounded && !game.player.hard_landing(),
                can_start: game.vitals.can_control()
                    && !game.door.pending()
                    && (!game.nail.active || game.nail.age >= FOCUS_PARAMS.attack_recovery_ticks),
            },
            &mut game.vitals,
        );
        focus_audio::tick(&game.focus, focus_events);
        if focus_fx::tick(
            game.focus.lines_active(),
            focus_events.completed,
            game.vitals.soul as u16,
            FOCUS_PARAMS.cost as u16,
            game.vitals.health >= game.settings.cheats.params(VITAL_PARAMS).max_health,
        ) {
            ability_sound(audio::FOCUS_READY);
            world::particles::pool().spawn_break_at(
                r.scene,
                focus_fx::MOTES_OWNER,
                [game.player.x, game.player.y],
            );
        }
        // Dust L and Dust R at the Knight's feet, in world space, while Focus runs.
        if focus_fx::dust_due() {
            world::particles::pool().spawn_break_at(
                r.scene,
                focus_fx::DUST_OWNER,
                [game.player.x, game.player.y],
            );
        }
        game.settings
            .cheats
            .maintain(&mut game.vitals, VITAL_PARAMS);
        unsafe {
            HK_FOCUS_STARTED += u32::from(focus_events.started);
            HK_FOCUS_COMPLETED += u32::from(focus_events.completed);
            HK_FOCUS_HEALED += u32::from(focus_events.healed);
            HK_FOCUS_DRAINED += u32::from(focus_events.drained);
            HK_FOCUS_REFUNDED += u32::from(focus_events.refunded);
        }
        if focus_events.started {
            game.nail = Nail::new();
            game.nail_response = hk_sim::NailResponse::new();
            game.player.vy = 0;
            game.player.jumping = false;
        }
        // Copied out because the closure below would otherwise hold a borrow of
        // the state struct across every mutation in the rest of the tick.
        let prev_pad = game.prev_pad;
        let pressed = |b: u16| pad.is_held(b) && !prev_pad.is_held(b);
        // Mothwing Cloak. Nothing in the admitted scenes grants it, so
        // the cheat is the only source until the pickups exist.
        // PlayerData alongside: nothing writes those bits yet, and when the
        // pickups do, a save carries them.
        // The Knight's own one-shots follow the state edges of this tick, the
        // dash and Crystal Heart inputs below included.
        let before = game.player;
        game.player.has_dash = game.settings.cheats.dash || persist::player(persist::HAS_DASH);
        game.player.has_walljump =
            game.settings.cheats.claw || persist::player(persist::HAS_WALLJUMP);
        game.player.has_double_jump =
            game.settings.cheats.wings || persist::player(persist::HAS_DOUBLE_JUMP);
        game.player.has_super_dash =
            game.settings.cheats.heart || persist::player(persist::HAS_SUPER_DASH);
        game.player.has_shade_cloak =
            game.settings.cheats.cloak || persist::player(persist::HAS_SHADOW_DASH);
        game.player.super_dash_input(
            PARAMS,
            pad.is_held(input::bind::SUPER_DASH)
                && game.vitals.can_control()
                && !locked_for_dash(&game.focus, &game.door, &game.bench)
                && !game.player.hard_landing(),
        );
        game.player.dash_input(
            PARAMS,
            pad.is_held(input::bind::DASH)
                && game.vitals.can_control()
                && !locked_for_dash(&game.focus, &game.door, &game.bench)
                && !game.player.hard_landing(),
        );
        // The seated prompt owns input while it is up, so the bench
        // neither reads a leave press nor starts another rest.
        let asking = game.save_ui.modal();
        let edges = save_ui::Edges {
            up: pressed(button::UP),
            down: pressed(button::DOWN),
            cross: pressed(button::CROSS),
            circle: pressed(button::CIRCLE),
        };
        if game.save_ui.step(edges).is_some() {
            let stand = [game.bench.seat()[0], game.player.y];
            game.save_requested = Some(save_record(
                game,
                r.scene,
                stand,
                game.player.facing,
                game.region_id,
            ));
        }
        let bench_events = {
            let hero_body = [
                game.player.x - PARAMS.half_width,
                game.player.y + PARAMS.bottom,
                game.player.x + PARAMS.half_width,
                game.player.y + PARAMS.top,
            ];
            let free = !(game.focus.locks_control()
                || dialogue::open()
                || game.door.pending()
                || read_consumes
                || asking)
                && game.vitals.can_control()
                && !game.player.hard_landing()
                && !game.nail.active;
            let leave = !asking
                && [
                    button::CROSS,
                    button::SQUARE,
                    button::LEFT,
                    button::RIGHT,
                    button::UP,
                    button::DOWN,
                ]
                .iter()
                .any(|&b| pressed(b));
            let grounded = game.player.grounded;
            game.bench.tick(
                r,
                hero_body,
                &mut game.player,
                grounded,
                free,
                !asking && pressed(button::UP),
                leave,
            )
        };
        if bench_events.sat {
            scene_sfx::play(scene_sfx::BENCH_REST);
        }
        if bench_events.rest {
            // Resting resets every semi-persistent item: the soul totems refill
            // and the enemies killed since are back.
            persist::reset_semi_persistent();
            // Rest Burst: HERO REVIVED and the respawn marker, then ask.
            // The source saves here without asking; this port never
            // writes the card unless the player says so.
            game.vitals
                .heal(game.settings.cheats.params(VITAL_PARAMS), u16::MAX);
            // Sitting keeps the Knight's floor y; the source seat y is the sprite's, and a
            // body spawned there starts inside the floor and falls through it.
            let stand = [game.bench.seat()[0], game.player.y];
            game.respawn = Some((r.scene, stand, game.player.facing, game.region_id));
            // The bench updates the map with the quill (`UpdateGameMap`).
            game_map::update_game_map();
            game.save_ui.ask();
        }
        let locked = game.focus.locks_control()
            || dialogue::open()
            || game.shop_screen.open
            || game.door.pending()
            || game.bench.locks_control()
            || game.save_ui.modal()
            || game.cast.locks_control()
            || game.pickups.kneeling()
            // `Update` skips LookForInput in `hard_landing`, and CanJump, CanAttack and CanDash
            // refuse the state: no steering, jump, dash or strike until the timer runs out.
            || game.player.hard_landing();
        // Take Control: the Dream Nail owns the Knight from Start to End.
        game.dream.has_dream_nail =
            game.settings.cheats.dream || persist::player(persist::HAS_DREAM_NAIL);
        game.dream.tick(
            DREAM_NAIL_PARAMS,
            pad.is_held(input::bind::DREAM_NAIL) && game.vitals.can_control(),
            &game.player,
            locked,
        );
        let locked = locked || game.dream.locks_control();
        let dir = if game.door.entering() {
            game.door.forced_direction()
        } else if locked {
            0
        } else {
            i32::from(pad.is_held(button::RIGHT)) - i32::from(pad.is_held(button::LEFT))
        };
        let was_grounded = game.player.grounded;
        let hard_locked = game.player.hard_landing();
        let jump = !locked && !read_consumes && pad.is_held(button::CROSS);
        let could_jump = was_grounded
            && !game.player.was_jump
            && jump
            && game.vitals.can_control()
            && game.nail_response.bounce_left == 0;
        if game.door.drop_starts() {
            game.player.vy = -12 * ONE;
        }
        if game.door.apply_entry(&mut game.player) {
            // Source entry owns position/animation while input is locked.
        } else if game.bench.locks_control() {
            // Seated or getting off: the bench owns position and animation.
            game.player.grounded = true;
            game.player.jumping = false;
            game.player.was_jump = pad.is_held(button::CROSS);
        } else if game.pickups.kneeling() && game.vitals.recoil_velocity(VITAL_PARAMS).is_none() {
            // Kneeling to a shiny: `Hero Down` stops the Knight where he stands.
            game.player.grounded = true;
            game.player.jumping = false;
            game.player.was_jump = pad.is_held(button::CROSS);
        } else if let Some((vx, vy)) = game.vitals.recoil_velocity(VITAL_PARAMS) {
            // A hit takes the Knight out of `hard_landing` (his state becomes no_input).
            game.player.end_hard_landing();
            let mut p = PARAMS;
            p.speed = vx.abs();
            p.gravity = 0;
            game.player.vy = vy;
            game.player.jumping = false;
            game.player.step(
                p,
                vx.signum(),
                false,
                room.counts[5],
                &game.state.edge_reader(r, room),
            );
            game.player.facing = -game.vitals.recoil_direction;
        } else {
            if could_jump {
                audio::jump();
            }
            // A bottom entry's drop carries the source's entry speed, not RUN_SPEED.
            let params = if game.door.dropping() {
                hk_sim::Params {
                    speed: great_door::ENTER_HOR,
                    ..PARAMS
                }
            } else {
                PARAMS
            };
            // The terrain sweep spills heavily; its frames go on the scratchpad.
            unsafe {
                spstack::sim(|| {
                    game.nail_response.step(
                        NAIL_RESPONSE_PARAMS,
                        params,
                        &mut game.player,
                        dir,
                        jump,
                        room.counts[5],
                        &game.state.edge_reader(r, room),
                    )
                })
            };
            game.door.landed(game.player.grounded);
            // A jump held through the lock is not a new press when control returns.
            if hard_locked {
                game.player.was_jump = pad.is_held(button::CROSS);
            }
        }
        hero_sounds(&before, &game.player);
        ability_fx::on_edges(&before, &game.player);
        great_door::probe(
            game.door.entering(),
            game.player.grounded,
            game.player.y,
            game.player.jumping,
        );
        // FallCheck, ShouldHardLand and DoHardLanding. A gate entry's drop is `no_input` in the
        // source, which lands through FinishedEnteringScene, never a hard landing.
        let hard = game
            .player
            .hard_landing_tick(hard_landing::RULE, was_grounded, !game.door.entering());
        if hard {
            hard_landing::landed(r.scene, &mut game.player);
        }
        hard_landing::tick(&game.player);
        audio::movement_tick(
            was_grounded,
            game.player.grounded,
            (game.door.entry_footsteps() || (dir != 0 && !locked))
                && game.vitals.can_control()
                && game.vitals.recoil_velocity(VITAL_PARAMS).is_none(),
            hard,
        );
        let vertical = i32::from(pad.is_held(button::UP)) - i32::from(pad.is_held(button::DOWN));
        let attacked = game.nail.tick(
            ATTACK_PARAMS,
            pad.is_held(button::SQUARE) && game.vitals.can_control() && !locked && !read_consumes,
            vertical,
            &mut game.player,
        );
        if attacked {
            game.attacks = game.attacks.wrapping_add(1);
            game.geo.begin_attack();
            audio::nail_kind(game.nail.kind);
        }
        // HeroController.Bounce returns early while cState.shroomBouncing,
        // so an ordinary pogo cannot cut a shroom's rise short.
        if game.vitals.can_control()
            && !strike_shroom(r, &game.nail, &mut game.player)
            && !game.player.shroom_bouncing
        {
            enemies::pogo_contact(
                r,
                &game.state,
                &mut game.player,
                &game.nail,
                ATTACK_PARAMS,
                NAIL_POLYGONS,
                &mut game.nail_response,
            );
        }
        let hero_body = [
            game.player.x - PARAMS.half_width,
            game.player.y + PARAMS.bottom,
            game.player.x + PARAMS.half_width,
            game.player.y + PARAMS.top,
        ];
        let strike = game.state.strike(
            r,
            &game.nail,
            ATTACK_PARAMS,
            NAIL_POLYGONS,
            &game.player,
            game.attacks,
            hero_body,
        );
        unsafe {
            HK_BREAK_COUNT = HK_BREAK_COUNT.saturating_add(strike.broken as u32);
        }
        // `Breakable.Break` ends by sending `EnemyKillShake` to the camera on
        // every break, a hidden wall's own `AverageShake` (secret_feedback, which
        // outranks it) included.
        let ordinary_breaks = strike.broken - u16::from(strike.secret.is_some_and(|e| e.broke));
        if ordinary_breaks > 0 {
            camera::request(camera::Shake::Kill);
            unsafe {
                HK_BREAK_SHAKES = HK_BREAK_SHAKES.saturating_add(ordinary_breaks as u32);
            }
        }
        if let Some(e) = strike.secret {
            secret_feedback(game, r, e);
        }
        // Vengeful Spirit breaks a hidden wall outright (`Spell Destroy`).
        let spell_break = game
            .cast
            .ball_bounds(FIREBALL_PARAMS)
            .and_then(|ball| game.state.spell_strike(r, ball));
        if let Some(e) = spell_break {
            secret_feedback(game, r, e);
        }
        if strike.door_sounds > 0 {
            audio::door();
        }
        // Barrels, poles and wagons break with their own clip where the scene bank has room.
        if let Some(event) = strike.last_broken.and_then(scene_sfx::break_sound) {
            if scene_sfx::resident(event) {
                scene_sfx::play(event);
            }
        }
        let mining = strike_geo(game.geo, r.scene, &game.nail, &game.player);
        if mining.hits > 0 {
            geo_audio::hit();
        }
        // `Geo Rock`'s `Hit`: Strike Nail R, Dust Hit Med and three to five chips at the rock; the
        // last hit adds `EnemyKillShake` and the rest of the chips.
        if let Some((at, bounds)) = mining.at {
            game.state
                .hit_impact(r, game.attacks as usize, bounds, game.player.x);
            fx::raise(r.scene, fx::HIT_DUST, at);
            fx::raise(r.scene, fx::HIT_CHIPS, at);
            if mining.depleted > 0 {
                camera::request(camera::Shake::Kill);
                fx::raise(r.scene, fx::HIT_CHIPS, at);
                fx::raise(r.scene, fx::HIT_CHIPS, at);
            }
        }
        if mining.depleted > 0 {
            geo_audio::break_rock();
            geo_render::apply(game.geo, &mut game.state, game.region_id, game.view);
        }
        // Soul totems: `Hit` flings 8 or 9 soul orbs of AddMPCharge(2) and
        // shakes the camera; the SOUL lands on the hit rather than in flight.
        soul_totems::tick();
        title_card::tick([
            game.player.x - PARAMS.half_width,
            game.player.y + PARAMS.bottom,
            game.player.x + PARAMS.half_width,
            game.player.y + PARAMS.top,
        ]);
        let totem_soul = strike_totems(r.scene, &game.nail, &game.player, game.attacks);
        if totem_soul > 0 {
            game.vitals.add_soul(VITAL_PARAMS, totem_soul);
            scene_sfx::play(scene_sfx::SOUL_TOTEM_SLASH);
            camera::request(camera::Shake::Kill);
        }
        // A break or depleted rock invalidated the edge table; rebuild it now so the
        // rest of this tick (enemies, coins, Lifeblood) does not fall back to the
        // per-query breakable scan, which cost several VBlanks with three coins.
        if strike.broken > 0 || mining.depleted > 0 || spell_break.is_some() {
            game.state.refresh_edges(r, room);
        }
        presentation::checkpoint();
        let blue = game.life.tick(
            r.scene,
            game.player.x,
            r.collision_bounds,
            room.counts[5],
            game.state.edge_reader(r, room),
        );
        game.vitals.add_blue_health(blue);
        // lifeblood::apply sets the shared scripted-edge scratch rather than
        // appending to it, so the door's exclusions and the gates' are gone for
        // the rest of this tick and only come back on the next render. Both are
        // re-applied here in render's own order, which is the order the other
        // three call sites already use.
        //
        // Unreachable today: no catalogue slot carries both a struck cocoon and
        // an arena gate, and none carries a cocoon and the Great Door. It is
        // written anyway because the symptom is a wall that exists for one
        // simulation tick, which is close to undiagnosable from a bug report.
        let life_strike = strike_lifeblood(game.life, r.scene, &game.nail, &game.player);
        // A struck Health Scuttler plays `ScuttlerControl.deathSound1` and
        // `deathSound2`. The second is `enemy_damage`, the resident enemy-hit
        // clip; the first is not identified here, so it stays silent. The
        // cocoon's own `health_cocoon_break` plays from the world bank.
        if life_strike.hit_bugs > 0 {
            audio::enemy_hit();
        }
        // `ScuttlerControl.Hit`: `EnemyKillShake` and the blue blood.
        if let Some(at) = life_strike.hit_at {
            camera::request(camera::Shake::Kill);
            fx::raise(r.scene, fx::SCUTTLER_BLOOD, at);
        }
        if life_strike.opened {
            audio::cocoon_break();
            // `HealthCocoon.OnTriggerEnter2D`: the nail's slash and strike at its top, the
            // `EnemyKillShake`, the cap knocked away from the Knight, and the flung spatters
            // and globs (the Scuttlers are `lifeblood`'s own).
            let at = lifeblood::origin();
            let top = at[1] + hk_sim::ONE * 4 / 5;
            let quarter = hk_sim::ONE / 4;
            game.state.hit_impact(
                r,
                game.attacks as usize,
                [at[0] - quarter, top - quarter, at[0] + quarter, top + quarter],
                game.player.x,
            );
            camera::request(camera::Shake::Kill);
            fx::raise(r.scene, fx::COCOON, at);
            fx::raise(
                r.scene,
                if at[0] >= game.player.x {
                    fx::COCOON_CAP_RIGHT
                } else {
                    fx::COCOON_CAP_LEFT
                },
                at,
            );
            persist::set(persist::Kind::Cocoon, r.scene, 0, 1);
            lifeblood::apply(game.life, &mut game.state, game.region_id, game.view);
            great_door::apply(&game.door, &mut game.state, game.region_id);
            battle_gates::apply(&mut game.state, game.region_id, game.view);
            blocker_terrain::apply(&mut game.state, game.region_id);
        }
        let door_hit = strike_great_door(&mut game.door, r.scene, &game.nail, &game.player);
        if door_hit.hit {
            audio::great_door_hit();
            // The source's `Hit` state: a nail impact where the nail met the
            // door, `EnemyKillShake` (`AverageShake` when the hit changed its
            // pose), and the sound. Its debris and dust are particle prefabs
            // the cook does not carry.
            let (x, y) = (great_door::BOUNDS[0], game.player.y + hk_sim::ONE / 2);
            let quarter = hk_sim::ONE / 4;
            game.state.hit_impact(
                r,
                game.door.hits() as usize,
                [x - quarter, y - quarter, x + quarter, y + quarter],
                game.player.x,
            );
            camera::request(if door_hit.staged {
                camera::Shake::Average
            } else {
                camera::Shake::Kill
            });
            // `Hit`: Door_dust over the door, Dust Hit Med and three to five chips at the nail,
            // and ten to twelve over the door's height; `Break 1` and `Break 2` add the rubble
            // falling from the lintel and twelve more.
            // Hits 4, 8 and 13 add `breakable_wall_death` to the hit's own sound.
            if (door_hit.staged || door_hit.opened) && scene_sfx::resident(scene_sfx::BREAKABLE_WALL_DEATH) {
                scene_sfx::play(scene_sfx::BREAKABLE_WALL_DEATH);
            }
            fx::raise(r.scene, fx::DOOR_DUST, [0, 0]);
            fx::raise(r.scene, fx::HIT_DUST, [x, y]);
            fx::raise(r.scene, fx::HIT_CHIPS, [x, y]);
            fx::raise(r.scene, fx::DOOR_CHIPS, [0, 0]);
            if door_hit.staged {
                fx::raise(r.scene, fx::DOOR_RUBBLE, [0, 0]);
                fx::raise(r.scene, fx::DOOR_CHIPS, [0, 0]);
            }
        }
        if door_hit.opened {
            great_door::apply(&game.door, &mut game.state, game.region_id);
            dialogue::cancel();
            game.focus.interrupt();
            focus_audio::interrupt();
        }
        {
            let hero_body = [
                game.player.x - PARAMS.half_width,
                game.player.y + PARAMS.bottom,
                game.player.x + PARAMS.half_width,
                game.player.y + PARAMS.top,
            ];
            let props_strike =
                strike_props(game.props, r.scene, &game.nail, &game.player, hero_body);
            // StalactiteControl: `breakSound` for a break, `hitSound` for a bat
            // (the scene voice keeps the later), or the enemy-hit clip where the
            // scene bank has no room for them, as for a freed grub; `Strike Nail R`
            // where it was struck (the Slash Impact stands in), and a break's
            // dust and flung rocks.
            let clip = if props_strike.broken > 0 {
                scene_sfx::STALACTITE_DEATH
            } else {
                scene_sfx::STALACTITE_IMPACT
            };
            let struck = props_strike.broken | props_strike.batted;
            if struck > 0 && scene_sfx::resident(clip) {
                scene_sfx::play(clip);
            } else if struck > 0 {
                audio::enemy_hit();
            }
            // `Bottle Control`'s Shatter: `EnemyKillShake` and the glass.
            if let Some(jar) = props_strike.jar {
                camera::request(camera::Shake::Kill);
                fx::raise(r.scene, fx::JAR_GLASS, jar);
                if scene_sfx::resident(scene_sfx::CRYSTAL_BREAK_1) {
                    scene_sfx::play(scene_sfx::CRYSTAL_BREAK_1);
                } else {
                    audio::enemy_hit();
                }
            }
            if let Some(b) = props_strike.impact {
                game.state
                    .hit_impact(r, game.attacks as usize, b, game.player.x);
            }
            if let Some((slot, index, at)) = props_strike.shattered {
                props::stalactite_dust(r.scene, slot, index, 0, at);
                game.props.fling(game.geo, at);
            }
            if let Some((slot, index, at)) = props_strike.embedded {
                // The fallen stalactite is a `Breakable`: its `Break` sends `EnemyKillShake`.
                camera::request(camera::Shake::Kill);
                props::stalactite_dust(r.scene, slot, index, 2, at);
            }
            // `Chest Control`'s `Open`: saved at once, and `Spawn Items` flings
            // its Geo through the coin pool.
            if let Some(chest) = strike_chests(game.pickups, &game.nail, &game.player, hero_body) {
                open_chest(game.geo, r.scene, chest);
            }
            let touched = game.props.tick(
                hero_body,
                [game.player.x, game.player.y],
                room.counts[5],
                game.state.edge_reader(r, room),
            );
            // `Grub Control`'s Leave: `grub_free`.
            if touched.left.is_some() && scene_sfx::resident(scene_sfx::GRUB_FREE_1) {
                scene_sfx::play(scene_sfx::GRUB_FREE_1);
            }
            if touched.fell > 0 && scene_sfx::resident(scene_sfx::STALACTITE_BREAK) {
                scene_sfx::play(scene_sfx::STALACTITE_BREAK);
            }
            if let Some((slot, index, at)) = touched.landed {
                props::stalactite_dust(r.scene, slot, index, 1, at);
            }
            if let Some(direction) = touched.hurt {
                apply_hurt(
                    &game.settings.cheats,
                    &mut game.vitals,
                    &game.player,
                    &mut game.nail,
                    &mut game.nail_response,
                    &mut game.focus,
                    1,
                    direction,
                    false,
                );
            }
        }
        {
            // HeroController's look state: it runs while `hero_state` is idle,
            // and attacking, jumping, dashing or losing control resets it.
            let control = !locked
                && game.vitals.can_control()
                && game.vitals.recoil_velocity(VITAL_PARAMS).is_none();
            let idle = control
                && game.player.grounded
                && dir == 0
                && game.player.dash_left == 0
                && !game.door.entering();
            let dashed = before.dash_left == 0 && game.player.dash_left > 0;
            let look = camera::LookInput {
                idle,
                up: pad.is_held(button::UP),
                down: pad.is_held(button::DOWN),
                moving: dir != 0,
                reset: attacked || could_jump || dashed || !control,
            };
            let hero = camera_hero(game);
            game.camera.tick(r, &hero, look, game.scene_ticks, &|id| {
                game.state.broken(id)
            });
            // HeroAnimationController: the look clips belong to a standing Knight only.
            game.look.tick(
                game.camera.look_anim(),
                idle && !game.nail.active && game.focus.animation().is_none(),
            );
        }
        if game.shade.present_here() {
            let hero_body = [
                game.player.x - PARAMS.half_width,
                game.player.y + PARAMS.bottom,
                game.player.x + PARAMS.half_width,
                game.player.y + PARAMS.top,
            ];
            if strike_shade(
                &mut game.shade,
                &game.nail,
                &game.player,
                game.settings.cheats.params(VITAL_PARAMS).nail_damage,
            ) {
                audio::enemy_hit();
            }
            let shade_position = game
                .shade
                .position()
                .unwrap_or([game.player.x, game.player.y]);
            let shade_events = game.shade.tick(
                [game.player.x, game.player.y],
                hero_body,
                room.counts[5],
                game.state.edge_reader(r, room),
            );
            if shade_events.touched {
                let direction = if game.player.x < shade_position[0] {
                    -1
                } else {
                    1
                };
                apply_hurt(
                    &game.settings.cheats,
                    &mut game.vitals,
                    &game.player,
                    &mut game.nail,
                    &mut game.nail_response,
                    &mut game.focus,
                    1,
                    direction,
                    false,
                );
            }
            if shade_events.returned_geo > 0 {
                game.geo
                    .add_quietly(shade_events.returned_geo, geo::GEO_PARAMS);
                geo_audio::pickup(1);
            }
        }
        // The Elder Baldur's spat Roller: three nail hits of SOUL, a mask to touch.
        {
            let hero_body = [
                game.player.x - PARAMS.half_width,
                game.player.y + PARAMS.bottom,
                game.player.x + PARAMS.half_width,
                game.player.y + PARAMS.top,
            ];
            let mut polygon = [[0; 2]; 16];
            let mut sides = 0;
            if game.nail.hitting(ATTACK_PARAMS) {
                let source = NAIL_POLYGONS[game.nail.kind as usize];
                sides = source.len();
                for (dst, p) in polygon.iter_mut().zip(source) {
                    *dst = [
                        game.player.x - p[0] * game.player.facing,
                        game.player.y + p[1],
                    ];
                }
            }
            let params = game.settings.cheats.params(VITAL_PARAMS);
            let roller = blocker_roller::tick(
                r.scene,
                [game.player.x, game.player.y],
                hero_body,
                (sides > 0).then_some(&polygon[..sides]),
                params.nail_damage,
                room.counts[5],
                game.state.edge_reader(r, room),
            );
            if roller.hits > 0 {
                game.vitals.gain_soul_on_nail_hit(params);
                audio::enemy_hit();
            }
            if roller.killed {
                camera::request(camera::Shake::Kill);
                audio::enemy_death();
            }
            if let Some(direction) = roller.touched {
                apply_hurt(
                    &game.settings.cheats,
                    &mut game.vitals,
                    &game.player,
                    &mut game.nail,
                    &mut game.nail_response,
                    &mut game.focus,
                    1,
                    direction,
                    false,
                );
            }
        }
        let enemy_events = game.enemies.tick(
            r,
            room,
            &mut game.state,
            &mut game.player,
            &mut game.vitals,
            &game.nail,
            &game.dream,
            game.cast
                .ball_bounds(FIREBALL_PARAMS)
                .map(|b| (b, FIREBALL_PARAMS.damage)),
            &mut game.nail_response,
            ATTACK_PARAMS,
            NAIL_POLYGONS,
            game.settings.cheats,
            [camera.0, camera.1, -2496922],
            runner_event,
            // Asked once per near off-view actor per tick; the bank scans spill heavily.
            |scene, x, y, located| unsafe {
                spstack::sim(|| {
                    let i = cache.locate_cached(scene, x, y, located)?;
                    Some((world::resident(i)?, cache.room_for(i)?))
                })
            },
        );
        // A gate, the floor or a Blocker's block moved during the enemies'
        // tick: rebuild the scripted terrain now, in render's order, so the
        // rest of this tick (the ball, coins, the next tick's hero) collides
        // with it whatever the frame rate. The source destroys a dead
        // Blocker's block with it and moves its gates in the same frame.
        if world::take_scripted_terrain_changed() {
            lifeblood::apply(game.life, &mut game.state, game.region_id, game.view);
            great_door::apply(&game.door, &mut game.state, game.region_id);
            battle_gates::apply(&mut game.state, game.region_id, game.view);
            blocker_terrain::apply(&mut game.state, game.region_id);
            game.state.refresh_edges(r, room);
        }
        // The wall stop comes after the enemies have seen the ball: in the
        // source the fireball's damage trigger and its terrain contact land in
        // the same physics step, so a ball that reaches a wall still hits what
        // it overlaps there. A Blocker's hurtbox is its Terrain Block's own
        // footprint, so stopping first made every Elder Baldur unkillable.
        game.cast.stop_at_wall(
            FIREBALL_PARAMS,
            room.counts[5],
            game.state.edge_reader(r, room),
        );
        ability_fx::emit(r.scene, &game.player, &game.cast.ball);
        game.enemies.take_geo_deaths(r.scene, |source, x, y| {
            let Some(e) = world::geo_enemy(r.scene, source) else {
                return false;
            };
            game.geo.spawn_enemy(
                r.scene,
                source,
                [x, y],
                geo::EnemySpec {
                    source_id: e.source_id,
                    scene: r.scene as u8,
                    drops: e.drops,
                    fling: geo::FLING[e.mega as usize],
                    offset: e.offset,
                },
            )
        });
        // Gruz Mother's burster `Geo`: FlingObjectsFromGlobalPool 50 Geo Small,
        // 15 to 30 a second between 80 and 100 degrees, 0.75 either way.
        if let Some((scene, origin)) = enemies::take_burster_geo() {
            game.geo.spawn_chest(
                scene,
                0x6a5f_6d00,
                origin,
                geo::Fling {
                    speed: [15 * ONE, 30 * ONE],
                    angle: [80 * ONE, 100 * ONE],
                    spread: [49152, 49152],
                },
                [hk_sim::gruz_mother::BURSTER_GEO, 0, 0],
            );
        }
        let h = geo::HERO_BOX;
        let collected = game.geo.tick(
            r.scene,
            geo::GEO_PARAMS,
            [
                game.player.x + h[0],
                game.player.y + h[1],
                game.player.x + h[2],
                game.player.y + h[3],
            ],
            r.collision_bounds,
            room.counts[5],
            game.state.edge_reader(r, room),
        );
        if collected.collected > 0 {
            geo_audio::pickup(collected.last_value);
        }
        presentation::checkpoint();
        unsafe {
            HK_ENEMY_HITS += enemy_events.hits as u32;
            HK_ENEMY_KILLS += enemy_events.kills as u32;
            HK_SOUL = game.vitals.soul as u32;
        }
        if enemy_events.dream_soul > 0 {
            game.vitals.add_soul(VITAL_PARAMS, enemy_events.dream_soul);
        }
        if enemy_events.hits > 0 {
            audio::enemy_hit();
            // The Slash Impact at the nail, facing away from the Knight.
            if let Some(b) = nail_bounds(&game.nail, &game.player) {
                game.state
                    .hit_impact(r, game.attacks as usize, b, game.player.x);
            }
        }
        // `EnemyDeathEffects.EmitInfectedEffects` ends in
        // ShakeCameraIfVisible("EnemyKillShake"). The False Knight requests
        // its own, higher-priority shakes, which this one never displaces.
        if enemy_events.kills > 0 {
            camera::request(camera::Shake::Kill);
            audio::enemy_death();
        }
        respond_to_hurt(
            enemy_events.hurt,
            [game.player.x, game.player.y],
            &mut game.nail,
            &mut game.nail_response,
            &mut game.focus,
        );
        let bank_region = cache
            .world_metadata()
            .region(meta_region)
            .expect("admitted world metadata covers the selected region");
        if let Some((damage, hazard, direction)) =
            game.state
                .hazard_contact(bank_region, &game.player, PARAMS, |id| {
                    game.props.owns_hazard(id)
                })
        {
            apply_hurt(
                &game.settings.cheats,
                &mut game.vitals,
                &game.player,
                &mut game.nail,
                &mut game.nail_response,
                &mut game.focus,
                damage,
                direction,
                hazard,
            );
        }
        if let Some((spawn, facing)) = game.state.checkpoint(bank_region, &game.player, PARAMS) {
            if let Some(id) = cache.locate(r.scene, spawn[0], spawn[1]) {
                game.safe = (spawn[0], spawn[1], id);
                game.safe_facing = facing;
            }
        }
        if game.gate_cooldown == 0
            && (door_exit || !game.door.pending())
            && !game.vitals.dead
            && !game.vitals.hazard_pending
        {
            let gate = if door_exit {
                Some(
                    world::gates(r.scene)
                        .find(|g| g.target_scene == great_door::TARGET_SCENE)
                        .expect("Great Door exit gate"),
                )
            } else {
                // A door answers UP against the hero body; every other
                // gate keeps the point test it has always used.
                let hero_body = [
                    game.player.x - PARAMS.half_width,
                    game.player.y + PARAMS.bottom,
                    game.player.x + PARAMS.half_width,
                    game.player.y + PARAMS.top,
                ];
                game.state.gate(
                    r.scene,
                    &game.player,
                    hero_body,
                    pressed(button::UP) && !read_consumes,
                    game.vitals.recoil_ticks != 0,
                    game.scene_ticks,
                )
            };
            // A tour takes only its own listed gates, never one the Knight stands in.
            let gate = if crate::gate_tour::active() {
                crate::gate_tour::due(r.scene).and_then(|(to, region)| {
                    world::gates(r.scene)
                        .find(|g| g.target_scene == to && g.target_region == region)
                })
            } else {
                gate
            };
            if let Some(g) = gate {
                crate::gate_tour::taken();
                crate::gate_probe::trigger(r.scene, g.target_scene);
                // Read ahead from now: the fade covers the drive's work.
                crate::disc::prefetch_hint(crate::disc::scene_index(g.target_region));
                if door_exit {
                    // The Great Door's own FADE OUT INSTANT: no fade here.
                    take_gate(game, r, g, door_exit, target);
                } else {
                    let dir = match g.side {
                        1 => -1,
                        2 => 1,
                        _ => 0,
                    };
                    game.exit = Some(Exit {
                        gate: g,
                        door_exit,
                        age: 0,
                        dir,
                    });
                    audio::stop_footsteps();
                }
            }
        }
        if target.is_none()
            && !game.vitals.dead
            && !game.vitals.hazard_pending
            && !world::contains(r.bounds, game.player.x, game.player.y)
        {
            if let Some(id) = cache.locate(r.scene, game.player.x, game.player.y) {
                *target = Some(id);
                *spatial_target = true;
            } else {
                apply_hurt(
                    &game.settings.cheats,
                    &mut game.vitals,
                    &game.player,
                    &mut game.nail,
                    &mut game.nail_response,
                    &mut game.focus,
                    1,
                    0,
                    true,
                );
            }
        }
    }
    let fired = game.reveals.tick(&game.player, PARAMS, |controller, body| {
        world::reveal_trigger_reaches(r.scene, controller, body)
    });
    if fired != 0 {
        // A one-way controller firing takes the `unmasker` sound branch only
        // when its placement sets `Play Sound`.
        if game.reveals.chimes(fired) && scene_sfx::resident(scene_sfx::SECRET_DISCOVERED) {
            scene_sfx::play(scene_sfx::SECRET_DISCOVERED);
        }
        persist::record_secrets(r.scene, game.reveals.revealed());
    }
}
/// A hidden wall's or cracked floor's answer to an accepted hit, or to its
/// break (host/secret_breaks.py has the states each comes from).
#[cfg_attr(not(test), optimize(size))]
fn secret_feedback(game: &mut Game, r: &world::Region, e: world::SecretEvent) {
    use crate::secret_breaks::{FAMILY_WALL, FAMILY_WALL_TK2D};
    let wall = matches!(e.family, FAMILY_WALL | FAMILY_WALL_TK2D);
    // `Strike Nail R` at the wall's pivot, or two units under a floor's
    // owner; the port draws its nail Slash Impact there.
    let o = e.origin;
    let half = hk_sim::ONE / 2;
    game.state.hit_impact(
        r,
        e.id,
        [o[0] - half, o[1] - half, o[0] + half, o[1] + half],
        game.player.x,
    );
    game.state.secret_particles(r, &e, game.player.facing);
    if e.broke {
        // Both families' `Break` send `AverageShake` and play two clips at
        // once; the second rides the shared voice so the first is not cut.
        camera::request(camera::Shake::Average);
        // A scene whose bank had no room for a clip (host/scene_sfx.py prints
        // the refusals; the False Knight's room has none left) skips it.
        if scene_sfx::resident(scene_sfx::BREAKABLE_WALL_DEATH) {
            scene_sfx::play(scene_sfx::BREAKABLE_WALL_DEATH);
        }
        let second = if wall {
            scene_sfx::SECRET_DISCOVERED
        } else {
            scene_sfx::BARREL_DEATH_1
        };
        if scene_sfx::resident(second) {
            scene_sfx::play_shared(second);
        }
        game.reveals.fire_driver(e.id % world::BREAKABLES_PER_SCENE);
    } else if wall {
        // `AudioPlayRandom`: breakable_wall_hit_1 or _2 at 1:1, pitch .85..1.15.
        crate::secret_breaks::wall_hit_sound();
    } else {
        // break_floor `Hit 1` / `Hit 2`: barrel_death_1 and `EnemyKillShake`.
        if scene_sfx::resident(scene_sfx::BARREL_DEATH_1) {
            scene_sfx::play(scene_sfx::BARREL_DEATH_1);
        }
        camera::request(camera::Shake::Kill);
    }
}
/// Leave the scene through gate `g` (after its exit fade): the source scene's
/// state resets, the Knight spawns at the destination entry and `target`
/// names the destination region, which the main loop loads.
#[cfg_attr(not(test), optimize(size))]
fn take_gate(
    game: &mut Game,
    r: &world::Region,
    g: world::Gate,
    door_exit: bool,
    target: &mut Option<usize>,
) {
    let side_entry = (g.scene == 0 && g.target_scene == great_door::TARGET_SCENE)
        || (g.scene == great_door::TARGET_SCENE && g.target_scene == 0);
    let (spawn, id) = if side_entry {
        let entry = if g.target_scene == 0 {
            &great_door::RETURN_ENTRY
        } else {
            &great_door::ENTRY
        };
        (entry.spawn, entry.region)
    } else {
        (g.spawn, g.target_region)
    };
    hit_fx::clear();
    ability_fx::clear();
    game.geo.leave_scene(r.scene, geo::GEO_PARAMS);
    game.geo.reset_enemies(r.scene);
    game.vitals.add_blue_health(game.life.leave_scene());
    let facing = game.player.facing;
    blocker_roller::reset();
    dialogue::cancel();
    game.state.reset_scene(r.scene);
    game.reveals.leave_scene();
    game.enemies.reset_scene(r.scene);
    runner_audio::reset();
    audio::reset_movement();
    game.player = Player::spawn(spawn[0], spawn[1]);
    // The Great Door pair walks in through its audited entries; every
    // other side gate uses the same source sequence toward the scene
    // interior (a right exit enters at the destination's left gate).
    if side_entry {
        if door_exit {
            game.door.begin_entry(g.target_scene);
        } else {
            game.door
                .begin_gate_entry(g.target_scene, if g.target_scene == 0 { -1 } else { 1 });
        }
        game.player.grounded = true;
    } else if g.side == 1 || g.side == 2 {
        game.door
            .begin_gate_entry(g.target_scene, if g.side == 2 { 1 } else { -1 });
        game.player.facing = if g.side == 2 { 1 } else { -1 };
        game.player.grounded = true;
    } else if g.side == 3 {
        // Up through a top gate is in through the destination's
        // bottom one, which rises out of it rather than dropping.
        game.player.y += great_door::BOTTOM_ENTRY_RAISE;
        game.player.facing = facing;
        game.door.begin_bottom_entry(g.target_scene, facing);
    } else {
        game.player.vy = g.entry_vy;
    }
    game.nail = Nail::new();
    game.nail_response = hk_sim::NailResponse::new();
    game.focus.interrupt();
    focus_audio::interrupt();
    *target = Some(id);
    game.safe = (game.player.x, game.player.y, id);
    game.safe_facing = game.player.facing;
    game.gate_cooldown = 90;
}

/// The Knight's one-shots that follow a state edge of one player step
/// (HeroController and the Superdash FSM, resources.assets):
/// `HeroDash` plays the dash, or `shadowDashClip` when the dash is a shade
/// dash; `DoWallJump` the wall jump; `DoDoubleJump` `doubleJumpClip`; `Update`
/// `mantisClawClip` once on the first frame of a wall slide; the Superdash
/// FSM's charge, `Ground/Wall Charged`, the burst, `Hit Wall` and `Air Cancel`.
fn hero_sounds(before: &Player, after: &Player) {
    if before.dash_left == 0 && after.dash_left > 0 {
        audio::hero_extra(if after.shadow_dashing {
            audio::SHADE_DASH
        } else {
            audio::DASH
        });
    }
    if !before.wall_locked && after.wall_locked {
        audio::hero_extra(audio::WALLJUMP);
    }
    if !before.double_jumping && after.double_jumping {
        audio::hero_extra(audio::WINGS);
    }
    if !before.wall_sliding && after.wall_sliding {
        audio::hero_extra(audio::CLAW);
    }
    use hk_sim::SuperDash as S;
    match (before.super_dash, after.super_dash) {
        (S::Off, S::Charging(_)) => ability_sound(audio::SUPER_CHARGE),
        (S::Charging(_), S::Ready) => ability_sound(audio::SUPER_READY),
        (S::Charging(_) | S::Ready, S::Travelling(_)) => ability_sound(audio::SUPER_BURST),
        (S::Travelling(_), S::Recovering(_)) => ability_sound(audio::SUPER_WALL),
        (S::Travelling(_), S::Off) if !after.grounded => ability_sound(audio::SUPER_BRAKE),
        _ => {}
    }
}

/// One of the Knight's ability one-shots from the Focus bank, once it is in SPU.
fn ability_sound(index: usize) {
    if focus_audio::ready() {
        audio::ability(index, focus_audio::ABILITY_SAMPLES[index]);
    }
}
