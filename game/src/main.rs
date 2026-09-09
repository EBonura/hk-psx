#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]
extern crate psx_rt;
use hk_format::{i32_at, u32_at, Room};
use hk_sim::{Nail, Player, ONE};
use psx_gpu::{self as gpu, framebuf::FrameBuffer, Resolution, VideoMode};
use psx_pad::{button, ButtonState};
use psx_telemetry::{counter, emit, stage, task};

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/params.rs"));
mod input_queue;
mod input_sampler;
mod input;
mod animation_cache;
mod audio;
mod ambience;
mod enemies;
mod hud;
mod world;
mod reveal_masks;
mod dialogue;
mod impact;
mod preload;
mod disc;
mod room_decode;
mod texture_upload;
mod vram_cache;
mod menu;
mod pause;
mod geo;
mod geo_render;
mod geo_audio;
mod lifeblood;
mod render;
mod alpha_scissor;
mod alpha_scissor_cache;
mod scenery_geometry;
mod draw_packet;
/// Read-only marker: 0 title, 1 gameplay, 2 loading, 3 disc error.
#[no_mangle]
pub static mut HK_GAME_MODE: u32 = 0;
/// One bit per cooked grass trigger; reset by the chamber's reset control.
#[no_mangle]
pub static mut HK_GRASS_CUT_MASK: u32 = 0;
#[no_mangle]
pub static mut HK_NAIL_ATTACK_COUNT: u32 = 0;
#[no_mangle] pub static mut HK_FOCUS_STARTED:u32=0;
#[no_mangle] pub static mut HK_FOCUS_COMPLETED:u32=0;
#[no_mangle] pub static mut HK_FOCUS_HEALED:u32=0;
#[no_mangle] pub static mut HK_FOCUS_DRAINED:u32=0;
#[no_mangle] pub static mut HK_FOCUS_REFUNDED:u32=0;
#[no_mangle] pub static mut HK_FOCUS_LOCKED:u32=0;
#[no_mangle] pub static mut HK_REVEAL_MASKS_HIDDEN:u32=0;
#[no_mangle] pub static mut HK_REVEAL_MASKS_PARTIAL:u32=0;
#[no_mangle] pub static mut HK_REVEAL_MASKS_VISIBLE:u32=0;
fn apply_reveal_masks(state:&reveal_masks::State,region:&world::Region) {
    let counts=state.apply(region.reveal_masks,region.reveal_bindings);
    unsafe {HK_REVEAL_MASKS_HIDDEN=counts.hidden;HK_REVEAL_MASKS_PARTIAL=counts.partial;HK_REVEAL_MASKS_VISIBLE=counts.visible;}
}
fn knight_frame(room: &Room, player: &Player, focus: &hk_sim::Focus) -> usize {
    let (clip,age)=if let Some((clip,age))=focus.animation() {
        let offset=match clip {hk_sim::FocusClip::Focus=>0,hk_sim::FocusClip::Get=>1,hk_sim::FocusClip::End=>2,hk_sim::FocusClip::GetOnce=>3};
        (FOCUS_CLIP_BASE+offset,age)
    } else {(player.animation as usize,player.animation_tick)};
    let c = room.clip(clip);
    let time = (age as u64 * c[2] as u64 / (60 * 65536)) as u32;
    let mode = c[3] & 65535;
    let loop_start = c[3] >> 16;
    let frame = if mode == 0 {
        time % c[1]
    } else if mode == 1 && time >= c[1] {
        loop_start + (time - loop_start) % (c[1] - loop_start)
    } else {
        time.min(c[1] - 1)
    };
    (c[0] + frame) as usize
}
fn nail_frame(room: &Room, nail: &Nail) -> Option<usize> {
    if !nail.active {
        return None;
    }
    let clip = room.clip(nail.effect_clip());
    let frame = (nail.age as u64 * clip[2] as u64 / (60 * 65536)) as u32;
    // Effect clips are one-shot, even while the attack recovery remains active.
    if frame >= clip[1] {
        return None;
    }
    Some((clip[0] + frame) as usize)
}
fn draw_frame(room: &Room, frame: usize, player: &Player, camera: (i32, i32)) {
    let f = room.frame(frame);
    let b = core::array::from_fn::<_, 4, _>(|i| i32_at(f, 4 + i * 4));
    // Full 320x240 view: original vertical FOV, horizontally cropped to 4:3.
    let coords = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
    let mut v = [(0i16, 0i16); 4];
    for k in 0..4 {
        // Retail FaceRight uses localScale.x = -1; the atlas faces left.
        let x = player.x - coords[k].0 * player.facing - camera.0;
        let y = player.y + coords[k].1 - camera.1;
        v[k] = (
            (160 + (((x >> 8) * KNIGHT_SCALE) >> 20)) as i16,
            (120 - (((y >> 8) * KNIGHT_SCALE) >> 20)) as i16,
        );
    }
    render::texture(u32_at(f, 0) as usize, v, (128, 128, 128));
}
#[cfg(feature = "emulator-telemetry")]
fn log_state(frame: u32, p: &Player, prims: u32) {
    use core::fmt::Write;
    struct Text {
        b: [u8; 192],
        n: usize,
    }
    impl core::fmt::Write for Text {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for b in s.bytes() {
                if self.n < 192 {
                    self.b[self.n] = b;
                    self.n += 1;
                }
            }
            Ok(())
        }
    }
    let mut text = Text { b: [0; 192], n: 0 };
    let vb = unsafe {
        core::ptr::read_volatile(core::ptr::addr_of!(
            psx_rt::interrupts::__psx_rt_vblank_count
        ))
    };
    let _ = write!(
        text,
        "HK frame={} vb={} x={} y={} vy={} ground={} anim={} quads={}",
        frame, vb, p.x, p.y, p.vy, p.grounded, p.animation, prims
    );
    if let Ok(s) = core::str::from_utf8(&text.b[..text.n]) {
        emit::debug_log(s);
    }
}
static mut ROOM_CACHE: disc::Cache = unsafe { disc::Cache::new() };
static mut GEO_WORLD:geo::World=geo::World::new();
static mut LIFE_WORLD:lifeblood::World=lifeblood::World::new();
#[no_mangle] pub static mut HK_HAZARD_RESPAWNS:u32=0;
#[no_mangle] pub static mut HK_CRAWLER_X:i32=0;
#[no_mangle] pub static mut HK_CRAWLER_Y:i32=0;
#[no_mangle] pub static mut HK_CRAWLER_HP:i32=0;
#[no_mangle] pub static mut HK_ENEMY_HITS:u32=0;
#[no_mangle] pub static mut HK_ENEMY_KILLS:u32=0;
#[no_mangle] pub static mut HK_SOUL:u32=0;
#[no_mangle] pub static mut HK_REGION_ID:u32=0;
#[no_mangle] pub static mut HK_BREAK_COUNT:u32=0;
#[no_mangle] pub static mut HK_HEALTH:u32=5;
#[no_mangle] pub static mut HK_DEATHS:u32=0;
#[no_mangle] pub static mut HK_PLAYER_X:i32=0;
#[no_mangle] pub static mut HK_PLAYER_Y:i32=0;
#[no_mangle] pub static mut HK_PAUSED:u32=0;
#[no_mangle] pub static mut HK_PAUSE_CONTROLS:u32=0;
#[no_mangle] pub static mut HK_SFX_LEVEL:u32=10;
#[no_mangle] pub static mut HK_AMBIENCE_LEVEL:u32=10;
#[no_mangle] pub static mut HK_GEO_LOST:u32=0;
#[no_mangle] pub static mut HK_BLUE_HEALTH:u32=0;
#[inline(never)]
fn strike_lifeblood(life:&mut lifeblood::World,scene:usize,nail:&Nail,player:&Player)->lifeblood::Strike {
    if !nail.hitting(ATTACK_PARAMS) {return lifeblood::Strike::default();}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    life.strike(scene,&polygon[..source.len()])
}
#[inline(never)]
fn strike_geo(geo:&mut geo::World,scene:usize,nail:&Nail,player:&Player)->geo::Strike {
    if !nail.hitting(ATTACK_PARAMS) {return geo::Strike::default();}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    geo.strike(scene,geo::GEO_ROCKS,&polygon[..source.len()])
}
/// Boundary backpressure is reported separately from visual-frame cadence.
#[no_mangle] pub static mut HK_BOUNDARY_WAIT_TICKS:u32=0;
#[no_mangle] pub static mut HK_BOUNDARY_WAIT_MAX:u32=0;
#[no_mangle] pub static mut HK_PAD_POLL_MAX_VBLANK_GAP:u32=0;
/// Keep the deeper RAM plan separate from the three direct GPU exits.
fn wishlist(index:usize,player:&Player)->preload::Plan {
    preload::plan_motion(index,player.x,player.y,player.facing,preload::Motion {
        grounded:player.grounded,bottom:PARAMS.bottom,vy:player.vy,
    },|id| {
        let r=&world::REGIONS[id];preload::Cell {bounds:r.bounds,neighbours:r.neighbours,scene:r.scene,
            flat_floor_heights:r.flat_floor_heights}
    })
}
/// Caller has retired the entire GPU frame. A pinned RAM view survives every
/// CD pump until its inactive VRAM bank has completed all bounded slices.
fn prepare_vram(cache:&mut disc::Cache,vram:&mut vram_cache::Cache,wanted:&[usize]) {
    if vram.pending().is_none() {
        for &id in wanted {
            if vram.ready(id)||!cache.is_ready(id) {continue;}
            if cache.protect_upload(Some(id)) {
                let room=cache.room_for(id).unwrap();
                if vram.begin(id,&room,wanted) {break;}
                cache.protect_upload(None);
            }
        }
    }
    if let Some(id)=vram.pending() {
        let room=cache.room_for(id).expect("GPU source evicted");
        if vram.pump(&room) {cache.protect_upload(None);}
    }
}
#[no_mangle]
fn main() {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    psx_rt::interrupts::install_vblank_counter();
    let mut fb=FrameBuffer::new(320,240);
    gpu::set_draw_area(0,0,319,239);gpu::set_draw_offset(0,0);
    audio::init();let mut settings=menu::run(&mut fb);menu::loading(&mut fb);
    let cache=unsafe { &mut *(&raw mut ROOM_CACHE) };
    let mut state=world::State::new();
    let geo=unsafe {&mut *(&raw mut GEO_WORLD)};
    let life=unsafe {&mut *(&raw mut LIFE_WORLD)};
    let mut reveals=reveal_masks::State::new();
    let mut enemies=enemies::EnemyWorld::new();
    let mut player=Player::spawn(SPAWN.0,SPAWN.1);
    let mut vitals=hk_sim::Vitals::new(VITAL_PARAMS);
    let mut nail=Nail::new();
    let mut focus=hk_sim::Focus::new();
    let mut nail_response=hk_sim::NailResponse::new();
    let mut region_id=0usize;
    let mut frame=0u32;let mut attacks=0u32;
    let mut safe=(SPAWN.0,SPAWN.1,0usize);
    let mut safe_facing=1;
    let mut gate_cooldown=0u16;
    let mut paused=false;let mut start_held=false;
    let mut pause_menu=pause::State::new();
    let mut vram=vram_cache::Cache::new();
    let mut sim_clock=psx_rt::interrupts::vblank_count();
    let mut initial=true;
    // Timer1 counts HBlanks for frame-pacing telemetry only.
    psx_io::timers::set_mode(psx_io::timers::Timer::Timer1,0x0100);
    'region: loop {
        if initial {
            unsafe { HK_GAME_MODE=2; }
            gpu::draw_sync();
            while cache.prepare_ambience().and_then(|_|cache.select(region_id)).is_err() {
                unsafe { HK_GAME_MODE=3; }
                vram=vram_cache::Cache::new();cache.protect_upload(None);
                menu::restore();menu::retry(&mut fb);menu::loading(&mut fb);
            }
            // Both authored scenes fit the proven RAM/VRAM layout. Admit all
            // neighbouring regions before simulation starts, including gates.
            for scene in 0..disc::SCENE_COUNT {
                let id=disc::scene_region(scene);
                while !vram.ready(id) {prepare_vram(cache,&mut vram,&[id]);}
            }
            vram.activate(region_id);
            hud::upload();dialogue::upload();geo_render::upload();lifeblood::upload();world::particles::break_effects::upload();geo_audio::upload();geo_audio::set_volume(settings.sfx);
            initial=false;
            sim_clock=psx_rt::interrupts::vblank_count();
            input::start(sim_clock);
        }
        // Publish the new region's neighbours before its first frame can pump
        // the loader. The old wishlist omits the room we just left and can
        // otherwise evict it before a short return crossing.
        let plan=wishlist(region_id,&player);
        cache.prefetch_regions(&plan.ram[..plan.ram_len]);
        let r=&world::REGIONS[region_id];
        assert!(ambience::set_scene(r.scene as u8));
        enemies.sync_region(r);
        reveals.scene_ready(r.scene,r.reveal_masks);
        let bank=vram.activate(region_id);
        render::init(&cache.room(),bank,region_id);state.apply(r);apply_reveal_masks(&reveals,r);
        geo_render::apply(geo,&mut state,region_id);
        lifeblood::apply(life,&mut state,region_id);
        unsafe { HK_GAME_MODE=1;HK_REGION_ID=region_id as u32+1; }
        let mut pending_target=None;
        let mut boundary_ticks=0u32;
        let mut spatial_target=false;
        loop {
            let mut target=pending_target.take();
            let room=cache.room();
            input::checkpoint();render::mark_frame_start();
            emit::frame_begin(frame);emit::stage_begin(stage::RENDER);
            state.apply(r);
            apply_reveal_masks(&reveals,r);
            geo_render::apply(geo,&mut state,region_id);
            lifeblood::apply(life,&mut state,region_id);
            let camera=(player.x.clamp(r.camera[0],r.camera[2]),(player.y+2*ONE).clamp(r.camera[1],r.camera[3]));
            let body_frame=knight_frame(&room,&player,&focus);let effect_frame=nail_frame(&room,&nail);
            let enemy_draws=enemies.prepare_draws(r,&room,camera);
            let mut needed=[0u16;8];needed[0]=u32_at(room.frame(body_frame),0) as u16;
            let mut needed_len=if effect_frame.is_some(){2}else{1};
            if let Some(f)=effect_frame {needed[1]=u32_at(room.frame(f),0) as u16;}
            enemy_draws.append_needed(&mut needed,&mut needed_len);
            render::begin(&room,&needed[..needed_len],fb.buffer_y(fb.drawing));
            fb.clear(9,13,20);let mut prims=render::scenery(camera,false);prims+=enemy_draws.draw();render::kick_ready();prims+=state.draw_impacts(r,&room,camera);
            prims+=geo_render::draw(geo,region_id,camera);
            prims+=lifeblood::draw(life,region_id,camera);render::kick_ready();
            if !vitals.dead && (vitals.invulnerable_ticks==0 || frame%4<2) {
                draw_frame(&room,body_frame,&player,camera);prims+=1;
            }
            if let Some(f)=effect_frame {draw_frame(&room,f,&player,camera);prims+=1;}
            prims+=render::scenery(camera,true);dialogue::prepare(camera,geo.wallet(),paused.then_some((&pause_menu,&settings)));render::submit(vitals.health,VITAL_PARAMS.max_health,vitals.soul,VITAL_PARAMS.max_soul,paused,vitals.blue_health);
            emit::counter(counter::TRI_PRIMITIVES,prims*2);emit::stage_end(stage::RENDER);
            emit::stage_begin(stage::PRESENT);
            let display=fb.begin_deferred_swap();
            let mut display_queued=false;
            let mut front_pending=true;
            let mut ticks=0;
            loop {
                // An earlier chunk may still be walking: kick the final list as
                // soon as the channel is free, then queue the display only
                // after that final list (including HUD) has reached GP0; the
                // IRQ then also waits for raster completion at a true VBlank edge.
                if !render::dma_pending() {
                    if front_pending {render::kick_front();front_pending=false;}
                    else if !display_queued {
                        psx_rt::interrupts::queue_gp1_at_vblank(display);
                        display_queued=true;unsafe {render::HK_FRAME_FLIP_LINES=render::frame_lines();}
                    }
                }
                if display_queued && !psx_rt::interrupts::gp1_queue_pending() {break;}
                input::checkpoint();
                let _=cache.pump(); // Detailed errors remain in the CD markers.
                input::checkpoint();
                let now=psx_rt::interrupts::vblank_count();
                while sim_clock!=now {
                    sim_clock=sim_clock.wrapping_add(1);
                    let pad=ButtonState::from_bits(input::consume(sim_clock));
                    dialogue::tick(r.scene,&player,PARAMS,target.is_none() && !paused && !vitals.dead && player.grounded && vitals.can_control() && !nail.active && !focus.locks_control(),pad.bits());
                    ambience::tick();
                    let start=pad.is_held(button::START);
                    let mut ui_consumed=paused;
                    if start&&!start_held && !dialogue::open() {
                        paused=!paused;ui_consumed=true;
                        if paused {pause_menu.enter(pad.bits());}
                    }
                    start_held=start;
                    if paused {
                        let before=(settings.sfx,settings.ambience);
                        if pause_menu.step(pad.bits(),&mut settings.sfx,&mut settings.ambience) {
                            paused=false;player.was_jump=pad.is_held(button::CROSS);
                        }
                        if before!=(settings.sfx,settings.ambience) {audio::set_volume(settings.sfx);geo_audio::set_volume(settings.sfx);ambience::set_volume(settings.ambience);}
                    }
                    unsafe {HK_PAUSED=u32::from(paused);HK_PAUSE_CONTROLS=u32::from(paused&&pause_menu.controls);
                        HK_SFX_LEVEL=settings.sfx as u32;HK_AMBIENCE_LEVEL=settings.ambience as u32;}
                    let ready_crossing=spatial_target && target.is_some_and(|id|cache.is_ready(id)&&vram.ready(id))
                        && world::contains(r.collision_bounds,player.x,player.y);
                    if target.is_some() && !ready_crossing && !pad.is_held(button::SELECT) {
                        boundary_ticks=boundary_ticks.saturating_add(1);
                        unsafe {HK_BOUNDARY_WAIT_TICKS=HK_BOUNDARY_WAIT_TICKS.saturating_add(1);HK_BOUNDARY_WAIT_MAX=HK_BOUNDARY_WAIT_MAX.max(boundary_ticks);}
                        continue;
                    }
                    boundary_ticks=0;
                    if spatial_target {target=None;spatial_target=false;}
                    let room=cache.room();
                    emit::task_begin(task::FIXED_UPDATE);emit::stage_begin(stage::UPDATE);
                    if pad.is_held(button::SELECT) {
                        geo.reset(geo::GEO_PARAMS);unsafe {HK_GEO_LOST=0;}
                        life.reset();
                        dialogue::reset();state=world::State::new();reveals=reveal_masks::State::new();enemies=enemies::EnemyWorld::new();vitals=hk_sim::Vitals::new(VITAL_PARAMS);
                        player=Player::spawn(SPAWN.0,SPAWN.1);safe=(SPAWN.0,SPAWN.1,0);safe_facing=1;
                        nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();attacks=0;target=Some(0);paused=false;
                        unsafe {HK_BREAK_COUNT=0;HK_DEATHS=0;HK_ENEMY_HITS=0;HK_ENEMY_KILLS=0;HK_HAZARD_RESPAWNS=0;HK_FOCUS_STARTED=0;HK_FOCUS_COMPLETED=0;HK_FOCUS_HEALED=0;HK_FOCUS_DRAINED=0;HK_FOCUS_REFUNDED=0;}
                    } else if !paused && !ui_consumed && !vitals.tick() {
                        gate_cooldown=gate_cooldown.saturating_sub(1);
                        state.tick();
                        state.tick_debris(r, &room);
                        if vitals.needs_death_respawn() {
                            unsafe {HK_GEO_LOST=HK_GEO_LOST.saturating_add(geo.death());}
                            geo.leave_scene(r.scene,geo::GEO_PARAMS);geo.reset_enemies(r.scene);
                            life.reset();
                            dialogue::cancel();state.reset_scene(r.scene);reveals.reset_scene(r.scene);enemies.reset_scene(r.scene);vitals=hk_sim::Vitals::new(VITAL_PARAMS);
                            player=Player::spawn(SPAWN.0,SPAWN.1);safe=(SPAWN.0,SPAWN.1,0);safe_facing=1;
                            nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();target=Some(0);
                        } else if vitals.hazard_pending {
                            dialogue::cancel();
                            unsafe {HK_HAZARD_RESPAWNS+=1;}
                            player=Player::spawn(safe.0,safe.1);player.facing=safe_facing;nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();
                            vitals.finish_hazard_respawn(VITAL_PARAMS);target=Some(safe.2);
                        } else if !vitals.dead {
                            let read_consumes=dialogue::consumes_actions();
                            let focus_events=focus.step(FOCUS_PARAMS,VITAL_PARAMS,hk_sim::FocusInput {
                                held:pad.is_held(button::CIRCLE)&&!read_consumes,grounded:player.grounded,
                                can_start:vitals.can_control() && (!nail.active || nail.age>=FOCUS_PARAMS.attack_recovery_ticks),
                            },&mut vitals);
                            unsafe {
                                HK_FOCUS_STARTED+=u32::from(focus_events.started);HK_FOCUS_COMPLETED+=u32::from(focus_events.completed);
                                HK_FOCUS_HEALED+=u32::from(focus_events.healed);HK_FOCUS_DRAINED+=u32::from(focus_events.drained);
                                HK_FOCUS_REFUNDED+=u32::from(focus_events.refunded);
                            }
                            if focus_events.started {
                                nail=Nail::new();nail_response=hk_sim::NailResponse::new();player.vy=0;player.jumping=false;
                            }
                            let locked=focus.locks_control()||dialogue::open();
                            let dir=if locked {0}else{i32::from(pad.is_held(button::RIGHT))-i32::from(pad.is_held(button::LEFT))};
                            let was_grounded=player.grounded;
                            let jump=!locked && !read_consumes && pad.is_held(button::CROSS);
                            let could_jump=was_grounded && !player.was_jump && jump && vitals.can_control() && nail_response.bounce_left==0;
                            if let Some((vx,vy))=vitals.recoil_velocity(VITAL_PARAMS) {
                                let mut p=PARAMS;p.speed=vx.abs();p.gravity=0;
                                player.vy=vy;player.jumping=false;
                                player.step(p,vx.signum(),false,room.counts[5],|i|state.edge(r,&room,i));
                                player.facing=-vitals.recoil_direction;
                            } else {
                                if could_jump {audio::jump();}
                                nail_response.step(NAIL_RESPONSE_PARAMS,PARAMS,&mut player,dir,jump,room.counts[5],|i|state.edge(r,&room,i));
                            }
                            if !was_grounded && player.grounded {audio::land();}
                            let vertical=i32::from(pad.is_held(button::UP))-i32::from(pad.is_held(button::DOWN));
                            if nail.tick(ATTACK_PARAMS,pad.is_held(button::SQUARE)&&vitals.can_control()&&!locked&&!read_consumes,vertical,&mut player) {attacks=attacks.wrapping_add(1);geo.begin_attack();audio::nail();}
                            if vitals.can_control() {enemies::pogo_contact(r,&state,&mut player,&nail,ATTACK_PARAMS,NAIL_POLYGONS,&mut nail_response);}
                            let strike=state.strike(r,&nail,ATTACK_PARAMS,NAIL_POLYGONS,&player);
                            unsafe {HK_BREAK_COUNT=HK_BREAK_COUNT.saturating_add(strike.broken as u32);}
                            if strike.door_sounds>0 {audio::door();}
                            let mining=strike_geo(geo,r.scene,&nail,&player);
                            if mining.hits>0 {geo_audio::hit();}
                            if mining.depleted>0 {geo_audio::break_rock();geo_render::apply(geo,&mut state,region_id);}
                            let blue=life.tick(r.scene,player.x,r.collision_bounds,room.counts[5],|i|state.edge(r,&room,i));
                            vitals.add_blue_health(blue);
                            if strike_lifeblood(life,r.scene,&nail,&player).opened {lifeblood::apply(life,&mut state,region_id);}
                            let enemy_events=enemies.tick(r,&room,&mut state,&mut player,&mut vitals,&nail,&mut nail_response,ATTACK_PARAMS,NAIL_POLYGONS);
                            enemies.take_geo_deaths(r.scene,|source,x,y|geo.spawn_enemy(r.scene,source,[x,y],geo::GEO_ENEMIES));
                            let h=geo::HERO_BOX;
                            let collected=geo.tick(r.scene,geo::GEO_PARAMS,[player.x+h[0],player.y+h[1],player.x+h[2],player.y+h[3]],
                                r.collision_bounds,room.counts[5],|i|state.edge(r,&room,i));
                            if collected.collected>0 {geo_audio::pickup(collected.last_value);}
                            unsafe {HK_ENEMY_HITS+=enemy_events.hits as u32;HK_ENEMY_KILLS+=enemy_events.kills as u32;HK_SOUL=vitals.soul as u32;}
                            if enemy_events.hits>0 {audio::enemy_hit();}
                            if enemy_events.hurt!=hk_sim::Hurt::Ignored {dialogue::cancel();audio::hurt();nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();}
                            if enemy_events.hurt==hk_sim::Hurt::Died {unsafe {HK_DEATHS+=1;}}
                            if let Some((damage,hazard,direction))=world::hazard_contact(r,&player,PARAMS) {
                                let hurt=vitals.hurt(VITAL_PARAMS,damage,direction,hazard);
                                if hurt==hk_sim::Hurt::Died {unsafe {HK_DEATHS+=1;}}
                                if hurt!=hk_sim::Hurt::Ignored {dialogue::cancel();audio::hurt();nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();}
                            }
                            if let Some(checkpoint)=world::checkpoint_record(r,&player,PARAMS) {
                                if let Some(id)=world::locate(r.scene,checkpoint.spawn[0],checkpoint.spawn[1]) {
                                    safe=(checkpoint.spawn[0],checkpoint.spawn[1],id);safe_facing=checkpoint.facing;
                                }
                            }
                            if gate_cooldown==0 && !vitals.dead && !vitals.hazard_pending {
                                if let Some(g)=world::gate(r.scene,&player) {
                                    if let Some(id)=world::locate(g.target_scene,g.spawn[0],g.spawn[1]) {
                                        geo.leave_scene(r.scene,geo::GEO_PARAMS);geo.reset_enemies(r.scene);
                                        vitals.add_blue_health(life.leave_scene());
                                        dialogue::cancel();state.reset_scene(r.scene);reveals.leave_scene();enemies.reset_scene(r.scene);player=Player::spawn(g.spawn[0],g.spawn[1]);
                                        nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();target=Some(id);safe=(player.x,player.y,id);safe_facing=player.facing;gate_cooldown=90;
                                    }
                                }
                            }
                            if target.is_none() && !vitals.dead && !vitals.hazard_pending && !world::contains(r.bounds,player.x,player.y) {
                                if let Some(id)=world::locate(r.scene,player.x,player.y) {target=Some(id);spatial_target=true;}
                                else {
                                    let hurt=vitals.hurt(VITAL_PARAMS,1,0,true);
                                    if hurt==hk_sim::Hurt::Died {unsafe {HK_DEATHS+=1;}}
                                    if hurt!=hk_sim::Hurt::Ignored {audio::hurt();focus.interrupt();nail=Nail::new();nail_response=hk_sim::NailResponse::new();}
                                }
                            }
                        }
                        reveals.tick(r.reveal_masks,&player,PARAMS);
                    }
                    if let Some((x,y,hp))=enemies.actor_state(0,12546) {unsafe {HK_CRAWLER_X=x;HK_CRAWLER_Y=y;HK_CRAWLER_HP=hp as i32;}}
                    let mut cut=0u32;for (i,p) in r.grass.iter().take(32).enumerate(){if state.cut(p.state){cut|=1<<i;}}
                    unsafe {HK_GRASS_CUT_MASK=cut;HK_NAIL_ATTACK_COUNT=attacks;HK_HEALTH=vitals.health as u32;HK_BLUE_HEALTH=vitals.blue_health as u32;HK_SOUL=vitals.soul as u32;HK_PLAYER_X=player.x;HK_PLAYER_Y=player.y;HK_FOCUS_LOCKED=u32::from(focus.locks_control());}
                    emit::stage_end(stage::UPDATE);emit::task_end(task::FIXED_UPDATE);ticks+=1;
                    input::checkpoint();
                }
            }
            render::finish_gpu_frame();fb.apply_draw_target();
            emit::counter(counter::SIM_TICKS,ticks);emit::stage_end(stage::PRESENT);emit::counter(counter::VISUAL_FRAMES,1);
            #[cfg(feature="emulator-telemetry")] if frame%30==0 {log_state(frame,&player,prims);}
            frame=frame.wrapping_add(1);
            let plan=wishlist(region_id,&player);
            let wanted=&plan.gpu[..plan.gpu_len];
            if let Some(id)=target {
                if vram.cancel_for_demand(id) {cache.protect_upload(None);}
                let required=plan.with_demand(id);
                cache.prefetch_regions(&required.ram[..required.ram_len]);
                let _=cache.request(id);
                // A resident destination can activate immediately. Do not put
                // an unrelated speculative upload on the transition path;
                // its source stays pinned until a later completed frame.
                if !vram.ready(id) {prepare_vram(cache,&mut vram,&[id]);}
                if vram.ready(id) && matches!(cache.try_select(id),Ok(true)) {
                    region_id=id;continue 'region;
                }
                pending_target=Some(id);
            } else if !paused {
                cache.prefetch_regions(&plan.ram[..plan.ram_len]);
                prepare_vram(cache,&mut vram,wanted);
            }
        }
    }
}
