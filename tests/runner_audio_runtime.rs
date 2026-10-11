//! Production Runner bootstrap and callback orchestration with fake SPU calls.
//! The MMIO envelope-drain loop remains a guest hardware check.
extern crate self as psx_spu;
use std::sync::Mutex;
#[derive(Clone,Copy)] pub struct SpuAddr(u32);
impl SpuAddr {pub fn new(n:u32)->Self{Self(n)}}
#[derive(Clone,Copy)] pub struct Pitch(u16);
impl Pitch {pub fn raw(n:u16)->Self{Self(n)}}
#[derive(Clone,Copy)] pub struct Volume(pub i16);
impl Volume {pub const SILENCE:Self=Self(0);}
#[derive(Clone,Copy)] pub struct Adsr {pub lower:u16,pub upper:u16}
impl Adsr {pub fn sample_one_shot()->Self{Self{lower:15,upper:37}}}
#[derive(Debug,PartialEq)] enum Event {Upload(u32,usize),Start(u8,u32),On(u32),Off(u32)}
static TRACE:Mutex<Vec<Event>>=Mutex::new(Vec::new());
fn record(e:Event){TRACE.lock().unwrap().push(e);}
fn take()->Vec<Event>{std::mem::take(&mut *TRACE.lock().unwrap())}
pub fn upload_adpcm(a:SpuAddr,b:&[u8]){record(Event::Upload(a.0,b.len()));}
pub struct Voice(u8);
impl Voice {
 pub fn new(n:u8)->Self{Self(n)}
 pub fn set_volume(&self,_:Volume,_:Volume){}
 pub fn set_adsr(&self,_:Adsr){}
 pub fn set_pitch(&self,p:Pitch){assert!(p.0>0&&p.0<=0x3fff);}
 pub fn set_start_addr(&self,a:SpuAddr){record(Event::Start(self.0,a.0));}
 pub fn set_loop_addr(&self,_:SpuAddr){}
 pub fn configure_sample(&self,a:SpuAddr,_:u32,_:Volume,_:Adsr){self.set_start_addr(a);}
 pub fn key_on(n:u32){record(Event::On(n));}
 pub fn key_off(n:u32){record(Event::Off(n));}
}
mod input {pub fn checkpoint(){}}
#[path="../game/src/runner_audio.rs"] mod audio;
#[test]
fn actual_bootstrap_rejects_bad_banks_and_dispatches_only_published_family(){
 let bank=include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/runner-audio.adpcm"));
 let mut bad=bank.to_vec();bad[1]=0;
 assert!(std::panic::catch_unwind(||audio::upload(&bad)).is_err());
 assert!(!audio::ready());assert!(take().is_empty());
 assert!(std::panic::catch_unwind(||audio::upload(&bank[..bank.len()-16])).is_err());
 assert!(!audio::ready());assert!(take().is_empty());
 audio::upload(bank);assert!(audio::family_ready(false));
 assert_eq!(take(),vec![Event::Upload(hk_sim::runner_audio::BASE,20464)]);
 audio::call_for(72090,0,true);assert!(take().is_empty());
 assert_eq!(unsafe{audio::HK_RUNNER_AUDIO_FAMILY_MISMATCH},1);
 audio::loop_start(71);audio::call(65536,0);take();audio::reset();
 assert_eq!(take(),vec![Event::Off(1<<21),Event::Off(1<<22),Event::Off(1<<23)]);
 // This publication substitutes the loader only for callback dispatch checks.
 audio::finish_overlay(true);assert!(!audio::family_ready(false));
 audio::loop_start(72);assert!(take().is_empty());
 for variant in 0..3 {audio::call_for(72090,variant,true);let events=take();assert!(events.contains(&Event::On(1<<23)));}
 audio::call_for(72090,3,true);assert!(take().is_empty());
}
