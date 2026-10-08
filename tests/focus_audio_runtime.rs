//! Execute the production Focus/SPU orchestration against bounded fake registers.
extern crate self as psx_spu;
use std::sync::Mutex;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct SpuAddr(u32);
impl SpuAddr {pub fn new(value:u32)->Self {assert!(value%8==0&&value<0x80000);Self(value)}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct Volume(pub i16);
impl Volume {pub const SILENCE:Self=Self(0);}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct Adsr(u16,u16);
impl Adsr {pub fn sample_one_shot()->Self {Self(0x000f,0x0025)}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum Event {Upload(u32,usize),Configure(u8,u32,u32,Volume,Adsr),Loop(u8,u32),Volume(u8,Volume,Volume),On(u32),Off(u32)}
static TRACE:Mutex<Vec<Event>>=Mutex::new(Vec::new());
fn record(event:Event){let mut trace=TRACE.lock().unwrap();assert!(trace.len()<2048,"unbounded register traffic");trace.push(event);}
fn take()->Vec<Event>{std::mem::take(&mut *TRACE.lock().unwrap())}
pub fn upload_adpcm(addr:SpuAddr,bytes:&[u8]){assert!(bytes.len()%16==0&&addr.0+bytes.len()as u32<=0x80000);record(Event::Upload(addr.0,bytes.len()));}
pub struct Voice(u8);
impl Voice {
 pub fn new(id:u8)->Self {assert!(id<24);Self(id)}
 pub fn configure_sample(&self,addr:SpuAddr,rate:u32,gain:Volume,adsr:Adsr){record(Event::Configure(self.0,addr.0,rate,gain,adsr));}
 pub fn set_loop_addr(&self,addr:SpuAddr){record(Event::Loop(self.0,addr.0));}
 pub fn set_volume(&self,left:Volume,right:Volume){assert!(left.0>=0&&right.0>=0);record(Event::Volume(self.0,left,right));}
 pub fn key_on(mask:u32){assert!(mask&!0xFFFFFF==0);record(Event::On(mask));}
 pub fn key_off(mask:u32){assert!(mask&!0xFFFFFF==0);record(Event::Off(mask));}
}
pub mod ambience {
 pub struct Clip {pub spu_address:u32,pub spu_bytes:usize}
 // The generated Focus bank begins exactly where ambience ends, and where that
 // is moves whenever the resident atmos set does. Take it from the Focus
 // manifest rather than restating an address ambience has already left behind,
 // which also keeps `focus_audio::ready`'s overlap check on its boundary.
 pub const CLIPS:[Clip;1]=[Clip {spu_address:0x18000,spu_bytes:(crate::audio::SPU_BASE-0x18000) as usize}];
}
#[path="../game/src/focus_audio.rs"]mod audio;
use hk_sim::{Focus,FocusEvents,FocusInput,FocusParams,VitalParams,Vitals};
const P:FocusParams=FocusParams {hold_ticks:15,start_ticks:15,heal_ticks:12,cancel_ticks:15,finish_ticks:14,
 first_grace_ticks:12,repeat_grace_ticks:27,drain_interval_us:27000,cost:33,heal_amount:1,attack_recovery_ticks:6};
const V:VitalParams=VitalParams {max_health:5,max_soul:99,nail_damage:5,soul_per_hit:11,invulnerable_ticks:79,
 hazard_invulnerable_ticks:40,recoil_ticks:12,freeze_ticks:19,death_ticks:171,recoil_speed:15*65536};
fn tick(focus:&mut Focus,vitals:&mut Vitals,held:bool)->FocusEvents {
 let event=focus.step(P,V,FocusInput {held,grounded:true,can_start:true},vitals);audio::tick(focus,event);event
}
fn setup(health:u16)->(Focus,Vitals){let mut v=Vitals::new(V);v.health=health;v.soul=99;(Focus::new(),v)}
fn count_on(events:&[Event],voice:u8)->usize{events.iter().filter(|e|**e==Event::On(1<<voice)).count()}
#[test]fn full_bank_and_actual_focus_lifecycle_obey_spu_contract(){
 let bank=include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/focus-audio.adpcm"));
 assert!(!audio::ready());
 assert!(!audio::upload(&bank[..bank.len()-1]));
 let mut damaged=bank.to_vec();damaged[32]^=1;assert!(!audio::upload(&damaged));
 audio::tick(&Focus::new(),FocusEvents {started:true,completed:true,..FocusEvents::default()});
 assert!(take().is_empty(),"bad/unavailable bank must cause no hardware calls");
 assert!(audio::upload(bank));assert!(audio::ready());
 let events=take();assert_eq!(events,vec![
  Event::Upload(audio::SPU_BASE,bank.len()),
  Event::Configure(18,audio::SPU_BASE,audio::CHARGE_RATE,Volume::SILENCE,Adsr(0x000f,0x0025)),
  Event::Loop(18,audio::SPU_BASE),
  Event::Configure(19,audio::SPU_BASE+audio::CHARGE_BYTES as u32,audio::HEAL_RATE,Volume(audio::GAIN),Adsr(0x000f,0x0025)),
  Event::Configure(20,audio::SPU_BASE+audio::CHARGE_BYTES as u32,audio::HEAL_RATE,Volume(audio::GAIN),Adsr(0x000f,0x0025))]);
 // A continuous hold crosses two real heal completions. Neither restarts charge.
 let(mut focus,mut vitals)=setup(2);for _ in 0..150 {tick(&mut focus,&mut vitals,true);}
 assert_eq!(vitals.health,4);let events=take();
 assert_eq!(count_on(&events,18),1);assert_eq!(count_on(&events,19),1);assert_eq!(count_on(&events,20),1);
 assert_eq!(events.iter().filter(|e|matches!(e,Event::Upload(..))).count(),0);
 assert_eq!(events.iter().filter(|e|**e==Event::Off(1<<18)).count(),1,"only restart safety key-off, no heal boundary stop");
 // Muting affects the active charge and both independent heal tails, without rekeying.
 audio::set_volume(0);assert_eq!(take(),vec![Event::Volume(18,Volume(0),Volume(0)),Event::Volume(19,Volume(0),Volume(0)),Event::Volume(20,Volume(0),Volume(0))]);
 audio::set_volume(5);let half=Volume(audio::GAIN/2);assert_eq!(take(),vec![Event::Volume(18,half,half),Event::Volume(19,half,half),Event::Volume(20,half,half)]);
 audio::set_volume(255);let full=Volume(audio::GAIN);assert_eq!(take(),vec![Event::Volume(18,full,full),Event::Volume(19,full,full),Event::Volume(20,full,full)]);
 // Damage/scene interruption stops only charge. Existing heal tails survive.
 focus.interrupt();audio::interrupt();assert_eq!(take(),vec![Event::Volume(18,Volume(0),Volume(0)),Event::Off(1<<18)]);
 for _ in 0..40 {tick(&mut focus,&mut vitals,true);}assert!(take().is_empty());
 // Source release fades for the animation duration, then forces zero on exit.
 let(mut focus,mut vitals)=setup(3);for _ in 0..35 {tick(&mut focus,&mut vitals,true);}take();
 tick(&mut focus,&mut vitals,false);assert!(take().is_empty());
 for age in 1..15 {tick(&mut focus,&mut vitals,false);let gain=Volume((audio::GAIN as i32*((20-age)*128/20)/128)as i16);assert_eq!(take(),vec![Event::Volume(18,gain,gain)]);}
 tick(&mut focus,&mut vitals,false);assert_eq!(take(),vec![Event::Off(1<<18),Event::Volume(18,Volume(0),Volume(0))]);
 // Full-health completion must still sound, even when no HP is added.
 let(mut focus,mut vitals)=setup(5);let mut completion=None;
 for _ in 0..84 {let event=tick(&mut focus,&mut vitals,true);if event.completed {completion=Some(event);}}
 assert_eq!(completion.unwrap().healed,0);let events=take();assert_eq!(count_on(&events,18),1);assert_eq!(count_on(&events,19),1);
 audio::interrupt();assert!(take().iter().all(|e|!matches!(e,Event::Off(mask) if mask&((1<<19)|(1<<20))!=0)));
}
