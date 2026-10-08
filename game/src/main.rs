#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]
// Size over speed for code that runs a handful of times a tick (camera.rs).
#![feature(optimize_attribute)]
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
mod persist;
mod shade;
mod ability_art;
mod input_queue;
mod input_sampler;
mod input;
mod presentation;
mod scene_transition;
mod gate_probe;
mod exit_fade;
mod gate_tour;
mod animation_cache;
mod audio;
mod focus_audio;
mod music;
#[cfg(feature="audio-probe")]mod audio_probe;
mod ambience;
mod enemies;
// The visible enemy draws are animation keys, and the cache cannot be asked for
// more than it holds: the tile budget and the four reserved keys have to leave
// the request array room for both.
const _: () = assert!(enemies::MAX_VISIBLE + hk_cache::RESERVED_SLOTS <= hk_cache::MAX_REQUESTS);
mod hud;
mod world;
mod reveal_masks;
mod secret_breaks;
/// Offsets into the boot art chunk (host/build_guest.py `boot_art`).
mod boot_art {include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/boot_art.rs"));}
mod dialogue;
mod game_map;
mod mapper;
mod shaman;
mod blocker_roller;
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
mod props;
mod pickups;
mod great_door;
mod battle_gates;
mod battle_gate_art;
mod decor;
mod drip;
mod blocker_terrain;
mod scene_sfx;
mod modules;
mod fk_art;
mod soul_totems;
mod title_card;
mod boss_art;
mod mawlek_art;
mod gruz_art;
mod render;
#[cfg(any(feature="hero-light",feature="hero-vignette"))]
mod hero_light;
mod alpha_scissor;
mod alpha_scissor_cache;
mod scenery_geometry;
mod draw_packet;
mod frame;
mod spstack;
/// Canonical source admission remains closed until Runner audio and Charge Dust
/// have resident bindings. Native integration tests supply a recording sink.
/// Never admit an actor here and silently discard its presentation events.
/// Units from a gate trigger inside which its scene is prefetched (about two
/// seconds of running at the Knight's RUN_SPEED, more than a group read).
const PREFETCH_RADIUS:i32=match option_env!("HK_PREFETCH_RADIUS") {Some(s)=>{let b=s.as_bytes();let mut i=0;let mut v=0i32;while i<b.len() {v=v*10+(b[i]-b'0') as i32;i+=1;}v},None=>16};
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
        // These are drained inside EnemyWorld::tick, where the projectile
        // pool and the actor pool they reach are sibling fields of the actors
        // that ask for them.
        // `Roller Assign`: it rolls the way the Blocker faces.
        Roller { velocity, offset } => blocker_roller::spawn(event.scene,
            [event.position[0] + offset[0], event.position[1] + offset[1]], velocity, event.facing > 0),
        Fire { .. } | Summon { .. } | Release { .. } | Shockwave | GuardWave { .. } | Effect { .. } | MawlekShots { .. }
        | ReleaseReserve => {}
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
    let counts=state.apply(|c|world::reveal_initial_opacity(region.scene,c),world::reveal_bindings(region));
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
fn draw_frame(room: &Room, frame: usize, player: &Player, camera: (i32, i32), tint: u8) {
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
    render::texture(u32_at(f, 0) as usize, v, (tint, tint, tint));
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
static mut PROPS_WORLD:props::World=props::World::new();
static mut PICKUPS_WORLD:pickups::World=pickups::World::new();
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
/// Harness telemetry for side-by-side runs, published with the Knight's
/// position at the end of each simulated tick so a trace row never pairs one
/// tick's count with another's position: the Knight's facing (1 right, -1
/// left), samples consumed so far (one per tick) and the last one's buttons.
/// tools/og_compare.py ties the count to the tape.
#[no_mangle] pub static mut HK_PLAYER_FACING:i32=0;
#[no_mangle] pub static mut HK_SIM_TICKS:u32=0;
#[no_mangle] pub static mut HK_SIM_PAD:u32=0;
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
/// The active nail polygon's world bounds, when the nail is hitting.
fn nail_bounds(nail:&Nail,player:&Player)->Option<[i32;4]> {
    if !nail.hitting(ATTACK_PARAMS) {return None;}
    let mut b=[i32::MAX,i32::MAX,i32::MIN,i32::MIN];
    for p in NAIL_POLYGONS[nail.kind as usize] {
        let (x,y)=(player.x-p[0]*player.facing,player.y+p[1]);
        b=[b[0].min(x),b[1].min(y),b[2].max(x),b[3].max(y)];
    }
    Some(b)
}
/// A pickup taken: its PlayerData, its saved bool and its message.
#[inline(never)]
fn take_pickup(scene:usize,p:&pickups::Pickup) {
    persist::set(persist::Kind::Pickup,scene,p.local as usize,1);
    match p.grant {
        pickups::Grant::Charm(n)=>charms::pick_up(n as usize),
        // A relic is only a count in the source, and nothing in the slice buys
        // one back, so the saved pickup is the whole record.
        pickups::Grant::Trinket(_)=>{}
        pickups::Grant::CityKey=>persist::set_player(persist::HAS_CITY_KEY),
        pickups::Grant::MaskShard=>shop::pick_up(|s|{s.add_mask_shard();}),
        pickups::Grant::VesselFragment=>shop::pick_up(|s|{s.add_vessel_fragment();}),
        pickups::Grant::RancidEgg=>shop::pick_up(|s|s.rancid_eggs=s.rancid_eggs.saturating_add(1)),
    }
    title_card::show_item(p.name);
    scene_sfx::play(if p.touch {scene_sfx::HEARTPIECE_COLLECT} else {scene_sfx::SHINY_ITEM_PICKUP});
}
/// `Chest Control`'s `Open`: saved at once, and `Spawn Items` flings its Geo
/// through the coin pool.
#[inline(never)]
fn open_chest(geo:&mut geo::World,scene:usize,chest:&pickups::Chest) {
    persist::set(persist::Kind::Pickup,scene,chest.local as usize,1);
    let fling=geo::Fling{speed:chest.speed,angle:chest.angle,spread:[0,0]};
    geo.spawn_chest(scene,0x8000_0000|chest.local as u32,chest.position,fling,chest.geo);
    scene_sfx::play(scene_sfx::CHEST_OPEN);camera::request(camera::Shake::Kill);
}
/// The nail against the scene's chests; the opened chest, if any.
#[inline(never)]
fn strike_chests(pickups:&mut pickups::World,nail:&Nail,player:&Player,body:[i32;4])->Option<&'static pickups::Chest> {
    if !nail.hitting(ATTACK_PARAMS) {return None;}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    pickups.strike(&polygon[..source.len()],body)
}
/// Stalactites and grub jars take the nail through the same swept polygon;
/// a freed grub is recorded in the world save at once.
fn strike_props(props:&mut props::World,scene:usize,nail:&Nail,player:&Player,body:[i32;4])->props::Strike {
    if !nail.hitting(ATTACK_PARAMS) {return props::Strike::default();}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    // `damages_enemy`'s direction: 0 right, 90 up, 180 left, 270 down.
    let direction=match nail.kind {2=>props::NailDirection::Up,3=>props::NailDirection::Down,
        _ if player.facing>0=>props::NailDirection::Right,_=>props::NailDirection::Left};
    let strike=props.strike(&polygon[..source.len()],body,direction,|local|persist::set(persist::Kind::Grub,scene,local,1));
    if strike.freed>0 {unsafe {HK_GRUBS=persist::count(persist::Kind::Grub) as u32;}}
    strike
}
/// Grubs freed, as the world save counts them.
#[no_mangle] pub static mut HK_GRUBS:u32=0;
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
    // `Knight Damage`'s `Gen` state, on the DAMAGE event TakeDamage sends:
    // AverageShake to the camera on every hit. Its hit particles, the
    // vignette flash and the low-health steam are not reproduced.
    camera::request(camera::Shake::Average);
    // The death FSM starts hero_damage and hero_death_v2 together; the hurt
    // clip above stands in for the first.
    if hurt==hk_sim::Hurt::Died {audio::hero_death();}
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
// `VITAL_PARAMS.max_health` and `shop::STARTING_MASKS` are two cooks of the
// same source number, and `shop::vitals` replaces the first with the shop's
// own mask total. A board nobody has touched therefore has to compose back to
// exactly the cooked base; if the two ever disagreed, every new game would
// start on the wrong number of masks and nothing else would say so.
const _:()=assert!(VITAL_PARAMS.max_health==shop::STARTING_MASKS as u16);
/// StartSoulLimiter caps the vessel at 66 while a Shade is owed.
const SOUL_LIMITED:u16=66;
/// The SOUL ceiling: the live `max_soul`, which carries whatever reserve a
/// fused vessel added to the Knight's own vessel, or the limiter's figure
/// while a Shade is owed.
///
/// The limited figure is the vessel's alone, not the vessel's plus the
/// reserve: the source's rule for a limiter running against a reserve was not
/// read here, and no run can reach that pair anyway, because Sly stocks two of
/// the three fragments a vessel needs and nothing else in the admitted slice
/// sells one.
fn soul_cap(shade:&shade::World,max_soul:u16)->u16 {
    if shade.soul_limited() {SOUL_LIMITED.min(max_soul)} else {max_soul}
}
/// The active nail polygon against the scene's soul totems; the attack counter
/// is the swing token, so one swing pays a totem once.
fn strike_totems(scene:usize,nail:&Nail,player:&Player,swing:u32)->u16 {
    if !nail.hitting(ATTACK_PARAMS) {return 0;}
    let source=NAIL_POLYGONS[nail.kind as usize];let mut polygon=[[0;2];16];
    assert!((3..=16).contains(&source.len()));
    for (dst,p) in polygon.iter_mut().zip(source) {*dst=[player.x-p[0]*player.facing,player.y+p[1]];}
    soul_totems::strike(scene,&polygon[..source.len()],swing)
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
    // The renderer's working storage lives for the whole program in this
    // frame, inside the 48 KiB stack reservation (measured peak about 32 KiB,
    // boss-fight), rather than in .bss: see render::RenderScratch.
    let mut render_scratch=core::mem::MaybeUninit::<render::RenderScratch>::uninit();
    unsafe {render::bind_scratch(render_scratch.as_mut_ptr());}
    gpu::set_draw_area(0,0,319,239);gpu::set_draw_offset(0,0);
    // The card is read before the pad sampler exists: SIO0 is not yet shared.
    let (slots,card_fault)=save::survey();
    // A launcher leaves the pad in whatever mode its last program used: ask for analog
    // and lock it (the Analog button then cannot flip it mid-play). Only now that the
    // card is done with SIO0. The game reads buttons only, so a digital pad plays too.
    let _=psx_pad::require_analog_port1();
    // Each slot's one line on the save screen. Geo and masks are what the
    // record holds; the source also shows play time, completion and the map
    // zone, none of which this port tracks.
    let mut slot_text=[[0u8;16];save::PROFILES];
    let mut slot_used=[0usize;save::PROFILES];
    for (i,slot) in slots.iter().enumerate() {
        if let save::Slot::Used(save)=slot {
            let line=&mut slot_text[i];let mut at=0;
            for b in b"Geo " {line[at]=*b;at+=1;}
            let mut digits=[0u8;psx_math::fmt::U32_DEC_MAX];
            for b in psx_math::fmt::u32_dec(&mut digits,save.geo.min(9999)).bytes() {line[at]=b;at+=1;}
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
    audio::init();
    // Before the title: it plays two of these. A failed read is retried by
    // bootstrap's prepare_ambience, so it only costs the title its clips.
    let _=unsafe { &mut *(&raw mut ROOM_CACHE) }.prepare_world_sfx();
    // The title art follows it on the disc; menu::run uploads it to VRAM
    // before anything else takes the arena.
    let art=unsafe { &mut *(&raw mut ROOM_CACHE) }.menu_art().ok();
    // The art the game uploads once, from the same chunk while it is staged.
    if let Some(boot)=art {geo_render::upload(boot);lifeblood::upload(boot);}
    let (settings,profile)=menu::run(&mut fb,art,&slot_lines,profile_fault);
    focus_audio::set_volume(settings.sfx);menu::loading(&mut fb);
    #[cfg(feature="audio-probe")]
    audio_probe::run(unsafe { &mut *(&raw mut ROOM_CACHE) },&mut fb);
    let saved=match slots[profile] {save::Slot::Used(s)=>Some(s),_=>None}
        // A record from another disc build may name a slot this catalogue lacks.
        .filter(|s|(s.region as usize)<world::REGION_SCENES.len()&&world::scene_of(s.region as usize)==s.scene as usize);
    // Sly's half of PlayerData before the charm board, and both before the
    // first Vitals: a mask a shard fuse bought is `maxHealthBase` and Fragile
    // Heart adds to it, so the base has to be in place before anything reads
    // the composed max_health. Taken from the record itself rather than from
    // `saved`, for the same reason the board is: a stale spawn point is no
    // reason to un-buy what the player paid for.
    shop::boot(match slots[profile] {save::Slot::Used(s)=>Some((s.shop_slots,s.shop_counters)),_=>None});
    charms::boot(match slots[profile] {save::Slot::Used(s)=>
        Some((s.charms_owned,s.charms_equipped,s.charm_notches,s.can_overcharm)),_=>None},settings.cheats.charms);
    // Each NPC's conversation cursor, from the record for the same reason: a
    // stale spawn point is no reason to make Elderbug introduce himself again.
    if let save::Slot::Used(s)=slots[profile] {dialogue::restore_met(s.npc_conversations);
        // And the PlayerData the cooked scripts wrote, if the record was
        // written by this bank. `script::boot` drops it when it was not.
        script::boot(&s.script_fields,s.script_field_fnv);
        // The port's PlayerData and SceneData last: an HKS4 record's False
        // Knight moves out of the script reserve `script::boot` just filled.
        persist::load(&s,profile);}
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
        props:unsafe {&mut *(&raw mut PROPS_WORLD)},
        pickups:unsafe {&mut *(&raw mut PICKUPS_WORLD)},
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
        exit:None,
        region_id:0,
        view:0,
        wake:false,
    };
    if gate_tour::active() {game.settings.cheats.invincible=true;}
    cheats::publish(game.settings.cheats,VITAL_PARAMS);
    // Continue: stand at the saved bench with the saved wallet and Great Door hits.
    if let Some(s)=saved {
        game.respawn=Some((s.scene as usize,s.seat,s.facing,s.region as usize));
        game.region_id=s.region as usize;game.player=Player::spawn(s.seat[0],s.seat[1]);game.player.facing=s.facing;
        game.safe=(s.seat[0],s.seat[1],game.region_id);game.safe_facing=s.facing;
        game.geo.restore_wallet(s.geo,geo::GEO_PARAMS);game.door.restore(s.door_hits);
        game.shade.restore(s.shade);game.shade.carry(None);
    }
    // After the Game exists, whether or not a save loaded: a new game hands
    // over an empty store and changes nothing.
    persist::apply(&mut game.state,game.geo,game.life);
    let mut frame=0u32;
    let mut scene_ticks_scene=usize::MAX;
    let mut entry_gate:Option<usize>=None;
    let mut scene_npcs:[Option<(usize,world::Npc)>;4]=[None;4];
    let mut scene_npcs_scene=usize::MAX;
    let mut vram=vram_cache::Cache::new();
    let mut sim_clock=psx_rt::interrupts::vblank_count();
    // Simulation ticks at 60 Hz on the shared psx-tick clock with unbounded
    // catch-up. `sim_clock` stays the VBlank of the last tick run (input
    // stamps, rendering and presentation read it); every place that resets it
    // realigns the clock to the tick after it.
    let mut hk_clock=psx_tick::FixedClock::new(psx_tick::TickConfig::new(psx_tick::TickRate::HZ60),sim_clock.wrapping_add(1));
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
                menu::restore(cache.menu_art().ok());menu::retry(&mut fb);menu::loading(&mut fb);
            }
            // Atlas bytes were verified/uploaded before the initial scene
            // was admitted. Every spatial view in this scene stays live.
            vram.admit_scene(game.region_id,cache.atlases_ready());
            vram.activate(game.region_id);
            // The Geo and Runner banks were streamed and uploaded by
            // prepare_ambience above, before any scene took the arena they
            // stage through; only their levels are applied here.
            hud::upload();dialogue::upload();props::upload();shade::upload();ability_art::upload();geo_audio::set_volume(game.settings.sfx);runner_audio::set_volume(game.settings.sfx);
            initial=false;
            sim_clock=psx_rt::interrupts::vblank_count();
            hk_clock.realign(sim_clock.wrapping_add(1));
            input::start(sim_clock);
        }
        // Region entry follows a scene load or a boundary crossing without a
        // poll since the last frame. Service the pad around each bank-driven
        // pass so no stretch between polls approaches a VBlank.
        input::checkpoint();
        let r=&world::resident(game.region_id).expect("admitted world metadata covers the selected region");
        let meta_region=cache.world_region_index(game.region_id).expect("admitted world metadata covers the selected region");
        assert!(ambience::set_scene(r.scene as u8));
        music::enter_scene(r.scene);
        // The view's cooked NPC, read once: the bank record cannot change while
        // this region is resident, and the tick and the draw both want it.
        // One per view, which host/regions.py refuses to exceed.
        let region_npc=world::npcs(r).next();
        // Every NPC of the scene and its view, once per scene: one standing in a
        // neighbouring view is drawn when the camera shows it.
        if scene_npcs_scene!=r.scene {
            scene_npcs=[None;4];let mut n=0;
            world::scene_npcs(r.scene,input::checkpoint,|slot,npc|{if n<scene_npcs.len() {scene_npcs[n]=Some((slot,npc));n+=1;}});
            scene_npcs_scene=r.scene;
        }
        game.enemies.sync_region(r);
        if game.reveals.scene_ready(r.scene,world::reveal_masks(r.scene)) {
            game.reveals.restore_revealed(persist::secrets(r.scene));
            // A mask a secret uncovers follows that secret's broken bit.
            let base=r.scene*world::BREAKABLES_PER_SCENE;
            game.reveals.restore_driven(&|local|game.state.broken(base+local));
        }
        input::checkpoint();
        let bank=vram.activate(game.region_id);
        // The first frame renders before the first tick: seat the camera in a
        // new scene now (a view change inside one scene leaves it be).
        let hero=frame::camera_hero(&game);
        game.camera.seat(r,&hero,&|id|game.state.broken(id));
        // Draw the view the camera needs, which a camera lock or the follow
        // lag can make a neighbour of the Knight's (disc::Cache::camera_view).
        let mut rv=bind_view(cache,&mut game,bank,r.scene);
        input::checkpoint();
        game.state.apply(&rv);apply_reveal_masks(&game.reveals,&rv);
        input::checkpoint();
        geo_render::apply(game.geo,&mut game.state,game.region_id,game.view);
        lifeblood::apply(game.life,&mut game.state,game.region_id,game.view);
        props::apply(game.view);pickups::apply(game.view);
        great_door::apply(&game.door,&mut game.state,game.region_id);
        battle_gates::apply(&mut game.state,game.region_id,game.view);blocker_terrain::apply(&mut game.state,game.region_id);
        unsafe { HK_GAME_MODE=1;HK_REGION_ID=game.region_id as u32+1; }
        let scene_ticks_scene_changed=scene_ticks_scene!=r.scene;
        if scene_ticks_scene!=r.scene {game.scene_ticks=0;scene_ticks_scene=r.scene;}
        game.shade.enter_scene(r.scene);
        game.props.enter_scene(r.scene,|local|persist::get(persist::Kind::Grub,r.scene,local).is_some());
        game.pickups.enter_scene(r.scene,&|local|persist::get(persist::Kind::Pickup,r.scene,local).is_some());
        unsafe {HK_GRUBS=persist::count(persist::Kind::Grub) as u32;}
        if scene_ticks_scene_changed {title_card::enter_scene(r.scene);}
        let mut pending_target:Option<usize>=None;
        // The gate the prefetch is reading for (index into the scene's gate list).
        let mut predicted:Option<usize>=None;
        // A new scene: the gate the Knight arrived by is found at the first
        // guess. Only a gate sets the cooldown, so a boot or a respawn at a
        // bench has no arrival gate to hold back: excluding the nearest gate
        // there kept a Knight who walks straight to it from ever prefetching.
        if scene_ticks_scene_changed {entry_gate=(game.gate_cooldown>0).then_some(usize::MAX);}
        let mut boundary_ticks=0u32;
        let mut spatial_target=false;
        loop {
            let mut target=pending_target.take();
            let room=cache.room();
            input::checkpoint();render::mark_frame_start();
            gate_probe::frame(game.door.shade());
            emit::frame_begin(frame);emit::stage_begin(stage::RENDER);
            // The camera this frame draws with. The simulation ticks below read
            // their own (frame::simulate), so behaviour does not follow the
            // frame rate.
            let camera=game.camera.position();
            // Rebind only when the camera has left the drawn view's range.
            let c=rv.camera;
            if camera.0<c[0]||camera.0>c[2]||camera.1<c[1]||camera.1>c[3]||render::bound_view()!=game.view {
                rv=bind_view(cache,&mut game,bank,r.scene);
            }
            if game.view!=game.region_id {unsafe {HK_VIEW_ELSEWHERE_FRAMES=HK_VIEW_ELSEWHERE_FRAMES.wrapping_add(1);}}
            // An NPC of another view the camera shows (sprite within a screen
            // plus a margin of its centre), drawn from its own view's room.
            let other=scene_npcs.iter().flatten().find(|(slot,n)| *slot!=game.region_id
                && (n.position[0]-camera.0).abs()<NPC_REACH[0] && (n.position[1]-camera.1).abs()<NPC_REACH[1]);
            let other_room=other.and_then(|(slot,_)|cache.room_for(*slot));
            let other_npc=other.zip(other_room.as_ref()).map(|((_,n),room)|(*n,room));
            let prims=frame::render(&mut game,r,&rv,&room,other_npc,&mut fb,region_npc,camera,sim_clock);
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
                // A simulation slower than real time skips ticks rather than
                // trail its input without bound (the input queue once faulted
                // here, standing still in Crossroads_37).
                if let Some(tick)=input::bound_lag(now) {sim_clock=tick;hk_clock.realign(tick.wrapping_add(1));}
                while hk_clock.due(now) {
                    sim_clock=hk_clock.next_due().wrapping_sub(1);
                    let raw=enemies::roar_lock(gate_tour::pad(input::consume(sim_clock)));
                    // Menus, dialogue and the shop read the pad as polled; the
                    // Knight's simulation reads it through the original's latency.
                    let pad=ButtonState::from_bits(raw);let delayed=ButtonState::from_bits(input::hero_latency(raw));
                    gate_tour::tick(game.door.pending());
                    // The source scene has ended. A rendered-frame catch-up must
                    // not simulate the destination player against outgoing terrain.
                    if target.is_some_and(|id|world::scene_of(id)!=r.scene) {
                        game.start_held=pad.is_held(button::START);game.player.was_jump=pad.is_held(button::CROSS);
                        continue;
                    }
                    let ui_consumed=frame::interact(&mut game,r,region_npc,pad,target.is_some());
                    let menu=ui_consumed||frame::menu_reads_pad(&game);
                    let hero=if menu {pad} else {delayed};
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
                        frame::simulate(&mut game,r,&room,cache,meta_region,hero,&mut target,&mut spatial_target);
                        #[cfg(feature="tick-log")]
                        input::log_tick(!menu);
                    } else {
                        #[cfg(feature="tick-log")]
                        input::log_tick(false);
                    }
                    if let Some((x,y,hp))=game.enemies.actor_state(0,12546) {unsafe {HK_CRAWLER_X=x;HK_CRAWLER_Y=y;HK_CRAWLER_HP=hp as i32;}}
                    // Crossroads_01 "Climber" (HealthManager level37:5145), for route telemetry.
                    if let Some((x,y,hp))=game.enemies.actor_state(2,5145) {unsafe {HK_CLIMBER_X=x;HK_CLIMBER_Y=y;HK_CLIMBER_HP=hp as i32;}}
                    let mut cut=0u32;for (i,p) in world::region_grass(r).take(32).enumerate(){if game.state.cut(p.state){cut|=1<<i;}}
                    unsafe {HK_GRASS_CUT_MASK=cut;HK_NAIL_ATTACK_COUNT=game.attacks;HK_HEALTH=game.vitals.health as u32;HK_BLUE_HEALTH=game.vitals.blue_health as u32;HK_SOUL=game.vitals.soul as u32;HK_SIM_TICKS=input::CONSUMED;HK_SIM_PAD=input::CONSUMED_PAD;HK_PLAYER_X=game.player.x;HK_PLAYER_Y=game.player.y;HK_PLAYER_FACING=game.player.facing;HK_FOCUS_LOCKED=u32::from(game.focus.locks_control());}
                    game.prev_pad=hero;
                    music::tick_regions(game.player.x,game.player.y);
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
                    game.state.apply(&rv);
                    apply_reveal_masks(&game.reveals,&rv);
                    geo_render::apply(game.geo,&mut game.state,game.region_id,game.view);
                    lifeblood::apply(game.life,&mut game.state,game.region_id,game.view);
                    // The same bindings, in the same order, as frame::render: a
                    // view with a prop or pickup otherwise snapshots a different
                    // set of hidden draws here and its prefix can never match.
                    props::apply(game.view);pickups::apply(game.view);
                    great_door::apply(&game.door,&mut game.state,game.region_id);
                    battle_gates::apply(&mut game.state,game.region_id,game.view);blocker_terrain::apply(&mut game.state,game.region_id);
                    let next_camera=game.camera.position();
                    // begin_deferred_swap already selected the future draw side.
                    // No draw-target writes, uploads or GPU kicks occur here.
                    unsafe {spstack::sim(||render::prepare_back_prefix(next_camera,fb.buffer_y(fb.drawing),cache.coverage()))};
                }
                present_spin(sim_clock);
            }
            presentation::finish();
            render::finish_gpu_frame();fb.apply_draw_target();
            hk_clock.end_frame();
            emit::counter(counter::SIM_TICKS,ticks);emit::stage_end(stage::PRESENT);emit::counter(counter::VISUAL_FRAMES,1);
            #[cfg(feature="emulator-telemetry")] if frame%30==0 {log_state(frame,&game.player,prims);}
            frame=frame.wrapping_add(1);
            // Install the approached room's code from its prefetched chunk,
            // a slice per frame (modules.rs).
            game.shade.carry(Some(r.scene));modules::service();
            // Guess the next gate every fourth frame and let the drive read
            // that scene ahead; an exit fade has already named its gate.
            if game.exit.is_none() && frame&3==0 && target.is_none() {
                let next=if cfg!(feature="prefetch-oracle") {gate_tour::upcoming().map(|(_,region)|disc::scene_index(region))}
                    else {game.state.predict_gate(r.scene,&game.player,predicted,&mut entry_gate,PREFETCH_RADIUS).and_then(|(i,g,distance)|{
                        // Only near a gate: a read costs the CPU its sector
                        // drain, so the drive reads for the gate the Knight is
                        // about to reach, not for every gate he passes far off.
                        (distance<=PREFETCH_RADIUS).then(||{predicted=Some(i);disc::scene_index(g.target_region)})})};
                if let Some(scene)=next {disc::prefetch_hint(scene);}
            }
            if let Some(s)=game.save_requested.take() {
                // Card frames share SIO0 with the pad: bracket the write like a scene load
                // so the sampler drops the polls it cannot take instead of faulting.
                input::begin_scene_load(sim_clock);input::begin_blocking_transfer();let _=save::write(profile,&s,persist::store().items());sim_clock=input::end_scene_load();hk_clock.realign(sim_clock.wrapping_add(1));input::seed_latency();
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
                    sim_clock=scene_transition::load(cache,&mut vram,id,sim_clock,fb.buffer_y(fb.drawing^1),fb.buffer_y(fb.drawing)).expect("scene gate load");
                    hk_clock.realign(sim_clock.wrapping_add(1));input::seed_latency();
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

/// The present loop's idle wait: nothing to simulate until the next VBlank,
/// the frame's final list still draining or its flip still queued. Returns on
/// a new VBlank, when the flip is queued (the loop may prebuild the next
/// frame then) or lands. Its own function so psoxide-pgo measure
/// --wait-range can name it (the loop around it calls out and stores, so the
/// wait-loop detector counts it as work). A flip whose GPU never signals is
/// written by hand after FLIP_TIMEOUT_VBLANKS instead of waiting forever.
#[inline(never)]
fn present_spin(sim_clock:u32) {
    let queued=presentation::queued();
    loop {
        presentation::checkpoint();
        input::checkpoint();
        if psx_rt::interrupts::vblank_count()!=sim_clock || presentation::queued()!=queued {return;}
        if queued && !psx_rt::interrupts::gp1_queue_pending() {return;}
        if presentation::flip_overdue() {presentation::force_flip();return;}
        // Idle until the flip: hash the approached room's prefetched chunks
        // in small slices so the gate need not (disc.rs).
        render::warm_step(2);
        disc::preverify_step(256);
    }
}

/// How far an NPC's position may be from the camera and still be drawn from a
/// neighbouring view: half a screen (10.8 by 8.1 units at KNIGHT_SCALE) plus
/// the largest NPC sprite's half extent, generously.
const NPC_REACH:[i32;2]=[16*ONE,13*ONE];
/// Views bound for drawing (a switch of the drawn view, not of the Knight's),
/// and frames drawn with a view other than the Knight's own.
#[no_mangle] pub static mut HK_VIEW_BINDS:u32=0;
#[no_mangle] pub static mut HK_VIEW_ELSEWHERE_FRAMES:u32=0;
/// The drawn view's catalogue slot plus one (HK_REGION_ID is the Knight's).
#[no_mangle] pub static mut HK_VIEW_ID:u32=0;
/// Frames drawn with the camera outside every resident view's cooked camera
/// range: the scenery there was not cooked for that viewpoint.
#[no_mangle] pub static mut HK_CAMERA_VIEW_MISS:u32=0;
/// Bind the scene view the camera needs to the renderer and return it: the
/// one whose cooked camera range holds the camera (disc::Cache::camera_view).
/// A no-op while that view is already bound. Every view of the admitted scene
/// is resident, so this only rebuilds the renderer's draw tables, which a
/// region change used to do every time.
#[inline(never)]
fn bind_view(cache:&disc::Cache,game:&mut frame::Game,bank:usize,scene:usize)->world::Region {
    let (x,y)=game.camera.position();
    let view=cache.camera_view(scene,x,y,game.view,game.region_id);
    if render::bound_view()!=view {
        let room=cache.room_for(view).expect("camera view resident");
        render::init(&room,bank,view,cache.coverage());
        unsafe {HK_VIEW_BINDS=HK_VIEW_BINDS.wrapping_add(1);}
    }
    game.view=view;unsafe {HK_VIEW_ID=view as u32+1;}
    let region=world::resident(view).expect("camera view in the admitted bank");
    let c=region.camera;
    if x<c[0]||x>c[2]||y<c[1]||y>c[3] {unsafe {HK_CAMERA_VIEW_MISS=HK_CAMERA_VIEW_MISS.wrapping_add(1);}}
    region
}
