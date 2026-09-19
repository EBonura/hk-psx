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
    pub geo: &'static mut geo::World,
    pub life: &'static mut lifeblood::World,
    // Screens and the input edges they read.
    pub settings: menu::Settings,
    pub pause_menu: pause::State,
    pub shop_screen: shop::Screen,
    pub paused: bool,
    pub start_held: bool,
    pub prev_pad: ButtonState,
    // Row 0 is Yes, row 1 is No, while the seated save prompt is open.
    pub save_prompt: Option<u8>,
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
}

/// RENDER: the visual frame for the simulation state as it stands.
///
/// `camera` is the frame's camera rather than one read here, because the
/// catch-up ticks that follow take the same value: what the enemies see has to
/// be where the view was drawn.
#[inline(never)]
pub fn render(game: &mut Game, r: &world::Region, room: &Room, fb: &mut FrameBuffer,
              npc: Option<world::Npc>, camera: (i32, i32), sim_clock: u32, frame: u32) -> u32 {
    game.state.apply(r);
    apply_reveal_masks(&game.reveals,r);
    geo_render::apply(game.geo,&mut game.state,game.region_id);
    lifeblood::apply(game.life,&mut game.state,game.region_id);
    great_door::apply(&game.door,&mut game.state,game.region_id);
    let door_frame=game.door.frame(game.region_id,camera);
    // The bench owns the body first, then an ability, then the room clip.
    let pose=if game.bench.animation().is_some() {None} else {ability_pose(&game.player,&game.dream,&game.cast)};
    let body_frame=if let Some((clip,age))=game.bench.animation() {clip_frame(room,clip,age)} else {knight_frame(room,&game.player,&game.focus)};
    let effect_frame=nail_frame(room,&game.nail);
    let enemy_draws=game.enemies.prepare_draws(r,room,camera);
    let ability_frame=pose.map(|(clip,age)|ability_art::frame_index(clip,age));
    // Vengeful Spirit's projectile carries its own frame and key.
    let ball_frame=game.cast.ball_bounds(FIREBALL_PARAMS).map(|_|
        ability_art::frame_index(ability_art::BALL,game.cast.ball.life as u32));
    // The view's NPC, whose clips ride in this view's own room bank.
    let npc_frame=npc.map(|npc|{
        // Conversation Control reaches Talk Right through Hero Is Right
        // and Talk Left through Hero Is Left, because the NPC's own
        // localScale.x is positive; Idle is what it stands in.
        let slot=if dialogue::npc_speaking()==Some(npc.source_id) {
            if game.player.x>=npc.position[0] {2} else {1}
        } else {0};
        (npc.position,clip_frame(room,npc.clip_base as usize+slot,sim_clock))
    });
    // One key per slot the cache can be asked for in a frame. A tiled
    // actor or NPC contributes one key per tile, so twelve no longer
    // bounds the working set.
    let mut needed=[0u16;hk_cache::MAX_REQUESTS];
    needed[0]=match ability_frame {
        Some(i)=>ability_art::KEY_BASE+i as u16,
        None=>u32_at(room.frame(body_frame),0) as u16,
    };
    let mut needed_len=if effect_frame.is_some(){2}else{1};
    if let Some(f)=effect_frame {needed[1]=u32_at(room.frame(f),0) as u16;}
    enemy_draws.append_needed(&mut needed,&mut needed_len);
    game.shade.append_needed(&mut needed,&mut needed_len);
    if let Some(i)=ball_frame {
        assert!(needed_len<needed.len(),"Vengeful Spirit animation working set");
        needed[needed_len]=ability_art::KEY_BASE+i as u16;needed_len+=1;
    }
    if let Some(f)=door_frame {
        assert!(needed_len<needed.len(),"Great Door animation working set");
        needed[needed_len]=u32_at(room.frame(f),0)as u16;needed_len+=1;
    }
    if let Some((_,f))=npc_frame {
        // An NPC frame larger than one 64x64 slot binds a rectangle of
        // them, so it can contribute several keys.
        render::append_frame_keys(room,f,&mut needed,&mut needed_len);
    }
    // Last, because the charm board is the one caller that yields when
    // the view behind it has already claimed the frame's whole cache.
    charms::append_needed(game.paused,&game.pause_menu.charms,&mut needed,&mut needed_len);
    render::begin(room,&needed[..needed_len],fb.buffer_y(fb.drawing));
    fb.clear(9,13,20);let mut prims=render::scenery(camera,false);prims+=enemy_draws.draw();render::kick_ready();prims+=game.state.draw_impacts(r,room,camera);
    if let Some(f)=door_frame {prims+=great_door::draw(room,f,camera);}
    prims+=game.shade.draw(camera);
    prims+=geo_render::draw(game.geo,game.region_id,camera);
    prims+=lifeblood::draw(game.life,game.region_id,camera);render::kick_ready();
    // Behind the Knight: the NPC sits a hair further from the camera.
    if let Some((position,f))=npc_frame {prims+=draw_at(room,f,position,camera);}
    if !game.vitals.dead && !game.door.hidden() && (game.vitals.invulnerable_ticks==0 || frame%4<2) {
        prims+=match ability_frame {
            Some(i)=>ability_art::draw(i,game.player.x,game.player.y,game.player.facing,camera),
            None=>{draw_frame(room,body_frame,&game.player,camera);1}
        };
    }
    if let Some(f)=effect_frame {draw_frame(room,f,&game.player,camera);prims+=1;}
    if let Some(i)=ball_frame {
        prims+=ability_art::draw(i,game.cast.ball.x,game.cast.ball.y,game.cast.ball.facing,camera);
    }
    prims+=render::scenery(camera,true);dialogue::prepare(camera,game.geo.wallet(),game.paused.then_some((&game.pause_menu,&game.settings)),game.settings.cheats.params(VITAL_PARAMS).max_health.saturating_add(game.vitals.blue_health),game.save_prompt);shop::prepare(&game.shop_screen,camera,game.paused);render::submit(game.vitals.health,game.settings.cheats.params(VITAL_PARAMS).max_health,game.vitals.soul,soul_cap(&game.shade),game.paused,game.vitals.blue_health,game.door.shade());
    prims
}

