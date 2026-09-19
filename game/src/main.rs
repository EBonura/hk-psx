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
mod bench;
mod camera;
mod save;
mod shade;
mod ability_art;
mod input_queue;
mod input_sampler;
mod input;
mod presentation;
mod scene_transition;
mod animation_cache;
mod audio;
mod focus_audio;
mod music;
mod ambience;
mod enemies;
mod hud;
mod world;
mod reveal_masks;
mod dialogue;
mod script;
mod impact;
mod disc;
mod room_decode;
mod texture_upload;
mod vram_cache;
mod menu;
mod pause;
mod charms;
mod cheats;
mod geo;
mod geo_render;
mod geo_audio;
mod runner_audio;
mod lifeblood;
mod great_door;
mod render;
mod alpha_scissor;
mod alpha_scissor_cache;
mod scenery_geometry;
mod draw_packet;
/// Canonical source admission remains closed until Runner audio and Charge Dust
/// have resident bindings. Native integration tests supply a recording sink.
/// Never admit an actor here and silently discard its presentation events.
#[no_mangle] pub static mut HK_RUNNER_DUST_EVENTS: u32 = 0;
#[no_mangle] pub static mut HK_CLIMBER_X: i32 = 0;
#[no_mangle] pub static mut HK_CLIMBER_Y: i32 = 0;
#[no_mangle] pub static mut HK_CLIMBER_HP: i32 = 0;
/// Runner presentation: audio through the resident Runner bank. Charge Dust is
/// counted, not drawn (recorded omission); Destroy releases the actor's voice.
fn runner_event(event: enemies::RunnerEvent) {
    use enemies::RunnerEventKind::*;
    match event.kind {
        AudioPlay => runner_audio::loop_start(event.source_id),
        AudioStop | Destroy => runner_audio::loop_stop(event.source_id),
        ChaseSound { pitch_q16, variant } => runner_audio::call(pitch_q16, variant),
        DustStart | DustStop => unsafe { HK_RUNNER_DUST_EVENTS = HK_RUNNER_DUST_EVENTS.saturating_add(1); },
        Fire { .. } => {} // spawned inside EnemyWorld::tick
    }
}
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
    let counts=state.apply(region.reveal_masks,world::reveal_bindings(region));
    unsafe {HK_REVEAL_MASKS_HIDDEN=counts.hidden;HK_REVEAL_MASKS_PARTIAL=counts.partial;HK_REVEAL_MASKS_VISIBLE=counts.visible;}
}
/// The ability clip that owns the Knight's body this frame, if any, and how
/// far into it we are. Ordered as the source's own states are: a cast or a
/// Dream Nail takes control outright, then the dash, then the wall, then the
/// wings. The Crystal Heart has no cooked clip and keeps the fall pose.
fn ability_pose(player:&Player,dream:&hk_sim::DreamNail,cast:&hk_sim::Cast)->Option<(usize,u32)> {
    use hk_sim::{CastPhase,DreamPhase};
    if cast.phase!=CastPhase::Off {
        let clip=if cast.phase==CastPhase::Antic {ability_art::FIREBALL_ANTIC} else {ability_art::FIREBALL_CAST};
        return Some((clip,cast.tick as u32));
    }
    if dream.phase!=DreamPhase::Off {
        // DN Start, DN Charge, DN Slash Antic and DN Slash sit in phase order.
        let offset=match dream.phase {DreamPhase::Start=>0,DreamPhase::Charge=>1,DreamPhase::Antic=>2,_=>3};
        return Some((ability_art::DN_START+offset,dream.tick as u32));
    }
    // The Crystal Heart owns the body through all four of its phases.
    match player.super_dash {
        hk_sim::SuperDash::Charging(t)=>return Some((if player.grounded {ability_art::SD_CHARGE_GROUND}
            else {ability_art::SD_WALL_CHARGE},t as u32)),
        hk_sim::SuperDash::Ready=>return Some((if player.grounded {ability_art::SD_CHARGE_GROUND}
            else {ability_art::SD_WALL_CHARGE},PARAMS.super_dash_charge_ticks as u32)),
        hk_sim::SuperDash::Travelling(t)=>return Some((ability_art::SD_DASH,t as u32)),
        hk_sim::SuperDash::Recovering(t)=>return Some((ability_art::SD_HIT_WALL,
            (PARAMS.super_dash_recover_ticks.saturating_sub(t)) as u32)),
        hk_sim::SuperDash::Off=>{}
    }
    if player.dash_left>0 {return Some((ability_art::DASH,(PARAMS.dash_ticks-player.dash_left) as u32));}
    if player.wall_locked {return Some((ability_art::WALLJUMP,player.wall_lock_ticks as u32));}
    if player.wall_sliding {return Some((ability_art::WALL_SLIDE,player.animation_tick));}
    if player.double_jumping {return Some((ability_art::DOUBLE_JUMP,player.double_jump_tick as u32));}
    None
}
fn knight_frame(room: &Room, player: &Player, focus: &hk_sim::Focus) -> usize {
    let (clip,age)=if let Some((clip,age))=focus.animation() {
        let offset=match clip {hk_sim::FocusClip::Focus=>0,hk_sim::FocusClip::Get=>1,hk_sim::FocusClip::End=>2,hk_sim::FocusClip::GetOnce=>3};
        (FOCUS_CLIP_BASE+offset,age)
    } else {(player.animation as usize,player.animation_tick)};
    clip_frame(room,clip,age)
}
/// Frame of a room clip at a 60 Hz age, honouring loop / loop-section / once.
fn clip_frame(room: &Room, clip: usize, age: u32) -> usize {
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
/// One room frame drawn at a fixed world point on the gameplay plane, for a
/// scene fixture that stands where it was authored rather than moving.
fn draw_at(room: &Room, frame: usize, position: [i32; 2], camera: (i32, i32)) -> u32 {
    // A frame too large for one animation slot is drawn as its tiles, each
    // covering its own share of the frame's world box.
    let (_, cols, rows) = room.frame_grid(frame);
    for tile in 0..cols * rows {
        let (texture, b) = room.frame_tile(frame, tile);
        let coords = [(b[0], b[3]), (b[2], b[3]), (b[0], b[1]), (b[2], b[1])];
        let mut v = [(0i16, 0i16); 4];
        for (k, (x, y)) in coords.iter().enumerate() {
            v[k] = ((160 + ((((position[0] + x - camera.0) >> 8) * KNIGHT_SCALE) >> 20)) as i16,
                    (120 - ((((position[1] + y - camera.1) >> 8) * KNIGHT_SCALE) >> 20)) as i16);
        }
        render::texture(texture, v, (128, 128, 128));
    }
    (cols * rows) as u32
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
pub(crate) static mut ROOM_CACHE: disc::Cache = unsafe { disc::Cache::new() };
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
fn strike_great_door(door:&mut great_door::World,scene:usize,nail:&Nail,player:&Player)->great_door::Strike {
    if !nail.hitting(ATTACK_PARAMS) {return great_door::Strike::default();}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    door.strike(scene,&polygon[..source.len()])
}
/// The Shade's body takes the nail through the same swept polygon as the rest.
#[inline(never)]
fn strike_shade(shade:&mut shade::World,nail:&Nail,player:&Player,damage:u16)->bool {
    if !nail.hitting(ATTACK_PARAMS) {return false;}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    shade.strike(&polygon[..source.len()],damage)
}
/// NailSlash.OnTriggerEnter2D's shroom branch. Only the down slash reaches it,
/// and the hero answers with ShroomBounce instead of the ordinary Bounce, so
/// this takes the contact before the pogo targets get a look at it.
#[inline(never)]
fn strike_shroom(region:&world::Region,nail:&Nail,player:&mut Player)->bool {
    if nail.kind!=3 || !nail.hitting(ATTACK_PARAMS) {return false;}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    for shroom in world::shrooms(region) {
        if hk_sim::polygon_hits_box(&polygon[..source.len()],shroom.bounds) {
            player.shroom_bounce(PARAMS);
            return true;
        }
    }
    false
}
/// A dash is refused for the same reasons a jump is: scripted control, a
/// dialogue, a door sequence or the bench all own the Knight.
fn locked_for_dash(focus:&hk_sim::Focus,door:&great_door::World,bench:&bench::State)->bool {
    focus.locks_control()||dialogue::open()||door.pending()||bench.locks_control()
}
/// Everything that follows a hit the Knight actually took. Four call sites used
/// to repeat this and the fifth, falling out of the world, had drifted and lost
/// the dialogue cancel, so a dialogue open at that moment survived the hazard
/// respawn when every other kind of damage ended it. See docs/DAMAGE.md.
#[inline(never)]
fn respond_to_hurt(hurt:hk_sim::Hurt,nail:&mut Nail,response:&mut hk_sim::NailResponse,
                   focus:&mut hk_sim::Focus) {
    if hurt==hk_sim::Hurt::Died {unsafe {HK_DEATHS+=1;}}
    if hurt==hk_sim::Hurt::Ignored {return;}
    dialogue::cancel();
    audio::hurt();
    *nail=Nail::new();
    *response=hk_sim::NailResponse::new();
    focus.interrupt();
    focus_audio::interrupt();
}
/// Every source of damage to the Knight goes through here, so a new one cannot
/// take the damage and forget the response. `hazard` routes to the respawn
/// marker instead of a recoil, and the Shade Cloak's immunity is read from the
/// player rather than passed, because no caller may forget that either.
#[inline(never)]
fn apply_hurt(cheats:&cheats::Settings,vitals:&mut hk_sim::Vitals,player:&Player,
              nail:&mut Nail,response:&mut hk_sim::NailResponse,focus:&mut hk_sim::Focus,
              damage:u16,direction:i32,hazard:bool)->hk_sim::Hurt {
    let hurt=cheats.hurt(vitals,VITAL_PARAMS,damage,direction,hazard,player.shadow_dashing);
    respond_to_hurt(hurt,nail,response,focus);
    hurt
}
/// StartSoulLimiter caps the vessel at 66 while a Shade is owed.
fn soul_cap(shade:&shade::World)->u16 {
    if shade.soul_limited() {66} else {VITAL_PARAMS.max_soul}
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
#[no_mangle]
fn main() {
    gpu::init(VideoMode::Ntsc, Resolution::R320X240);
    psx_rt::interrupts::install_vblank_counter();
    let mut fb=FrameBuffer::new(320,240);
    gpu::set_draw_area(0,0,319,239);gpu::set_draw_offset(0,0);
    // The card is read before the pad sampler exists: SIO0 is not yet shared.
    let (slots,card_fault)=save::survey();
    // Each slot's one line on the save screen. Geo and masks are what the
    // record holds; the source also shows play time, completion and the map
    // zone, none of which this port tracks.
    let mut slot_text=[[0u8;16];save::PROFILES];
    let mut slot_used=[0usize;save::PROFILES];
    for (i,slot) in slots.iter().enumerate() {
        if let save::Slot::Used(save)=slot {
            let line=&mut slot_text[i];let mut at=0;
            for b in b"Geo " {line[at]=*b;at+=1;}
            let mut geo=save.geo.min(9999);let mut digits=[0u8;4];let mut n=0;
            loop {digits[n]=b'0'+(geo%10) as u8;geo/=10;n+=1;if geo==0 {break;}}
            while n>0 {n-=1;line[at]=digits[n];at+=1;}
            slot_used[i]=at;
        }
    }
    let mut slot_lines=["Empty";save::PROFILES];
    for (i,slot) in slots.iter().enumerate() {
        slot_lines[i]=match slot {
            save::Slot::Empty=>"Empty",
            save::Slot::Corrupt=>"Damaged",
            save::Slot::Used(_)=>core::str::from_utf8(&slot_text[i][..slot_used[i]]).unwrap_or("Used"),
        };
    }
    let profile_fault=card_fault.map_or("",|f|f.message());
    audio::init();let (mut settings,profile)=menu::run(&mut fb,&slot_lines,profile_fault);
    focus_audio::set_volume(settings.sfx);menu::loading(&mut fb);
    let saved=match slots[profile] {save::Slot::Used(s)=>Some(s),_=>None}
        // A record from another disc build may name a slot this catalogue lacks.
        .filter(|s|(s.region as usize)<world::REGION_SCENES.len()&&world::scene_of(s.region as usize)==s.scene as usize);
    // The charm board before the first Vitals, because Fragile Heart raises
    // max_health. Taken from the record itself rather than from `saved`: a
    // stale spawn point is no reason to throw the collection away.
    charms::boot(match slots[profile] {save::Slot::Used(s)=>
        Some((s.charms_owned,s.charms_equipped,s.charm_notches,s.can_overcharm)),_=>None},settings.cheats.charms);
    // Each NPC's conversation cursor, from the record for the same reason: a
    // stale spawn point is no reason to make Elderbug introduce himself again.
    if let save::Slot::Used(s)=slots[profile] {dialogue::restore_met(s.npc_conversations);
        // And the PlayerData the cooked scripts wrote, if the record was
        // written by this bank. `script::boot` drops it when it was not.
        script::boot(&s.script_fields,s.script_field_fnv);}
    let cache=unsafe { &mut *(&raw mut ROOM_CACHE) };
    let mut state=world::State::new();
    let geo=unsafe {&mut *(&raw mut GEO_WORLD)};
    let life=unsafe {&mut *(&raw mut LIFE_WORLD)};
    let mut door=great_door::World::new();
    let mut reveals=reveal_masks::State::new();
    let mut shade=shade::World::new();
    let mut enemies=enemies::EnemyWorld::new();
    let mut player=Player::spawn(SPAWN.0,SPAWN.1);
    let mut vitals=settings.cheats.new_vitals(VITAL_PARAMS);cheats::publish(settings.cheats,VITAL_PARAMS);
    let mut nail=Nail::new();
    let mut focus=hk_sim::Focus::new();
    let mut nail_response=hk_sim::NailResponse::new();
    let mut dream=hk_sim::DreamNail::new();
    let mut cast=hk_sim::Cast::new();
    let mut region_id=0usize;
    let mut frame=0u32;let mut attacks=0u32;
    let mut safe=(SPAWN.0,SPAWN.1,0usize);
    let mut safe_facing=1;
    // Pending Save Game record, written at the frame boundary once the player
    // accepts the bench prompt. Nothing else ever writes the card.
    let mut save_requested:Option<save::Save>=None;
    // Row 0 is Yes, row 1 is No, while the seated save prompt is open.
    let mut save_prompt:Option<u8>=None;
    let mut gate_cooldown=0u16;
    // Ticks since the current scene started (source Delay Collider gates).
    let mut scene_ticks=0u32;let mut scene_ticks_scene=usize::MAX;
    let mut paused=false;let mut start_held=false;
    let mut pause_menu=pause::State::new();
    let mut vram=vram_cache::Cache::new();
    let mut sim_clock=psx_rt::interrupts::vblank_count();
    let mut initial=true;
    let mut camera_state=camera::Camera::new();
    let mut bench_state=bench::State::new();
    // Bench respawn marker: scene, seat, facing, catalogue slot. None means the new-game spawn.
    let mut respawn:Option<(usize,[i32;2],i32,usize)>=None;
    // Continue: stand at the saved bench with the saved wallet and Great Door hits.
    if let Some(s)=saved {
        respawn=Some((s.scene as usize,s.seat,s.facing,s.region as usize));
        region_id=s.region as usize;player=Player::spawn(s.seat[0],s.seat[1]);player.facing=s.facing;
        safe=(s.seat[0],s.seat[1],region_id);safe_facing=s.facing;
        geo.restore_wallet(s.geo,geo::GEO_PARAMS);door.restore(s.door_hits);
        shade.restore(s.shade);
    }
    let mut prev_pad=ButtonState::from_bits(0);
    // Timer1 counts HBlanks for frame-pacing telemetry only.
    psx_io::timers::set_mode(psx_io::timers::Timer::Timer1,0x0100);
    'region: loop {
        if initial {
            unsafe { HK_GAME_MODE=2; }
            gpu::draw_sync();
            while cache.prepare_ambience().and_then(|_|cache.select(region_id)).is_err() {
                unsafe { HK_GAME_MODE=3; }
                vram=vram_cache::Cache::new();cache.reset_bootstrap();
                menu::restore();menu::retry(&mut fb);menu::loading(&mut fb);
            }
            // Atlas bytes were verified/uploaded before the initial scene
            // was admitted. Every spatial view in this scene stays live.
            vram.admit_scene(region_id,cache.atlases_ready());
            vram.activate(region_id);
            hud::upload();dialogue::upload();geo_render::upload();lifeblood::upload();shade::upload();ability_art::upload();geo_audio::upload();geo_audio::set_volume(settings.sfx);runner_audio::upload();runner_audio::set_volume(settings.sfx);
            initial=false;
            sim_clock=psx_rt::interrupts::vblank_count();
            input::start(sim_clock);
        }
        // Region entry follows a scene load or a boundary crossing without a
        // poll since the last frame. Service the pad around each bank-driven
        // pass so no stretch between polls approaches a VBlank.
        input::checkpoint();
        let r=&world::resident(region_id).expect("admitted world metadata covers the selected region");
        let meta_region=cache.world_region_index(region_id).expect("admitted world metadata covers the selected region");
        assert!(ambience::set_scene(r.scene as u8));
        // The view's cooked NPC, read once: the bank record cannot change while
        // this region is resident, and the tick and the draw both want it.
        // One per view, which host/regions.py refuses to exceed.
        let region_npc=world::npcs(r).next();
        enemies.sync_region(r);
        reveals.scene_ready(r.scene,r.reveal_masks);
        input::checkpoint();
        let bank=vram.activate(region_id);
        render::init(&cache.room(),bank,region_id,cache.coverage());
        input::checkpoint();
        state.apply(r);apply_reveal_masks(&reveals,r);
        input::checkpoint();
        geo_render::apply(geo,&mut state,region_id);
        lifeblood::apply(life,&mut state,region_id);
        great_door::apply(&door,&mut state,region_id);
        unsafe { HK_GAME_MODE=1;HK_REGION_ID=region_id as u32+1; }
        if scene_ticks_scene!=r.scene {scene_ticks=0;scene_ticks_scene=r.scene;}
        shade.enter_scene(r.scene);
        // The first frame renders before the first tick: seat the camera now
        // (a scene gate or respawn snaps through the displacement rule).
        camera_state.tick((player.x,player.y),player.facing,camera::limits(r,[player.x-PARAMS.half_width,player.y+PARAMS.bottom,player.x+PARAMS.half_width,player.y+PARAMS.top]));
        let mut pending_target:Option<usize>=None;
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
            great_door::apply(&door,&mut state,region_id);
            let camera=camera_state.position();
            let door_frame=door.frame(region_id,camera);
            // The bench owns the body first, then an ability, then the room clip.
            let pose=if bench_state.animation().is_some() {None} else {ability_pose(&player,&dream,&cast)};
            let body_frame=if let Some((clip,age))=bench_state.animation() {clip_frame(&room,clip,age)} else {knight_frame(&room,&player,&focus)};
            let effect_frame=nail_frame(&room,&nail);
            let enemy_draws=enemies.prepare_draws(r,&room,camera);
            let ability_frame=pose.map(|(clip,age)|ability_art::frame_index(clip,age));
            // Vengeful Spirit's projectile carries its own frame and key.
            let ball_frame=cast.ball_bounds(FIREBALL_PARAMS).map(|_|
                ability_art::frame_index(ability_art::BALL,cast.ball.life as u32));
            // The view's NPC, whose clips ride in this view's own room bank.
            let npc_frame=region_npc.map(|npc|{
                // Conversation Control reaches Talk Right through Hero Is Right
                // and Talk Left through Hero Is Left, because the NPC's own
                // localScale.x is positive; Idle is what it stands in.
                let slot=if dialogue::npc_speaking()==Some(npc.source_id) {
                    if player.x>=npc.position[0] {2} else {1}
                } else {0};
                (npc.position,clip_frame(&room,npc.clip_base as usize+slot,sim_clock))
            });
            let mut needed=[0u16;12];
            needed[0]=match ability_frame {
                Some(i)=>ability_art::KEY_BASE+i as u16,
                None=>u32_at(room.frame(body_frame),0) as u16,
            };
            let mut needed_len=if effect_frame.is_some(){2}else{1};
            if let Some(f)=effect_frame {needed[1]=u32_at(room.frame(f),0) as u16;}
            enemy_draws.append_needed(&mut needed,&mut needed_len);
            shade.append_needed(&mut needed,&mut needed_len);
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
                render::append_frame_keys(&room,f,&mut needed,&mut needed_len);
            }
            render::begin(&room,&needed[..needed_len],fb.buffer_y(fb.drawing));
            fb.clear(9,13,20);let mut prims=render::scenery(camera,false);prims+=enemy_draws.draw();render::kick_ready();prims+=state.draw_impacts(r,&room,camera);
            if let Some(f)=door_frame {prims+=great_door::draw(&room,f,camera);}
            prims+=shade.draw(camera);
            prims+=geo_render::draw(geo,region_id,camera);
            prims+=lifeblood::draw(life,region_id,camera);render::kick_ready();
            // Behind the Knight: the NPC sits a hair further from the camera.
            if let Some((position,f))=npc_frame {prims+=draw_at(&room,f,position,camera);}
            if !vitals.dead && !door.hidden() && (vitals.invulnerable_ticks==0 || frame%4<2) {
                prims+=match ability_frame {
                    Some(i)=>ability_art::draw(i,player.x,player.y,player.facing,camera),
                    None=>{draw_frame(&room,body_frame,&player,camera);1}
                };
            }
            if let Some(f)=effect_frame {draw_frame(&room,f,&player,camera);prims+=1;}
            if let Some(i)=ball_frame {
                prims+=ability_art::draw(i,cast.ball.x,cast.ball.y,cast.ball.facing,camera);
            }
            prims+=render::scenery(camera,true);dialogue::prepare(camera,geo.wallet(),paused.then_some((&pause_menu,&settings)),settings.cheats.params(VITAL_PARAMS).max_health.saturating_add(vitals.blue_health),save_prompt);render::submit(vitals.health,settings.cheats.params(VITAL_PARAMS).max_health,vitals.soul,soul_cap(&shade),paused,vitals.blue_health,door.shade());
            emit::counter(counter::TRI_PRIMITIVES,prims*2);emit::stage_end(stage::RENDER);
            emit::stage_begin(stage::PRESENT);
            let display=fb.begin_deferred_swap();
            presentation::begin(display);
            let mut ticks=0;
            // Idle time before the next simulation tick can already prepare
            // the current camera/state. A later changed state rejects/replaces
            // it through the same exact-key validation as any other prefix.
            let mut coverage_clock=sim_clock.wrapping_sub(1);
            loop {
                // An earlier chunk may still be walking: kick the final list as
                // soon as the channel is free, then queue the display only
                // after that final list (including HUD) has reached GP0; the
                // IRQ then also waits for raster completion at a true VBlank edge.
                presentation::checkpoint();
                // A completed flip is not permission to start another expensive
                // render while sampled input still waits for simulation. Drain
                // the observed VBlanks first, including ticks spent rendering.
                if presentation::queued() && !psx_rt::interrupts::gp1_queue_pending()
                    && sim_clock==psx_rt::interrupts::vblank_count() {break;}
                input::checkpoint();
                let _=cache.pump(); // Detailed errors remain in the CD markers.
                input::checkpoint();
                let now=psx_rt::interrupts::vblank_count();
                while sim_clock!=now {
                    sim_clock=sim_clock.wrapping_add(1);
                    let pad=ButtonState::from_bits(input::consume(sim_clock));
                    // The source scene has ended. A rendered-frame catch-up must
                    // not simulate the destination player against outgoing terrain.
                    if target.is_some_and(|id|world::scene_of(id)!=r.scene) {
                        start_held=pad.is_held(button::START);player.was_jump=pad.is_held(button::CROSS);
                        continue;
                    }
                    let can_inspect=target.is_none() && !paused && !door.pending() && !vitals.dead && player.grounded && vitals.can_control() && !nail.active && !focus.locks_control();
                    dialogue::tick(r.scene,&player,PARAMS,can_inspect,pad.bits());
                    // npc_control's own range trigger, cooked into the view's
                    // bank: the hero body inside it is what UP answers.
                    let body=[player.x-PARAMS.half_width,player.y+PARAMS.bottom,player.x+PARAMS.half_width,player.y+PARAMS.top];
                    let npc_in_range=region_npc.filter(|n|
                        n.bounds[0]<=body[2] && n.bounds[2]>=body[0] && n.bounds[1]<=body[3] && n.bounds[3]>=body[1]
                    ).map(|n|n.source_id);
                    dialogue::npc_tick(r.scene,npc_in_range,can_inspect,pad.bits());
                    // The cooked FSM programs. Their trigger volumes take the
                    // same hero body the NPC range does, so a script and an NPC
                    // standing in the same place agree on where the Knight is.
                    script::tick(r.scene,body);
                    ambience::tick();
                    let start=pad.is_held(button::START);
                    let mut ui_consumed=paused;
                    if start&&!start_held && !dialogue::open() {
                        paused=!paused;ui_consumed=true;
                        if paused {pause_menu.enter(pad.bits());}
                    }
                    start_held=start;
                    if paused {
                        let before=(settings.sfx,settings.ambience,settings.music);let previous_cheats=settings.cheats;
                        if pause_menu.step(pad.bits(),&mut settings.sfx,&mut settings.ambience,&mut settings.music,&mut settings.cheats) {
                            paused=false;player.was_jump=pad.is_held(button::CROSS);
                        }
                        settings.cheats.apply(previous_cheats,&mut vitals,VITAL_PARAMS,pause_menu.action);
                        if before!=(settings.sfx,settings.ambience,settings.music) {audio::set_volume(settings.sfx);focus_audio::set_volume(settings.sfx);geo_audio::set_volume(settings.sfx);runner_audio::set_volume(settings.sfx);ambience::set_volume(settings.ambience);music::set_volume(settings.music);}
                    }
                    if paused || dialogue::open() || (door.pending() && !door.entering()) || vitals.dead || !vitals.can_control() || focus.locks_control() {audio::stop_footsteps();}
                    settings.cheats.maintain(&mut vitals,VITAL_PARAMS);cheats::publish(settings.cheats,VITAL_PARAMS);
                    // StartSoulLimiter lowers maxMP itself while a Shade is owed.
                    vitals.soul=vitals.soul.min(soul_cap(&shade));
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
                        door.reset();
                        dialogue::reset();script::reset();state=world::State::new();reveals=reveal_masks::State::new();enemies=enemies::EnemyWorld::new();vitals=settings.cheats.new_vitals(VITAL_PARAMS);
                        audio::reset_movement();player=Player::spawn(SPAWN.0,SPAWN.1);safe=(SPAWN.0,SPAWN.1,0);safe_facing=1;
                        nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();focus_audio::interrupt();attacks=0;target=Some(0);paused=false;
                        unsafe {HK_BREAK_COUNT=0;HK_DEATHS=0;HK_ENEMY_HITS=0;HK_ENEMY_KILLS=0;HK_HAZARD_RESPAWNS=0;HK_FOCUS_STARTED=0;HK_FOCUS_COMPLETED=0;HK_FOCUS_HEALED=0;HK_FOCUS_DRAINED=0;HK_FOCUS_REFUNDED=0;}
                    } else if !paused && !ui_consumed && !vitals.tick() {
                        gate_cooldown=gate_cooldown.saturating_sub(1);
                        scene_ticks=scene_ticks.saturating_add(1);
                        state.tick();
                        state.tick_debris(r, &room);
                        presentation::checkpoint();
                        if vitals.needs_death_respawn() {
                            // Hero Death Anim: Remove Geo moves the wallet into the pool the
                            // Shade carries and Set Shade records where and how strong it is.
                            // The source also saves here; this port never writes the card on
                            // its own, so the Shade reaches it at the next bench the player
                            // chooses to save at.
                            let params=settings.cheats.params(VITAL_PARAMS);
                            shade.record_death(r.scene,[player.x,player.y],
                                shade::death_health(params.max_health,params.nail_damage),geo.wallet());
                            unsafe {HK_GEO_LOST=HK_GEO_LOST.saturating_add(geo.death());}
                            geo.leave_scene(r.scene,geo::GEO_PARAMS);geo.reset_enemies(r.scene);
                            life.reset();
                            if !door.opened() {door.reset();}
                            dialogue::cancel();state.reset_scene(r.scene);reveals.reset_scene(r.scene);enemies.reset_scene(r.scene);runner_audio::reset();vitals=settings.cheats.new_vitals(VITAL_PARAMS);
                            // Respawn at the last bench (standing at its seat) or the new-game spawn.
                            let (spawn,facing,slot)=respawn.map_or(((SPAWN.0,SPAWN.1),1,0),|(_,seat,facing,slot)|((seat[0],seat[1]),facing,slot));
                            audio::reset_movement();player=Player::spawn(spawn.0,spawn.1);player.facing=facing;safe=(spawn.0,spawn.1,slot);safe_facing=facing;bench_state.reset();
                            nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();focus_audio::interrupt();target=Some(slot);
                        } else if vitals.hazard_pending {
                            dialogue::cancel();
                            unsafe {HK_HAZARD_RESPAWNS+=1;}
                            audio::reset_movement();player=Player::spawn(safe.0,safe.1);player.facing=safe_facing;nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();focus_audio::interrupt();
                            vitals.finish_hazard_respawn(settings.cheats.params(VITAL_PARAMS));target=Some(safe.2);
                        } else if !vitals.dead {
                            let door_exit=door.tick(r.scene);
                            let read_consumes=dialogue::consumes_actions();
                            // Source Spell Control: the cast button waits Button Down Time
                            // for a release. Let go inside that and it casts; keep holding
                            // and the Focus starts instead. One button, two moves.
                            let cast_button=pad.is_held(button::CIRCLE)&&!read_consumes&&vitals.can_control();
                            let cast_blocked=!vitals.can_control()||door.pending()||read_consumes;
                            cast.has_fireball=settings.cheats.spell;
                            cast.tick(FIREBALL_PARAMS,FIREBALL_ANTIC_TICKS,FIREBALL_CAST_TICKS,
                                cast_button,&mut vitals.soul,&player,cast_blocked);
                            cast.stop_at_wall(FIREBALL_PARAMS,room.counts[5],state.edge_reader(r,&room));
                            let focus_events=focus.step(FOCUS_PARAMS,settings.cheats.params(VITAL_PARAMS),hk_sim::FocusInput {
                                held:cast_button&&cast.held_past_tap(FIREBALL_PARAMS)&&!cast.locks_control(),grounded:player.grounded,
                                can_start:vitals.can_control() && !door.pending() && (!nail.active || nail.age>=FOCUS_PARAMS.attack_recovery_ticks),
                            },&mut vitals);
                            focus_audio::tick(&focus,focus_events);
                            settings.cheats.maintain(&mut vitals,VITAL_PARAMS);
                            unsafe {
                                HK_FOCUS_STARTED+=u32::from(focus_events.started);HK_FOCUS_COMPLETED+=u32::from(focus_events.completed);
                                HK_FOCUS_HEALED+=u32::from(focus_events.healed);HK_FOCUS_DRAINED+=u32::from(focus_events.drained);
                                HK_FOCUS_REFUNDED+=u32::from(focus_events.refunded);
                            }
                            if focus_events.started {
                                nail=Nail::new();nail_response=hk_sim::NailResponse::new();player.vy=0;player.jumping=false;
                            }
                            let pressed=|b:u16| pad.is_held(b)&&!prev_pad.is_held(b);
                            // Mothwing Cloak. Nothing in the admitted scenes grants it, so
                            // the cheat is the only source until the pickups exist.
                            player.has_dash=settings.cheats.dash;
                            player.has_walljump=settings.cheats.claw;
                            player.has_double_jump=settings.cheats.wings;
                            player.has_super_dash=settings.cheats.heart;
                            player.has_shade_cloak=settings.cheats.cloak;
                            player.super_dash_input(PARAMS,pad.is_held(button::R1)&&vitals.can_control()&&!locked_for_dash(&focus,&door,&bench_state));
                            player.dash_input(PARAMS,pad.is_held(button::L1)&&vitals.can_control()&&!locked_for_dash(&focus,&door,&bench_state));
                            // The seated prompt owns input while it is up, so the bench
                            // neither reads a leave press nor starts another rest.
                            let asking=save_prompt.is_some();
                            if let Some(row)=save_prompt {
                                if pressed(button::UP)||pressed(button::DOWN) {save_prompt=Some(1-row);}
                                else if pressed(button::CROSS) {
                                    if row==0 {
                                        let stand=[bench_state.seat()[0],player.y];
                                        let (owned,equipped,notches,can_overcharm)=charms::record();
                                        let (script_fields,script_field_fnv)=script::record();
                                        save_requested=Some(save::Save{scene:r.scene as u32,seat:stand,facing:player.facing,
                                            region:region_id as u32,geo:geo.wallet(),door_hits:door.hits(),
                                            shade:shade.record(),sequence:0,charms_owned:owned,
                                            charms_equipped:equipped,charm_notches:notches,can_overcharm,
                                            npc_conversations:dialogue::met_bits(),script_fields,script_field_fnv});
                                    }
                                    save_prompt=None;
                                } else if pressed(button::CIRCLE) {save_prompt=None;}
                            }
                            let bench_events={
                                let hero_body=[player.x-PARAMS.half_width,player.y+PARAMS.bottom,player.x+PARAMS.half_width,player.y+PARAMS.top];
                                let free=!(focus.locks_control()||dialogue::open()||door.pending()||read_consumes||asking)&&vitals.can_control()&&!nail.active;
                                let leave=!asking&&[button::CROSS,button::SQUARE,button::LEFT,button::RIGHT,button::UP,button::DOWN].iter().any(|&b|pressed(b));
                                let grounded=player.grounded;
                                bench_state.tick(r,hero_body,&mut player,grounded,free,!asking&&pressed(button::UP),leave)
                            };
                            if bench_events.rest {
                                // Rest Burst: HERO REVIVED and the respawn marker, then ask.
                                // The source saves here without asking; this port never
                                // writes the card unless the player says so.
                                vitals.heal(settings.cheats.params(VITAL_PARAMS),u16::MAX);
                                // Sitting keeps the Knight's floor y; the source seat y is the sprite's, and a
                                // body spawned there starts inside the floor and falls through it.
                                let stand=[bench_state.seat()[0],player.y];
                                respawn=Some((r.scene,stand,player.facing,region_id));
                                save_prompt=Some(0);
                            }
                            let locked=focus.locks_control()||dialogue::open()||door.pending()||bench_state.locks_control()||save_prompt.is_some()||cast.locks_control();
                            // Take Control: the Dream Nail owns the Knight from Start to End.
                            dream.has_dream_nail=settings.cheats.dream;
                            dream.tick(DREAM_NAIL_PARAMS,pad.is_held(button::TRIANGLE)&&vitals.can_control(),&player,locked);
                            let locked=locked||dream.locks_control();
                            let dir=if door.entering() {door.forced_direction()}else if locked {0}else{i32::from(pad.is_held(button::RIGHT))-i32::from(pad.is_held(button::LEFT))};
                            let was_grounded=player.grounded;
                            let jump=!locked && !read_consumes && pad.is_held(button::CROSS);
                            let could_jump=was_grounded && !player.was_jump && jump && vitals.can_control() && nail_response.bounce_left==0;
                            if door.drop_starts() {player.vy=-12*ONE;}
                            if door.apply_entry(&mut player) {
                                // Source entry owns position/animation while input is locked.
                            } else if bench_state.locks_control() {
                                // Seated or getting off: the bench owns position and animation.
                                player.grounded=true;player.jumping=false;player.was_jump=pad.is_held(button::CROSS);
                            } else if let Some((vx,vy))=vitals.recoil_velocity(VITAL_PARAMS) {
                                let mut p=PARAMS;p.speed=vx.abs();p.gravity=0;
                                player.vy=vy;player.jumping=false;
                                player.step(p,vx.signum(),false,room.counts[5],state.edge_reader(r,&room));
                                player.facing=-vitals.recoil_direction;
                            } else {
                                if could_jump {audio::jump();}
                                nail_response.step(NAIL_RESPONSE_PARAMS,PARAMS,&mut player,dir,jump,room.counts[5],state.edge_reader(r,&room));
                            }
                            audio::movement_tick(was_grounded,player.grounded,player.vy<0,
                                (door.entry_footsteps() || (dir!=0 && !locked)) && vitals.can_control() && vitals.recoil_velocity(VITAL_PARAMS).is_none(),true);
                            let vertical=i32::from(pad.is_held(button::UP))-i32::from(pad.is_held(button::DOWN));
                            if nail.tick(ATTACK_PARAMS,pad.is_held(button::SQUARE)&&vitals.can_control()&&!locked&&!read_consumes,vertical,&mut player) {attacks=attacks.wrapping_add(1);geo.begin_attack();audio::nail();}
                            // HeroController.Bounce returns early while cState.shroomBouncing,
                            // so an ordinary pogo cannot cut a shroom's rise short.
                            if vitals.can_control() && !strike_shroom(r,&nail,&mut player) && !player.shroom_bouncing {
                                enemies::pogo_contact(r,&state,&mut player,&nail,ATTACK_PARAMS,NAIL_POLYGONS,&mut nail_response);
                            }
                            let strike=state.strike(r,&nail,ATTACK_PARAMS,NAIL_POLYGONS,&player);
                            unsafe {HK_BREAK_COUNT=HK_BREAK_COUNT.saturating_add(strike.broken as u32);}
                            if strike.door_sounds>0 {audio::door();}
                            let mining=strike_geo(geo,r.scene,&nail,&player);
                            if mining.hits>0 {geo_audio::hit();}
                            if mining.depleted>0 {geo_audio::break_rock();geo_render::apply(geo,&mut state,region_id);}
                            // A break or depleted rock invalidated the edge table; rebuild it now so the
                            // rest of this tick (enemies, coins, Lifeblood) does not fall back to the
                            // per-query breakable scan, which cost several VBlanks with three coins.
                            if strike.broken>0 || mining.depleted>0 {state.refresh_edges(r,&room);}
                            presentation::checkpoint();
                            let blue=life.tick(r.scene,player.x,r.collision_bounds,room.counts[5],state.edge_reader(r,&room));
                            vitals.add_blue_health(blue);
                            if strike_lifeblood(life,r.scene,&nail,&player).opened {lifeblood::apply(life,&mut state,region_id);}
                            let door_hit=strike_great_door(&mut door,r.scene,&nail,&player);
                            if door_hit.hit {audio::great_door_hit();}
                            if door_hit.opened {
                                great_door::apply(&door,&mut state,region_id);
                                dialogue::cancel();focus.interrupt();focus_audio::interrupt();
                            }
                            {
                                let hero_body=[player.x-PARAMS.half_width,player.y+PARAMS.bottom,player.x+PARAMS.half_width,player.y+PARAMS.top];
                                camera_state.tick((player.x,player.y),player.facing,camera::limits(r,hero_body));
                            }
                            if shade.present_here() {
                                let hero_body=[player.x-PARAMS.half_width,player.y+PARAMS.bottom,player.x+PARAMS.half_width,player.y+PARAMS.top];
                                if strike_shade(&mut shade,&nail,&player,settings.cheats.params(VITAL_PARAMS).nail_damage) {audio::enemy_hit();}
                                let shade_position=shade.position().unwrap_or([player.x,player.y]);
                                let shade_events=shade.tick([player.x,player.y],hero_body,room.counts[5],state.edge_reader(r,&room));
                                if shade_events.touched {
                                    let direction=if player.x<shade_position[0] {-1}else{1};
                                    apply_hurt(&settings.cheats,&mut vitals,&player,&mut nail,&mut nail_response,&mut focus,1,direction,false);
                                }
                                if shade_events.returned_geo>0 {geo.add_quietly(shade_events.returned_geo,geo::GEO_PARAMS);geo_audio::pickup(1);}
                            }
                            let enemy_events=enemies.tick(r,&room,&mut state,&mut player,&mut vitals,&nail,&dream,cast.ball_bounds(FIREBALL_PARAMS).map(|b|(b,FIREBALL_PARAMS.damage)),&mut nail_response,ATTACK_PARAMS,NAIL_POLYGONS,settings.cheats,
                                [camera.0,camera.1,-2496922],runner_event,
                                |scene,x,y| {let i=cache.locate(scene,x,y)?;Some((world::resident(i)?,cache.room_for(i)?))});
                            enemies.take_geo_deaths(r.scene,|source,x,y|geo.spawn_enemy(r.scene,source,[x,y],geo::GEO_ENEMIES));
                            let h=geo::HERO_BOX;
                            let collected=geo.tick(r.scene,geo::GEO_PARAMS,[player.x+h[0],player.y+h[1],player.x+h[2],player.y+h[3]],
                                r.collision_bounds,room.counts[5],state.edge_reader(r,&room));
                            if collected.collected>0 {geo_audio::pickup(collected.last_value);}
                            presentation::checkpoint();
                            unsafe {HK_ENEMY_HITS+=enemy_events.hits as u32;HK_ENEMY_KILLS+=enemy_events.kills as u32;HK_SOUL=vitals.soul as u32;}
                            if enemy_events.dream_soul>0 {vitals.add_soul(VITAL_PARAMS,enemy_events.dream_soul);}
                            if enemy_events.hits>0 {audio::enemy_hit();}
                            respond_to_hurt(enemy_events.hurt,&mut nail,&mut nail_response,&mut focus);
                            let bank_region=cache.world_metadata().region(meta_region).expect("admitted world metadata covers the selected region");
                            if let Some((damage,hazard,direction))=world::hazard_contact(bank_region,&player,PARAMS) {
                                apply_hurt(&settings.cheats,&mut vitals,&player,&mut nail,&mut nail_response,&mut focus,damage,direction,hazard);
                            }
                            if let Some((spawn,facing))=world::checkpoint(bank_region,&player,PARAMS) {
                                if let Some(id)=cache.locate(r.scene,spawn[0],spawn[1]) {
                                    safe=(spawn[0],spawn[1],id);safe_facing=facing;
                                }
                            }
                            if gate_cooldown==0 && (door_exit || !door.pending()) && !vitals.dead && !vitals.hazard_pending {
                                let gate=if door_exit {
                                    Some(world::GATES.iter().find(|g|g.scene==r.scene&&g.target_scene==great_door::TARGET_SCENE).expect("Great Door exit gate"))
                                } else {world::gate(r.scene,&player,vitals.recoil_ticks!=0,scene_ticks)};
                                if let Some(g)=gate {
                                    let side_entry=(g.scene==0&&g.target_scene==great_door::TARGET_SCENE)
                                        ||(g.scene==great_door::TARGET_SCENE&&g.target_scene==0);
                                    let (spawn,id)=if side_entry {
                                        let entry=if g.target_scene==0 {&great_door::RETURN_ENTRY}else{&great_door::ENTRY};
                                        (entry.spawn,entry.region)
                                    } else {(g.spawn,g.target_region)};
                                    {let id=id;
                                        geo.leave_scene(r.scene,geo::GEO_PARAMS);geo.reset_enemies(r.scene);
                                        vitals.add_blue_health(life.leave_scene());
                                        dialogue::cancel();state.reset_scene(r.scene);reveals.leave_scene();enemies.reset_scene(r.scene);runner_audio::reset();audio::reset_movement();player=Player::spawn(spawn[0],spawn[1]);
                                        // The Great Door pair walks in through its audited entries; every
                                        // other side gate uses the same source sequence toward the scene
                                        // interior (a right exit enters at the destination's left gate).
                                        if side_entry {
                                            if door_exit {door.begin_entry(g.target_scene);}else{door.begin_gate_entry(g.target_scene,if g.target_scene==0 {-1}else{1});}
                                            player.grounded=true;
                                        } else if g.side==1||g.side==2 {
                                            door.begin_gate_entry(g.target_scene,if g.side==2 {1}else{-1});
                                            player.facing=if g.side==2 {1}else{-1};player.grounded=true;
                                        } else if g.side==3 {
                                            door.begin_top_entry(g.target_scene);
                                        } else {player.vy=g.entry_vy;}
                                        nail=Nail::new();nail_response=hk_sim::NailResponse::new();focus.interrupt();focus_audio::interrupt();target=Some(id);safe=(player.x,player.y,id);safe_facing=player.facing;gate_cooldown=90;
                                    }
                                }
                            }
                            if target.is_none() && !vitals.dead && !vitals.hazard_pending && !world::contains(r.bounds,player.x,player.y) {
                                if let Some(id)=cache.locate(r.scene,player.x,player.y) {target=Some(id);spatial_target=true;}
                                else {
                                    apply_hurt(&settings.cheats,&mut vitals,&player,&mut nail,&mut nail_response,&mut focus,1,0,true);
                                }
                            }
                        }
                        reveals.tick(r.reveal_masks,&player,PARAMS);
                    }
                    if let Some((x,y,hp))=enemies.actor_state(0,12546) {unsafe {HK_CRAWLER_X=x;HK_CRAWLER_Y=y;HK_CRAWLER_HP=hp as i32;}}
                    // Crossroads_01 "Climber" (HealthManager level37:5145), for route telemetry.
                    if let Some((x,y,hp))=enemies.actor_state(2,5145) {unsafe {HK_CLIMBER_X=x;HK_CLIMBER_Y=y;HK_CLIMBER_HP=hp as i32;}}
                    let mut cut=0u32;for (i,p) in world::region_grass(r).take(32).enumerate(){if state.cut(p.state){cut|=1<<i;}}
                    unsafe {HK_GRASS_CUT_MASK=cut;HK_NAIL_ATTACK_COUNT=attacks;HK_HEALTH=vitals.health as u32;HK_BLUE_HEALTH=vitals.blue_health as u32;HK_SOUL=vitals.soul as u32;HK_PLAYER_X=player.x;HK_PLAYER_Y=player.y;HK_FOCUS_LOCKED=u32::from(focus.locks_control());}
                    prev_pad=pad;
                    emit::stage_end(stage::UPDATE);emit::task_end(task::FIXED_UPDATE);ticks+=1;
                    input::checkpoint();
                }
                // Final DMA has drained, so CPU-only work may reuse packet
                // RAM while the GPU finishes its copied commands. Reflect the
                // next tick's visual state before preparing its BACK prefix.
                // The renderer stops at source boundaries when the flip arrives
                // and validates exact camera/state again before using a prefix.
                if sim_clock!=coverage_clock && target.is_none()
                    && presentation::queued() && psx_rt::interrupts::gp1_queue_pending() {
                    coverage_clock=sim_clock;
                    state.apply(r);
                    apply_reveal_masks(&reveals,r);
                    geo_render::apply(geo,&mut state,region_id);
                    lifeblood::apply(life,&mut state,region_id);
                    great_door::apply(&door,&mut state,region_id);
                    let next_camera=camera_state.position();
                    // begin_deferred_swap already selected the future draw side.
                    // No draw-target writes, uploads or GPU kicks occur here.
                    render::prepare_back_prefix(next_camera,fb.buffer_y(fb.drawing),cache.coverage());
                }
            }
            presentation::finish();
            render::finish_gpu_frame();fb.apply_draw_target();
            emit::counter(counter::SIM_TICKS,ticks);emit::stage_end(stage::PRESENT);emit::counter(counter::VISUAL_FRAMES,1);
            #[cfg(feature="emulator-telemetry")] if frame%30==0 {log_state(frame,&player,prims);}
            frame=frame.wrapping_add(1);
            if let Some(s)=save_requested.take() {
                // Card frames share SIO0 with the pad: bracket the write like a scene load
                // so the sampler drops the polls it cannot take instead of faulting.
                input::begin_scene_load(sim_clock);input::begin_blocking_transfer();let _=save::write(profile,&s);sim_clock=input::end_scene_load();
                let held=ButtonState::from_bits(input::held_buttons());
                player.was_jump=held.is_held(button::CROSS);start_held=held.is_held(button::START);
            }
            if let Some(id)=target {
                if !cache.is_ready(id) {
                    assert!(world::scene_of(id)!=r.scene,"spatial region not resident");
                    audio::stop_footsteps();focus_audio::interrupt();
                    unsafe {HK_GAME_MODE=2;}
                    // Revoke all world views before scene metadata/geometry
                    // arenas are reused. This increments even when a retry
                    // admits the same scene at the same address.
                    state.begin_world_admission();
                    sim_clock=scene_transition::load(cache,&mut vram,id,sim_clock).expect("scene gate load");
                    let held=ButtonState::from_bits(input::held_buttons());
                    player.was_jump=held.is_held(button::CROSS);start_held=held.is_held(button::START);
                }
                // Every spatial view within the selected scene remains resident.
                if vram.ready(id) && matches!(cache.try_select(id),Ok(true)) {
                    region_id=id;continue 'region;
                }
                pending_target=Some(id);
            }
        }
    }
}
