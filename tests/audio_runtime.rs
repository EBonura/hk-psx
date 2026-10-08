//! Actual guest audio module with only the hardware boundary replaced. Synthetic
//! samples distinguish every source address; fixtures contain no retail audio.
extern crate self as psx_spu;
use std::sync::Mutex;

#[derive(Clone,Copy)]pub struct Volume(pub i16);
#[derive(Clone,Copy)]pub struct SpuAddr(u32);
impl SpuAddr {pub fn new(value:u32)->Self{Self(value)}}
pub struct Pitch(u16);
impl Pitch {pub fn raw(value:u16)->Self{Self(value)}}
pub struct Adsr;
impl Adsr {pub fn sample_one_shot()->Self{Self}}
pub struct Voice(u8);

const ALLOWED:u32=0x3803f; // 0..5,15,16,17; never ambience6..11 or Geo12..14.
#[derive(Clone,Copy,Debug,PartialEq)]enum Event {
    Init,Upload(u32,usize),Configure(u8,u32,u32,i16),Gain(u8,i16),
    Off(u32),Pitch(u8,u16),On(u32,i16),
}
struct Trace {events:Vec<Event>,gains:[i16;24]}
static TRACE:Mutex<Trace>=Mutex::new(Trace{events:Vec::new(),gains:[0;24]});
fn take()->Vec<Event>{std::mem::take(&mut TRACE.lock().unwrap().events)}
fn voice(mask:u32)->usize {
    assert_eq!(mask&!ALLOWED,0,"other subsystem's voice modified");
    assert_eq!(mask.count_ones(),1,"event must affect exactly one dedicated voice");
    mask.trailing_zeros() as usize
}
impl Voice {
    pub fn new(index:u8)->Self {assert!(index<24 && ALLOWED&(1<<index)!=0);Self(index)}
    pub fn configure_sample(self,address:SpuAddr,rate:u32,gain:Volume,_:Adsr){
        let mut t=TRACE.lock().unwrap();t.gains[self.0 as usize]=gain.0;
        t.events.push(Event::Configure(self.0,address.0,rate,gain.0));
    }
    pub fn set_volume(self,left:Volume,right:Volume){
        assert_eq!(left.0,right.0);let mut t=TRACE.lock().unwrap();
        t.gains[self.0 as usize]=left.0;t.events.push(Event::Gain(self.0,left.0));
    }
    pub fn set_pitch(self,pitch:Pitch){TRACE.lock().unwrap().events.push(Event::Pitch(self.0,pitch.0));}
    pub fn key_off(mask:u32){voice(mask);TRACE.lock().unwrap().events.push(Event::Off(mask));}
    pub fn key_on(mask:u32){
        let index=voice(mask);let mut t=TRACE.lock().unwrap();let gain=t.gains[index];
        t.events.push(Event::On(mask,gain));
    }
}
pub fn init(){TRACE.lock().unwrap().events.push(Event::Init);}
pub fn upload_adpcm(address:SpuAddr,bytes:&[u8]){
    assert_eq!((bytes.as_ptr() as usize)%4,0,"DMA source alignment");
    // init's own upload: the menu's two clips, right above the bank.
    if address.0==0x1010+320 {
        assert_eq!(bytes.len(),64);assert_eq!((bytes[0],bytes[32]),(20,21));
        TRACE.lock().unwrap().events.push(Event::Upload(address.0,bytes.len()));return;
    }
    // The world bank, at its own base below Focus.
    if address.0==0x60000 {
        assert_eq!(bytes.len(),160);
        for (i,sample) in bytes.chunks_exact(32).enumerate(){assert_eq!(sample[0],40+i as u8);}
        TRACE.lock().unwrap().events.push(Event::Upload(address.0,bytes.len()));return;
    }
    assert_eq!(address.0,0x1010);assert_eq!(bytes.len(),320);
    for (i,sample) in bytes.chunks_exact(32).enumerate(){
        assert_eq!(sample[0],i as u8,"complete, ordered synthetic bank");
        assert!(sample[1..].iter().all(|b|*b==0));
    }
    TRACE.lock().unwrap().events.push(Event::Upload(address.0,bytes.len()));
}
#[path="../game/src/audio.rs"]mod audio;
// The bank now arrives as a slice from the disc loader, which stages it in the
// word-aligned scene arena; the fixture keeps that alignment so the DMA
// contract asserted in upload_adpcm above still means something.
#[repr(align(4))]struct Bank([u8;320]);
static BANK:Bank=Bank(*include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/sfx.adpcm")));
#[repr(align(4))]struct World([u8;160]);
static WORLD:World=World(*include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/world-sfx.adpcm")));

fn descent(ticks:usize){for _ in 0..ticks{audio::movement_tick(false,false,true,false,true);}}
fn run(){audio::movement_tick(true,true,false,true,true);}
fn pair(index:u8,gain:i16)->Vec<Event>{vec![Event::Off(1<<index),Event::On(1<<index,gain)]}

#[test]
fn actual_audio_owns_only_reserved_voices_and_routes_source_events(){
    // Muting before boot must affect initial configuration too.
    audio::set_volume(0);take();
    // init owns the SPU reset and runs before the disc is ready; no voice is
    // configured until the bank it points at is actually uploaded.
    audio::init();assert!(!audio::ready());assert_eq!(take(),[Event::Init,Event::Upload(0x1010+320,64)]);
    audio::upload(&BANK.0);assert!(audio::ready());
    let mut events=vec![Event::Init];events.extend(take());
    assert_eq!(events[1],Event::Upload(0x1010,320));
    let physical=[0,1,2,3,4,5,16,17];
    let mut expected=vec![Event::Init,Event::Upload(0x1010,320)];
    for (i,v) in physical.iter().enumerate(){expected.push(Event::Configure(*v,0x1010+i as u32*32,if i==7{11025}else{22050},0));}
    // The shared voice is configured by each play, never by the upload: the
    // title's start clip can still be sounding on it when this bank arrives.
    assert_eq!(events,expected);

    for (i,play) in [audio::door,audio::jump,audio::land,audio::nail,audio::hurt,audio::enemy_hit].into_iter().enumerate(){
        play();
        // Voice 0 (the wall hit, shared with the movement extras) and the
        // nail's voice change clip from play to play, so they are pointed at
        // their own sample every time.
        if i==0 || i==3 {
            assert_eq!(take(),[Event::Off(1<<i),Event::Configure(i as u8,0x1010+i as u32*32,22050,0),Event::On(1<<i,0)]);
        } else {assert_eq!(take(),pair(i as u8,0));}
    }
    unsafe {assert_eq!(audio::HK_SFX_COUNT,6);assert_eq!(audio::HK_SFX_EVENT_COUNTS,[1,1,1,1,1,1,0,0]);}
    // The swing variants: AltSlash and DownSlash carry their own clips on the
    // nail's voice; up and normal keep the nail's.
    audio::nail_kind(1);assert_eq!(take(),[Event::Off(1<<3),Event::Configure(3,0x1010,11025,0),Event::On(1<<3,0)]);
    audio::nail_kind(3);assert_eq!(take(),[Event::Off(1<<3),Event::Configure(3,0x1030,11025,0),Event::On(1<<3,0)]);
    audio::nail_kind(2);assert_eq!(take(),[Event::Off(1<<3),Event::Configure(3,0x1010+3*32,22050,0),Event::On(1<<3,0)]);
    audio::hero_extra(audio::DASH);assert_eq!(take(),[Event::Off(1),Event::Configure(0,0x1050,11025,0),Event::On(1,0)]);
    unsafe {assert_eq!(audio::HK_HERO_EXTRA_SFX,[1,1,1,0,0,0,0]);assert_eq!(audio::HK_SFX_EVENT_COUNTS[3],4);
        audio::HK_SFX_COUNT-=4;}

    let mut pitches=std::collections::BTreeSet::new();
    for _ in 0..256 {
        audio::great_door_hit();let events=take();
        // The False Knight shares this voice, so the door rewrites its own
        // sample address and rate before the pitch and the key-on.
        assert_eq!(events.len(),4);assert_eq!(events[0],Event::Off(1<<15));
        assert_eq!(events[1],Event::Configure(15,0x1010,22050,0));assert_eq!(events[3],Event::On(1<<15,0));
        let Event::Pitch(v,p)=events[2] else {panic!("pitch must be set before key-on");};
        assert_eq!(v,15);assert!((1741..=2355).contains(&p));pitches.insert(p);
    }
    assert!(pitches.len()>100,"source pitch interval must produce variation");
    unsafe {assert_eq!(audio::HK_GREAT_DOOR_HIT_SFX,256);assert_eq!(audio::HK_SFX_COUNT,262);}

    // The boss takes the same voice back at its own address, rate and no pitch
    // override, and the door's next hit must not inherit either of them.
    for (index,play,address,rate) in [(0usize,audio::boss_land as fn(),0x1010+8*32,11025u32),
                                      (1,audio::boss_swing as fn(),0x1010+9*32,22050)] {
        play();
        assert_eq!(take(),[Event::Off(1<<15),Event::Configure(15,address,rate,0),Event::On(1<<15,0)]);
        unsafe {assert_eq!(audio::HK_BOSS_SFX_COUNTS[index],1);}
    }
    audio::great_door_hit();
    assert_eq!(take()[1],Event::Configure(15,0x1010,22050,0),"the door inherited the boss sample");
    unsafe {assert_eq!(audio::HK_SFX_COUNT,265);assert_eq!(audio::HK_BOSS_SFX_COUNTS,[1,1]);}

    // The world bank: silent and uncounted until it arrives, then every clip
    // on the shared voice at its own address and rate, with the enemy death
    // drawn from its source pitch range and the rest at their fixed pitch.
    audio::enemy_death();audio::ui_confirm();assert!(take().is_empty());
    unsafe {assert_eq!(audio::HK_WORLD_SFX,[0;5]);assert_eq!(audio::HK_SFX_COUNT,265);}
    assert!(!audio::world_ready());audio::upload_world(&WORLD.0);assert!(audio::world_ready());
    assert_eq!(take(),[Event::Upload(0x60000,160)]);
    let mut pitches=std::collections::BTreeSet::new();
    for _ in 0..256 {
        audio::enemy_death();let events=take();
        assert_eq!(events.len(),4);assert_eq!(events[0],Event::Off(1<<15));
        assert_eq!(events[1],Event::Configure(15,0x60000,22050,0));assert_eq!(events[3],Event::On(1<<15,0));
        let Event::Pitch(v,p)=events[2] else {panic!("pitch must be set before key-on");};
        assert_eq!(v,15);assert!((1536..=2560).contains(&p));pitches.insert(p);
    }
    assert!(pitches.len()>100,"source pitch interval must produce variation");
    for (index,play,rate,pitch) in [(1usize,audio::hero_death as fn(),4000u32,372u16),(2,audio::ui_confirm,5512,512),
                                    (3,audio::ui_start,8000,743),(4,audio::cocoon_break,11025,1024)] {
        play();
        assert_eq!(take(),[Event::Off(1<<15),Event::Configure(15,0x60000+index as u32*32,rate,0),Event::Pitch(15,pitch),Event::On(1<<15,0)]);
    }
    unsafe {assert_eq!(audio::HK_WORLD_SFX,[256,1,1,1,1]);assert_eq!(audio::HK_SFX_COUNT,265+260);}
    // And the door still takes its own sample back afterwards.
    audio::great_door_hit();
    assert_eq!(take()[1],Event::Configure(15,0x1010,22050,0),"the door inherited a world sample");

    // Threshold and suppression exercise the actual event->physical voice path.
    audio::reset_movement();assert_eq!(take(),[Event::Off(1<<17)]);
    descent(67);assert!(take().is_empty());
    audio::movement_tick(false,true,false,false,true);assert_eq!(take(),pair(16,0));
    audio::movement_tick(true,true,false,false,true);assert!(take().is_empty());
    audio::reset_movement();take();descent(66);
    audio::movement_tick(false,true,false,false,true);assert_eq!(take(),pair(2,0));
    audio::reset_movement();take();descent(67);
    audio::movement_tick(false,true,false,false,false);assert_eq!(take(),pair(2,0));
    unsafe {assert_eq!(audio::HK_HARD_LAND_SFX,1);assert_eq!(audio::HK_SFX_EVENT_COUNTS[6],1);}

    audio::reset_movement();take();run();assert_eq!(take(),pair(17,0));
    for _ in 0..3 {run();assert!(take().is_empty());}
    run();assert_eq!(take(),pair(17,0)); // Four-tick synthetic complete sequence.
    audio::stop_footsteps();assert_eq!(take(),[Event::Off(1<<17)]);
    run();assert_eq!(take(),pair(17,0));
    audio::movement_tick(true,false,false,false,true);assert_eq!(take(),[Event::Off(1<<17)]);
    descent(3);assert!(take().is_empty());
    audio::movement_tick(false,true,false,true,true);assert_eq!(take(),pair(2,0));
    run();assert!(take().is_empty());run();assert_eq!(take(),pair(17,0)); // Soft voice gates restart.
    unsafe {assert_eq!(audio::HK_FOOTSTEP_STARTS,4);assert_eq!(audio::HK_SFX_EVENT_COUNTS[7],4);}

    // Pause stops playback without discarding fall state; scene reset does.
    audio::reset_movement();take();descent(67);audio::stop_footsteps();take();
    audio::movement_tick(false,true,false,false,true);assert_eq!(take(),pair(16,0));
    descent(67);audio::reset_movement();take();
    audio::movement_tick(false,true,false,false,true);assert_eq!(take(),pair(2,0));

    // Every owned physical voice changes gain, including both new voices and
    // Great Door; keying an event after exact mute cannot restore its gain.
    for level in [5,255,0] {
        audio::set_volume(level);let mut expected=Vec::new();
        for (i,v) in physical.iter().enumerate(){expected.push(Event::Gain(*v,((i as i32+1)*1000*level.min(10) as i32/10) as i16));}
        expected.push(Event::Gain(15,1000*level.min(10) as i16/10));assert_eq!(take(),expected);
    }
    audio::great_door_hit();assert_eq!(take().last(),Some(&Event::On(1<<15,0)));
    audio::reset_movement();take();run();assert_eq!(take(),pair(17,0));
    descent(67);take();audio::movement_tick(false,true,false,false,true);assert_eq!(take(),pair(16,0));
    let t=TRACE.lock().unwrap();for v in physical.into_iter().chain([15]){assert_eq!(t.gains[v as usize],0);}
}