/// The half of a tick that runs whether or not the Knight is under control:
/// conversations, the shop counter, the cooked scripts, ambience and the pause
/// menu. Returns whether a screen took this tick's input.
#[inline(never)]
pub fn interact(game: &mut Game, r: &world::Region, npc: Option<world::Npc>,
                pad: ButtonState, crossing: bool) -> bool {
    let can_inspect=!crossing && !game.paused && !game.door.pending() && !game.vitals.dead && game.player.grounded && game.vitals.can_control() && !game.nail.active && !game.focus.locks_control();
    dialogue::tick(r.scene,&game.player,PARAMS,can_inspect,pad.bits());
    // npc_control's own range trigger, cooked into the view's
    // bank: the hero body inside it is what UP answers.
    let body=[game.player.x-PARAMS.half_width,game.player.y+PARAMS.bottom,game.player.x+PARAMS.half_width,game.player.y+PARAMS.top];
    let npc_in_range=npc.filter(|n|
        n.bounds[0]<=body[2] && n.bounds[2]>=body[0] && n.bounds[1]<=body[3] && n.bounds[3]>=body[1]
    ).map(|n|n.source_id);
    dialogue::npc_tick(r.scene,npc_in_range,can_inspect,pad.bits());
    // Sly's `Shop Region`, whose trigger answers UP the way an
    // NPC's does. It takes the same hero body, so the shop and a
    // conversation cannot disagree about where the Knight is.
    let paid=shop::tick(&mut game.shop_screen,r.scene,body,can_inspect&&!dialogue::open(),pad.bits(),game.geo.wallet());
    if paid>0 {game.geo.take(paid,geo::GEO_PARAMS);}
    // The cooked FSM programs. Their trigger volumes take the
    // same hero body the NPC range does, so a script and an NPC
    // standing in the same place agree on where the Knight is.
    script::tick(r.scene,body);
    ambience::tick();
    let start=pad.is_held(button::START);
    let mut ui_consumed=game.paused;
    if start&&!game.start_held && !dialogue::open() && !game.shop_screen.open {
        game.paused=!game.paused;ui_consumed=true;
        if game.paused {game.pause_menu.enter(pad.bits());}
    }
    game.start_held=start;
    if game.paused {
        let before=(game.settings.sfx,game.settings.ambience,game.settings.music);let previous_cheats=game.settings.cheats;
        if game.pause_menu.step(pad.bits(),&mut game.settings.sfx,&mut game.settings.ambience,&mut game.settings.music,&mut game.settings.cheats) {
            game.paused=false;game.player.was_jump=pad.is_held(button::CROSS);
        }
        game.settings.cheats.apply(previous_cheats,&mut game.vitals,VITAL_PARAMS,game.pause_menu.action);
        if before!=(game.settings.sfx,game.settings.ambience,game.settings.music) {audio::set_volume(game.settings.sfx);focus_audio::set_volume(game.settings.sfx);geo_audio::set_volume(game.settings.sfx);runner_audio::set_volume(game.settings.sfx);ambience::set_volume(game.settings.ambience);music::set_volume(game.settings.music);}
    }
    if game.paused || dialogue::open() || game.shop_screen.open || (game.door.pending() && !game.door.entering()) || game.vitals.dead || !game.vitals.can_control() || game.focus.locks_control() {audio::stop_footsteps();}
    game.settings.cheats.maintain(&mut game.vitals,VITAL_PARAMS);cheats::publish(game.settings.cheats,VITAL_PARAMS);
    // StartSoulLimiter lowers maxMP itself while a Shade is owed.
    game.vitals.soul=game.vitals.soul.min(soul_cap(&game.shade));
    unsafe {HK_PAUSED=u32::from(game.paused);HK_PAUSE_CONTROLS=u32::from(game.paused&&game.pause_menu.controls);
        HK_SFX_LEVEL=game.settings.sfx as u32;HK_AMBIENCE_LEVEL=game.settings.ambience as u32;}
    ui_consumed
}

