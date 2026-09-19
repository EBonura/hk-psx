extern crate self as psx_spu;
use std::sync::Mutex;
#[derive(Clone, Copy, Debug, PartialEq)] pub struct Volume(pub i16);
#[derive(Clone, Copy)] pub struct SpuAddr(u32);
impl SpuAddr { pub fn new(value:u32)->Self{Self(value)} }
pub struct Adsr;
impl Adsr {pub fn sample_one_shot()->Self{Self}}
pub struct Voice(u8);
#[derive(Default)]struct Trace {uploads:usize,plays:Vec<(u8,u32,u32,i16)>,gains:Vec<(u8,i16)>,on:u32,off:u32}
static TRACE:Mutex<Trace>=Mutex::new(Trace{uploads:0,plays:Vec::new(),gains:Vec::new(),on:0,off:0});
impl Voice {
 pub fn new(index:u8)->Self{assert!((12..=14).contains(&index));Self(index)}
 pub fn configure_sample(self,address:SpuAddr,rate:u32,gain:Volume,_:Adsr){TRACE.lock().unwrap().plays.push((self.0,address.0,rate,gain.0));}
 pub fn set_volume(self,l:Volume,r:Volume){assert_eq!(l,r);TRACE.lock().unwrap().gains.push((self.0,l.0));}
 pub fn key_off(mask:u32){assert_eq!(mask&!0x7000,0);TRACE.lock().unwrap().off|=mask;}
 pub fn key_on(mask:u32){assert_eq!(mask&!0x7000,0);TRACE.lock().unwrap().on|=mask;}
}
pub fn upload_adpcm(address:SpuAddr,bytes:&[u8]){assert_eq!(address.0,0x14000);assert_eq!((bytes.as_ptr() as usize)%4,0);assert!(bytes.len()<=16384);TRACE.lock().unwrap().uploads+=1;}
#[path="../game/src/geo_audio.rs"]mod geo_audio;
// The bank now arrives as a slice from the disc loader. Word alignment is the
// loader's guarantee (the scene arena it stages through), so the fixture keeps
// it rather than relying on include_bytes!'s.
#[repr(align(4))]struct Bank([u8;192]);
static BANK:Bank=Bank(*include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/geo-audio.adpcm")));
#[test]fn actual_runtime_preserves_voice_ownership_variants_mute_and_upload_idempotence(){
 geo_audio::set_volume(0);geo_audio::upload(&BANK.0);geo_audio::upload(&BANK.0);
 for _ in 0..32 {geo_audio::pickup(1);geo_audio::pickup(5);geo_audio::pickup(25);geo_audio::hit();}
 geo_audio::break_rock();
 {
 let t=TRACE.lock().unwrap();assert_eq!(t.uploads,1);assert!(t.plays.iter().all(|p|p.3==0));
 let address=|i:u32|0x14000+i*32;
 for (index,p) in t.plays[..128].chunks_exact(4).enumerate(){
  let _=index;assert!([address(0),address(1)].contains(&p[0].1));
  assert!([address(0),address(2)].contains(&p[1].1));
  assert!([address(0),address(1)].contains(&p[2].1));
  assert!([address(3),address(4),address(5)].contains(&p[3].1));
 }
 for i in 0..6 {assert!(t.plays.iter().any(|p|p.1==address(i)));}
 assert_eq!(t.plays.last().copied(),Some((14,0x1010,22050,0)));assert_eq!(t.on,0x7000);assert_eq!(t.off,0x7000);
 }
 geo_audio::set_volume(5);geo_audio::hit();geo_audio::set_volume(255);geo_audio::pickup(5);
 let t=TRACE.lock().unwrap();assert_eq!(t.plays[t.plays.len()-2].3,2730);assert_eq!(t.plays.last().unwrap().3,5461);
 assert_eq!(t.gains[t.gains.len()-3..],[(12,5461),(13,5461),(14,5461)]);
 unsafe {assert_eq!(geo_audio::HK_GEO_AUDIO_READY,1);assert_eq!(geo_audio::HK_GEO_PICKUP_SFX,97);assert_eq!(geo_audio::HK_GEO_ROCK_HIT_SFX,33);assert_eq!(geo_audio::HK_GEO_ROCK_BREAK_SFX,1);}
}
