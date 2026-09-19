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
// The visible enemy draws are animation keys, and the cache cannot be asked for
// more than it holds: the tile budget and the four reserved keys have to leave
// the request array room for both.
const _: () = assert!(enemies::MAX_VISIBLE + hk_cache::RESERVED_SLOTS <= hk_cache::MAX_REQUESTS);
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
mod shop;
mod cheats;
mod geo;
mod geo_render;
mod geo_audio;
mod runner_audio;
mod lifeblood;
mod great_door;
mod battle_gates;
mod render;
mod alpha_scissor;
mod alpha_scissor_cache;
mod scenery_geometry;
mod draw_packet;
mod frame;
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
// This and the helpers below it carried `#[inline(never)]` to hold `main`
// under the +/-128 KB a MIPS PC16 branch reaches. Splitting the frame loop
// into `frame::render`, `frame::interact` and `frame::simulate` is what holds
// it now, so the attributes are gone from this file: an inlining hint has to
// earn its place on its own, not stand in for a function that was too big.
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
fn strike_lifeblood(life:&mut lifeblood::World,scene:usize,nail:&Nail,player:&Player)->lifeblood::Strike {
    if !nail.hitting(ATTACK_PARAMS) {return lifeblood::Strike::default();}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    life.strike(scene,&polygon[..source.len()])
}
fn strike_great_door(door:&mut great_door::World,scene:usize,nail:&Nail,player:&Player)->great_door::Strike {
    if !nail.hitting(ATTACK_PARAMS) {return great_door::Strike::default();}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    door.strike(scene,&polygon[..source.len()])
}
/// The Shade's body takes the nail through the same swept polygon as the rest.
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
    audio::init();let (settings,profile)=menu::run(&mut fb,&slot_lines,profile_fault);
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
    let mut game=frame::Game {
        player:Player::spawn(SPAWN.0,SPAWN.1),
        vitals:settings.cheats.new_vitals(VITAL_PARAMS),
        nail:Nail::new(),
        nail_response:hk_sim::NailResponse::new(),
        focus:hk_sim::Focus::new(),
        dream:hk_sim::DreamNail::new(),
        cast:hk_sim::Cast::new(),
        state:world::State::new(),
        reveals:reveal_masks::State::new(),
        enemies:enemies::EnemyWorld::new(),
        door:great_door::World::new(),
        shade:shade::World::new(),
        bench:bench::State::new(),
        camera:camera::Camera::new(),
        geo:unsafe {&mut *(&raw mut GEO_WORLD)},
        life:unsafe {&mut *(&raw mut LIFE_WORLD)},
        settings,
        pause_menu:pause::State::new(),
        // Sly's counter. The shelf itself is `shop::state()`, because the charm
        // half of a purchase lands in `charms::state()` and the two must agree.
        shop_screen:shop::Screen::new(),
        paused:false,
        start_held:false,
        prev_pad:ButtonState::from_bits(0),
        save_prompt:None,
        save_requested:None,
        respawn:None,
        safe:(SPAWN.0,SPAWN.1,0usize),
        safe_facing:1,
        attacks:0,
        scene_ticks:0,
        gate_cooldown:0,
        region_id:0,
    };
    cheats::publish(game.settings.cheats,VITAL_PARAMS);
    // Continue: stand at the saved bench with the saved wallet and Great Door hits.
    if let Some(s)=saved {
        game.respawn=Some((s.scene as usize,s.seat,s.facing,s.region as usize));
        game.region_id=s.region as usize;game.player=Player::spawn(s.seat[0],s.seat[1]);game.player.facing=s.facing;
        game.safe=(s.seat[0],s.seat[1],game.region_id);game.safe_facing=s.facing;
        game.geo.restore_wallet(s.geo,geo::GEO_PARAMS);game.door.restore(s.door_hits);
        game.shade.restore(s.shade);
    }
    let mut frame=0u32;
    let mut scene_ticks_scene=usize::MAX;
    let mut vram=vram_cache::Cache::new();
    let mut sim_clock=psx_rt::interrupts::vblank_count();
    let mut initial=true;
    // Timer1 counts HBlanks for frame-pacing telemetry only.
    psx_io::timers::set_mode(psx_io::timers::Timer::Timer1,0x0100);
    'region: loop {
        if initial {
            unsafe { HK_GAME_MODE=2; }
            gpu::draw_sync();
            while cache.prepare_ambience().and_then(|_|cache.select(game.region_id)).is_err() {
                unsafe { HK_GAME_MODE=3; }
                vram=vram_cache::Cache::new();cache.reset_bootstrap();
                menu::restore();menu::retry(&mut fb);menu::loading(&mut fb);
            }
            // Atlas bytes were verified/uploaded before the initial scene
            // was admitted. Every spatial view in this scene stays live.
            vram.admit_scene(game.region_id,cache.atlases_ready());
            vram.activate(game.region_id);
            hud::upload();dialogue::upload();geo_render::upload();lifeblood::upload();shade::upload();ability_art::upload();geo_audio::upload();geo_audio::set_volume(game.settings.sfx);runner_audio::upload();runner_audio::set_volume(game.settings.sfx);
            initial=false;
            sim_clock=psx_rt::interrupts::vblank_count();
            input::start(sim_clock);
        }
        // Region entry follows a scene load or a boundary crossing without a
        // poll since the last frame. Service the pad around each bank-driven
        // pass so no stretch between polls approaches a VBlank.
        input::checkpoint();
        let r=&world::resident(game.region_id).expect("admitted world metadata covers the selected region");
        let meta_region=cache.world_region_index(game.region_id).expect("admitted world metadata covers the selected region");
        assert!(ambience::set_scene(r.scene as u8));
        // The view's cooked NPC, read once: the bank record cannot change while
        // this region is resident, and the tick and the draw both want it.
        // One per view, which host/regions.py refuses to exceed.
        let region_npc=world::npcs(r).next();
        game.enemies.sync_region(r);
        game.reveals.scene_ready(r.scene,r.reveal_masks);
        input::checkpoint();
        let bank=vram.activate(game.region_id);
        render::init(&cache.room(),bank,game.region_id,cache.coverage());
        input::checkpoint();
        game.state.apply(r);apply_reveal_masks(&game.reveals,r);
        input::checkpoint();
        geo_render::apply(game.geo,&mut game.state,game.region_id);
        lifeblood::apply(game.life,&mut game.state,game.region_id);
        great_door::apply(&game.door,&mut game.state,game.region_id);
        battle_gates::apply(&mut game.state,game.region_id);
        unsafe { HK_GAME_MODE=1;HK_REGION_ID=game.region_id as u32+1; }
        if scene_ticks_scene!=r.scene {game.scene_ticks=0;scene_ticks_scene=r.scene;}
        game.shade.enter_scene(r.scene);
        // The first frame renders before the first tick: seat the camera now
        // (a scene gate or respawn snaps through the displacement rule).
        game.camera.tick((game.player.x,game.player.y),game.player.facing,camera::limits(r,[game.player.x-PARAMS.half_width,game.player.y+PARAMS.bottom,game.player.x+PARAMS.half_width,game.player.y+PARAMS.top]));
        let mut pending_target:Option<usize>=None;
        let mut boundary_ticks=0u32;
        let mut spatial_target=false;
        loop {
            let mut target=pending_target.take();
            let room=cache.room();
            input::checkpoint();render::mark_frame_start();
            emit::frame_begin(frame);emit::stage_begin(stage::RENDER);
            // One camera for the whole frame: the catch-up ticks below hand the
            // same value to the enemies, so what they see is where the view was
            // drawn rather than where this frame's simulation leaves it.
            let camera=game.camera.position();
            let prims=frame::render(&mut game,r,&room,&mut fb,region_npc,camera,sim_clock,frame);
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
                        game.start_held=pad.is_held(button::START);game.player.was_jump=pad.is_held(button::CROSS);
                        continue;
                    }
                    let ui_consumed=frame::interact(&mut game,r,region_npc,pad,target.is_some());
                    let ready_crossing=spatial_target && target.is_some_and(|id|cache.is_ready(id)&&vram.ready(id))
                        && world::contains(r.collision_bounds,game.player.x,game.player.y);
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
                        frame::debug_reset(&mut game,&mut target);
                    } else if !game.paused && !ui_consumed && !game.vitals.tick() {
                        frame::simulate(&mut game,r,&room,cache,meta_region,pad,camera,&mut target,&mut spatial_target);
                    }
                    if let Some((x,y,hp))=game.enemies.actor_state(0,12546) {unsafe {HK_CRAWLER_X=x;HK_CRAWLER_Y=y;HK_CRAWLER_HP=hp as i32;}}
                    // Crossroads_01 "Climber" (HealthManager level37:5145), for route telemetry.
                    if let Some((x,y,hp))=game.enemies.actor_state(2,5145) {unsafe {HK_CLIMBER_X=x;HK_CLIMBER_Y=y;HK_CLIMBER_HP=hp as i32;}}
                    let mut cut=0u32;for (i,p) in world::region_grass(r).take(32).enumerate(){if game.state.cut(p.state){cut|=1<<i;}}
                    unsafe {HK_GRASS_CUT_MASK=cut;HK_NAIL_ATTACK_COUNT=game.attacks;HK_HEALTH=game.vitals.health as u32;HK_BLUE_HEALTH=game.vitals.blue_health as u32;HK_SOUL=game.vitals.soul as u32;HK_PLAYER_X=game.player.x;HK_PLAYER_Y=game.player.y;HK_FOCUS_LOCKED=u32::from(game.focus.locks_control());}
                    game.prev_pad=pad;
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
                    game.state.apply(r);
                    apply_reveal_masks(&game.reveals,r);
                    geo_render::apply(game.geo,&mut game.state,game.region_id);
                    lifeblood::apply(game.life,&mut game.state,game.region_id);
                    great_door::apply(&game.door,&mut game.state,game.region_id);
                    battle_gates::apply(&mut game.state,game.region_id);
                    let next_camera=game.camera.position();
                    // begin_deferred_swap already selected the future draw side.
                    // No draw-target writes, uploads or GPU kicks occur here.
                    render::prepare_back_prefix(next_camera,fb.buffer_y(fb.drawing),cache.coverage());
                }
            }
            presentation::finish();
            render::finish_gpu_frame();fb.apply_draw_target();
            emit::counter(counter::SIM_TICKS,ticks);emit::stage_end(stage::PRESENT);emit::counter(counter::VISUAL_FRAMES,1);
            #[cfg(feature="emulator-telemetry")] if frame%30==0 {log_state(frame,&game.player,prims);}
            frame=frame.wrapping_add(1);
            if let Some(s)=game.save_requested.take() {
                // Card frames share SIO0 with the pad: bracket the write like a scene load
                // so the sampler drops the polls it cannot take instead of faulting.
                input::begin_scene_load(sim_clock);input::begin_blocking_transfer();let _=save::write(profile,&s);sim_clock=input::end_scene_load();
                let held=ButtonState::from_bits(input::held_buttons());
                game.player.was_jump=held.is_held(button::CROSS);game.start_held=held.is_held(button::START);
            }
            if let Some(id)=target {
                if !cache.is_ready(id) {
                    assert!(world::scene_of(id)!=r.scene,"spatial region not resident");
                    audio::stop_footsteps();focus_audio::interrupt();
                    unsafe {HK_GAME_MODE=2;}
                    // Revoke all world views before scene metadata/geometry
                    // arenas are reused. This increments even when a retry
                    // admits the same scene at the same address.
                    game.state.begin_world_admission();
                    sim_clock=scene_transition::load(cache,&mut vram,id,sim_clock).expect("scene gate load");
                    let held=ButtonState::from_bits(input::held_buttons());
                    game.player.was_jump=held.is_held(button::CROSS);game.start_held=held.is_held(button::START);
                }
                // Every spatial view within the selected scene remains resident.
                if vram.ready(id) && matches!(cache.try_select(id),Ok(true)) {
                    game.region_id=id;continue 'region;
                }
                pending_target=Some(id);
            }
        }
    }
}