/// SELECT: the development reset, back to the new-game spawn with every
/// counter the route tables read cleared.
#[inline(never)]
pub fn debug_reset(game: &mut Game, target: &mut Option<usize>) {
    game.geo.reset(geo::GEO_PARAMS);unsafe {HK_GEO_LOST=0;}
    game.life.reset();
    game.door.reset();
    dialogue::reset();script::reset();game.state=world::State::new();game.reveals=reveal_masks::State::new();game.enemies=enemies::EnemyWorld::new();game.vitals=game.settings.cheats.new_vitals(VITAL_PARAMS);
    audio::reset_movement();game.player=Player::spawn(SPAWN.0,SPAWN.1);game.safe=(SPAWN.0,SPAWN.1,0);game.safe_facing=1;
    game.nail=Nail::new();game.nail_response=hk_sim::NailResponse::new();game.focus.interrupt();focus_audio::interrupt();game.attacks=0;*target=Some(0);game.paused=false;
    unsafe {HK_BREAK_COUNT=0;HK_DEATHS=0;HK_ENEMY_HITS=0;HK_ENEMY_KILLS=0;HK_HAZARD_RESPAWNS=0;HK_FOCUS_STARTED=0;HK_FOCUS_COMPLETED=0;HK_FOCUS_HEALED=0;HK_FOCUS_DRAINED=0;HK_FOCUS_REFUNDED=0;}
}

/// UPDATE: one simulation tick that the player's input drives. Death and the
/// hazard respawn take the tick instead when they are owed one.
#[inline(never)]
pub fn simulate(game: &mut Game, r: &world::Region, room: &Room, cache: &disc::Cache,
                meta_region: usize, pad: ButtonState, camera: (i32, i32),
                target: &mut Option<usize>, spatial_target: &mut bool) {
    game.gate_cooldown=game.gate_cooldown.saturating_sub(1);
    game.scene_ticks=game.scene_ticks.saturating_add(1);
    game.state.tick();
    game.state.tick_debris(r, room);
    presentation::checkpoint();
    if game.vitals.needs_death_respawn() {
        // Hero Death Anim: Remove Geo moves the wallet into the pool the
        // Shade carries and Set Shade records where and how strong it is.
        // The source also saves here; this port never writes the card on
        // its own, so the Shade reaches it at the next bench the player
        // chooses to save at.
        let params=game.settings.cheats.params(VITAL_PARAMS);
        game.shade.record_death(r.scene,[game.player.x,game.player.y],
            shade::death_health(params.max_health,params.nail_damage),game.geo.wallet());
        unsafe {HK_GEO_LOST=HK_GEO_LOST.saturating_add(game.geo.death());}
        game.geo.leave_scene(r.scene,geo::GEO_PARAMS);game.geo.reset_enemies(r.scene);
        game.life.reset();
        if !game.door.opened() {game.door.reset();}
        dialogue::cancel();game.state.reset_scene(r.scene);game.reveals.reset_scene(r.scene);game.enemies.reset_scene(r.scene);runner_audio::reset();game.vitals=game.settings.cheats.new_vitals(VITAL_PARAMS);
        // Respawn at the last bench (standing at its seat) or the new-game spawn.
        let (spawn,facing,slot)=game.respawn.map_or(((SPAWN.0,SPAWN.1),1,0),|(_,seat,facing,slot)|((seat[0],seat[1]),facing,slot));
        audio::reset_movement();game.player=Player::spawn(spawn.0,spawn.1);game.player.facing=facing;game.safe=(spawn.0,spawn.1,slot);game.safe_facing=facing;game.bench.reset();
        game.nail=Nail::new();game.nail_response=hk_sim::NailResponse::new();game.focus.interrupt();focus_audio::interrupt();*target=Some(slot);
    } else if game.vitals.hazard_pending {
        dialogue::cancel();
        unsafe {HK_HAZARD_RESPAWNS+=1;}
        audio::reset_movement();game.player=Player::spawn(game.safe.0,game.safe.1);game.player.facing=game.safe_facing;game.nail=Nail::new();game.nail_response=hk_sim::NailResponse::new();game.focus.interrupt();focus_audio::interrupt();
        game.vitals.finish_hazard_respawn(game.settings.cheats.params(VITAL_PARAMS));*target=Some(game.safe.2);
    } else if !game.vitals.dead {
        let door_exit=game.door.tick(r.scene);
        let read_consumes=dialogue::consumes_actions()||game.shop_screen.consumes_actions();
        // Source Spell Control: the cast button waits Button Down Time
        // for a release. Let go inside that and it casts; keep holding
        // and the Focus starts instead. One button, two moves.
        let cast_button=pad.is_held(button::CIRCLE)&&!read_consumes&&game.vitals.can_control();
        let cast_blocked=!game.vitals.can_control()||game.door.pending()||read_consumes;
        game.cast.has_fireball=game.settings.cheats.spell;
        game.cast.tick(FIREBALL_PARAMS,FIREBALL_ANTIC_TICKS,FIREBALL_CAST_TICKS,
            cast_button,&mut game.vitals.soul,&game.player,cast_blocked);
        game.cast.stop_at_wall(FIREBALL_PARAMS,room.counts[5],game.state.edge_reader(r,room));
        let focus_events=game.focus.step(FOCUS_PARAMS,game.settings.cheats.params(VITAL_PARAMS),hk_sim::FocusInput {
            held:cast_button&&game.cast.held_past_tap(FIREBALL_PARAMS)&&!game.cast.locks_control(),grounded:game.player.grounded,
            can_start:game.vitals.can_control() && !game.door.pending() && (!game.nail.active || game.nail.age>=FOCUS_PARAMS.attack_recovery_ticks),
        },&mut game.vitals);
        focus_audio::tick(&game.focus,focus_events);
        game.settings.cheats.maintain(&mut game.vitals,VITAL_PARAMS);
        unsafe {
            HK_FOCUS_STARTED+=u32::from(focus_events.started);HK_FOCUS_COMPLETED+=u32::from(focus_events.completed);
            HK_FOCUS_HEALED+=u32::from(focus_events.healed);HK_FOCUS_DRAINED+=u32::from(focus_events.drained);
            HK_FOCUS_REFUNDED+=u32::from(focus_events.refunded);
        }
        if focus_events.started {
            game.nail=Nail::new();game.nail_response=hk_sim::NailResponse::new();game.player.vy=0;game.player.jumping=false;
        }
        // Copied out because the closure below would otherwise hold a borrow of
        // the state struct across every mutation in the rest of the tick.
        let prev_pad=game.prev_pad;
        let pressed=|b:u16| pad.is_held(b)&&!prev_pad.is_held(b);
        // Mothwing Cloak. Nothing in the admitted scenes grants it, so
        // the cheat is the only source until the pickups exist.
        game.player.has_dash=game.settings.cheats.dash;
        game.player.has_walljump=game.settings.cheats.claw;
        game.player.has_double_jump=game.settings.cheats.wings;
        game.player.has_super_dash=game.settings.cheats.heart;
        game.player.has_shade_cloak=game.settings.cheats.cloak;
        game.player.super_dash_input(PARAMS,pad.is_held(button::R1)&&game.vitals.can_control()&&!locked_for_dash(&game.focus,&game.door,&game.bench));
        game.player.dash_input(PARAMS,pad.is_held(button::L1)&&game.vitals.can_control()&&!locked_for_dash(&game.focus,&game.door,&game.bench));
        // The seated prompt owns input while it is up, so the bench
        // neither reads a leave press nor starts another rest.
        let asking=game.save_prompt.is_some();
        if let Some(row)=game.save_prompt {
            if pressed(button::UP)||pressed(button::DOWN) {game.save_prompt=Some(1-row);}
            else if pressed(button::CROSS) {
                if row==0 {
                    let stand=[game.bench.seat()[0],game.player.y];
                    let (owned,equipped,notches,can_overcharm)=charms::record();
                    let (script_fields,script_field_fnv)=script::record();
                    game.save_requested=Some(save::Save{scene:r.scene as u32,seat:stand,facing:game.player.facing,
                        region:game.region_id as u32,geo:game.geo.wallet(),door_hits:game.door.hits(),
                        shade:game.shade.record(),sequence:0,charms_owned:owned,
                        charms_equipped:equipped,charm_notches:notches,can_overcharm,
                        npc_conversations:dialogue::met_bits(),script_fields,script_field_fnv});
                }
                game.save_prompt=None;
            } else if pressed(button::CIRCLE) {game.save_prompt=None;}
        }
        let bench_events={
            let hero_body=[game.player.x-PARAMS.half_width,game.player.y+PARAMS.bottom,game.player.x+PARAMS.half_width,game.player.y+PARAMS.top];
            let free=!(game.focus.locks_control()||dialogue::open()||game.door.pending()||read_consumes||asking)&&game.vitals.can_control()&&!game.nail.active;
            let leave=!asking&&[button::CROSS,button::SQUARE,button::LEFT,button::RIGHT,button::UP,button::DOWN].iter().any(|&b|pressed(b));
            let grounded=game.player.grounded;
            game.bench.tick(r,hero_body,&mut game.player,grounded,free,!asking&&pressed(button::UP),leave)
        };
        if bench_events.rest {
            // Rest Burst: HERO REVIVED and the respawn marker, then ask.
            // The source saves here without asking; this port never
            // writes the card unless the player says so.
            game.vitals.heal(game.settings.cheats.params(VITAL_PARAMS),u16::MAX);
            // Sitting keeps the Knight's floor y; the source seat y is the sprite's, and a
            // body spawned there starts inside the floor and falls through it.
            let stand=[game.bench.seat()[0],game.player.y];
            game.respawn=Some((r.scene,stand,game.player.facing,game.region_id));
            game.save_prompt=Some(0);
        }
        let locked=game.focus.locks_control()||dialogue::open()||game.shop_screen.open||game.door.pending()||game.bench.locks_control()||game.save_prompt.is_some()||game.cast.locks_control();
        // Take Control: the Dream Nail owns the Knight from Start to End.
        game.dream.has_dream_nail=game.settings.cheats.dream;
        game.dream.tick(DREAM_NAIL_PARAMS,pad.is_held(button::TRIANGLE)&&game.vitals.can_control(),&game.player,locked);
        let locked=locked||game.dream.locks_control();
        let dir=if game.door.entering() {game.door.forced_direction()}else if locked {0}else{i32::from(pad.is_held(button::RIGHT))-i32::from(pad.is_held(button::LEFT))};
        let was_grounded=game.player.grounded;
        let jump=!locked && !read_consumes && pad.is_held(button::CROSS);
        let could_jump=was_grounded && !game.player.was_jump && jump && game.vitals.can_control() && game.nail_response.bounce_left==0;
        if game.door.drop_starts() {game.player.vy=-12*ONE;}
        if game.door.apply_entry(&mut game.player) {
            // Source entry owns position/animation while input is locked.
        } else if game.bench.locks_control() {
            // Seated or getting off: the bench owns position and animation.
            game.player.grounded=true;game.player.jumping=false;game.player.was_jump=pad.is_held(button::CROSS);
        } else if let Some((vx,vy))=game.vitals.recoil_velocity(VITAL_PARAMS) {
            let mut p=PARAMS;p.speed=vx.abs();p.gravity=0;
            game.player.vy=vy;game.player.jumping=false;
            game.player.step(p,vx.signum(),false,room.counts[5],game.state.edge_reader(r,room));
            game.player.facing=-game.vitals.recoil_direction;
        } else {
            if could_jump {audio::jump();}
            game.nail_response.step(NAIL_RESPONSE_PARAMS,PARAMS,&mut game.player,dir,jump,room.counts[5],game.state.edge_reader(r,room));
        }
        audio::movement_tick(was_grounded,game.player.grounded,game.player.vy<0,
            (game.door.entry_footsteps() || (dir!=0 && !locked)) && game.vitals.can_control() && game.vitals.recoil_velocity(VITAL_PARAMS).is_none(),true);
        let vertical=i32::from(pad.is_held(button::UP))-i32::from(pad.is_held(button::DOWN));
        if game.nail.tick(ATTACK_PARAMS,pad.is_held(button::SQUARE)&&game.vitals.can_control()&&!locked&&!read_consumes,vertical,&mut game.player) {game.attacks=game.attacks.wrapping_add(1);game.geo.begin_attack();audio::nail();}
        // HeroController.Bounce returns early while cState.shroomBouncing,
        // so an ordinary pogo cannot cut a shroom's rise short.
        if game.vitals.can_control() && !strike_shroom(r,&game.nail,&mut game.player) && !game.player.shroom_bouncing {
            enemies::pogo_contact(r,&game.state,&mut game.player,&game.nail,ATTACK_PARAMS,NAIL_POLYGONS,&mut game.nail_response);
        }
        let strike=game.state.strike(r,&game.nail,ATTACK_PARAMS,NAIL_POLYGONS,&game.player);
        unsafe {HK_BREAK_COUNT=HK_BREAK_COUNT.saturating_add(strike.broken as u32);}
        if strike.door_sounds>0 {audio::door();}
        let mining=strike_geo(game.geo,r.scene,&game.nail,&game.player);
        if mining.hits>0 {geo_audio::hit();}
        if mining.depleted>0 {geo_audio::break_rock();geo_render::apply(game.geo,&mut game.state,game.region_id);}
        // A break or depleted rock invalidated the edge table; rebuild it now so the
        // rest of this tick (enemies, coins, Lifeblood) does not fall back to the
        // per-query breakable scan, which cost several VBlanks with three coins.
        if strike.broken>0 || mining.depleted>0 {game.state.refresh_edges(r,room);}
        presentation::checkpoint();
        let blue=game.life.tick(r.scene,game.player.x,r.collision_bounds,room.counts[5],game.state.edge_reader(r,room));
        game.vitals.add_blue_health(blue);
        if strike_lifeblood(game.life,r.scene,&game.nail,&game.player).opened {lifeblood::apply(game.life,&mut game.state,game.region_id);}
        let door_hit=strike_great_door(&mut game.door,r.scene,&game.nail,&game.player);
        if door_hit.hit {audio::great_door_hit();}
        if door_hit.opened {
            great_door::apply(&game.door,&mut game.state,game.region_id);
            dialogue::cancel();game.focus.interrupt();focus_audio::interrupt();
        }
        {
            let hero_body=[game.player.x-PARAMS.half_width,game.player.y+PARAMS.bottom,game.player.x+PARAMS.half_width,game.player.y+PARAMS.top];
            game.camera.tick((game.player.x,game.player.y),game.player.facing,camera::limits(r,hero_body));
        }
        if game.shade.present_here() {
            let hero_body=[game.player.x-PARAMS.half_width,game.player.y+PARAMS.bottom,game.player.x+PARAMS.half_width,game.player.y+PARAMS.top];
            if strike_shade(&mut game.shade,&game.nail,&game.player,game.settings.cheats.params(VITAL_PARAMS).nail_damage) {audio::enemy_hit();}
            let shade_position=game.shade.position().unwrap_or([game.player.x,game.player.y]);
            let shade_events=game.shade.tick([game.player.x,game.player.y],hero_body,room.counts[5],game.state.edge_reader(r,room));
            if shade_events.touched {
                let direction=if game.player.x<shade_position[0] {-1}else{1};
                apply_hurt(&game.settings.cheats,&mut game.vitals,&game.player,&mut game.nail,&mut game.nail_response,&mut game.focus,1,direction,false);
            }
            if shade_events.returned_geo>0 {game.geo.add_quietly(shade_events.returned_geo,geo::GEO_PARAMS);geo_audio::pickup(1);}
        }
        let enemy_events=game.enemies.tick(r,room,&mut game.state,&mut game.player,&mut game.vitals,&game.nail,&game.dream,game.cast.ball_bounds(FIREBALL_PARAMS).map(|b|(b,FIREBALL_PARAMS.damage)),&mut game.nail_response,ATTACK_PARAMS,NAIL_POLYGONS,game.settings.cheats,
            [camera.0,camera.1,-2496922],runner_event,
            |scene,x,y| {let i=cache.locate(scene,x,y)?;Some((world::resident(i)?,cache.room_for(i)?))});
        game.enemies.take_geo_deaths(r.scene,|source,x,y|game.geo.spawn_enemy(r.scene,source,[x,y],geo::GEO_ENEMIES));
        let h=geo::HERO_BOX;
        let collected=game.geo.tick(r.scene,geo::GEO_PARAMS,[game.player.x+h[0],game.player.y+h[1],game.player.x+h[2],game.player.y+h[3]],
            r.collision_bounds,room.counts[5],game.state.edge_reader(r,room));
        if collected.collected>0 {geo_audio::pickup(collected.last_value);}
        presentation::checkpoint();
        unsafe {HK_ENEMY_HITS+=enemy_events.hits as u32;HK_ENEMY_KILLS+=enemy_events.kills as u32;HK_SOUL=game.vitals.soul as u32;}
        if enemy_events.dream_soul>0 {game.vitals.add_soul(VITAL_PARAMS,enemy_events.dream_soul);}
        if enemy_events.hits>0 {audio::enemy_hit();}
        respond_to_hurt(enemy_events.hurt,&mut game.nail,&mut game.nail_response,&mut game.focus);
        let bank_region=cache.world_metadata().region(meta_region).expect("admitted world metadata covers the selected region");
        if let Some((damage,hazard,direction))=world::hazard_contact(bank_region,&game.player,PARAMS) {
            apply_hurt(&game.settings.cheats,&mut game.vitals,&game.player,&mut game.nail,&mut game.nail_response,&mut game.focus,damage,direction,hazard);
        }
        if let Some((spawn,facing))=world::checkpoint(bank_region,&game.player,PARAMS) {
            if let Some(id)=cache.locate(r.scene,spawn[0],spawn[1]) {
                game.safe=(spawn[0],spawn[1],id);game.safe_facing=facing;
            }
        }
        if game.gate_cooldown==0 && (door_exit || !game.door.pending()) && !game.vitals.dead && !game.vitals.hazard_pending {
            let gate=if door_exit {
                Some(world::GATES.iter().find(|g|g.scene==r.scene&&g.target_scene==great_door::TARGET_SCENE).expect("Great Door exit gate"))
            } else {
                // A door answers UP against the hero body; every other
                // gate keeps the point test it has always used.
                let hero_body=[game.player.x-PARAMS.half_width,game.player.y+PARAMS.bottom,game.player.x+PARAMS.half_width,game.player.y+PARAMS.top];
                world::gate(r.scene,&game.player,hero_body,pressed(button::UP)&&!read_consumes,game.vitals.recoil_ticks!=0,game.scene_ticks)
            };
            if let Some(g)=gate {
                let side_entry=(g.scene==0&&g.target_scene==great_door::TARGET_SCENE)
                    ||(g.scene==great_door::TARGET_SCENE&&g.target_scene==0);
                let (spawn,id)=if side_entry {
                    let entry=if g.target_scene==0 {&great_door::RETURN_ENTRY}else{&great_door::ENTRY};
                    (entry.spawn,entry.region)
                } else {(g.spawn,g.target_region)};
                {let id=id;
                    game.geo.leave_scene(r.scene,geo::GEO_PARAMS);game.geo.reset_enemies(r.scene);
                    game.vitals.add_blue_health(game.life.leave_scene());
                    dialogue::cancel();game.state.reset_scene(r.scene);game.reveals.leave_scene();game.enemies.reset_scene(r.scene);runner_audio::reset();audio::reset_movement();game.player=Player::spawn(spawn[0],spawn[1]);
                    // The Great Door pair walks in through its audited entries; every
                    // other side gate uses the same source sequence toward the scene
                    // interior (a right exit enters at the destination's left gate).
                    if side_entry {
                        if door_exit {game.door.begin_entry(g.target_scene);}else{game.door.begin_gate_entry(g.target_scene,if g.target_scene==0 {-1}else{1});}
                        game.player.grounded=true;
                    } else if g.side==1||g.side==2 {
                        game.door.begin_gate_entry(g.target_scene,if g.side==2 {1}else{-1});
                        game.player.facing=if g.side==2 {1}else{-1};game.player.grounded=true;
                    } else if g.side==3 {
                        game.door.begin_top_entry(g.target_scene);
                    } else {game.player.vy=g.entry_vy;}
                    game.nail=Nail::new();game.nail_response=hk_sim::NailResponse::new();game.focus.interrupt();focus_audio::interrupt();*target=Some(id);game.safe=(game.player.x,game.player.y,id);game.safe_facing=game.player.facing;game.gate_cooldown=90;
                }
            }
        }
        if target.is_none() && !game.vitals.dead && !game.vitals.hazard_pending && !world::contains(r.bounds,game.player.x,game.player.y) {
            if let Some(id)=cache.locate(r.scene,game.player.x,game.player.y) {*target=Some(id);*spatial_target=true;}
            else {
                apply_hurt(&game.settings.cheats,&mut game.vitals,&game.player,&mut game.nail,&mut game.nail_response,&mut game.focus,1,0,true);
            }
        }
    }
    game.reveals.tick(r.reveal_masks,&game.player,PARAMS);
}
