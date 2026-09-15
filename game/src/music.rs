//! Direct CD-DA Title playback through the pinned SDK; no song RAM/SPU bank.
use psx_io::{cdrom,cdda::{CddaStarter,CddaEndDetector}};
use psx_spu::{self as spu,CdVolume};
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/title_music.rs"));
static mut STARTER:CddaStarter=CddaStarter::new();
static mut END:CddaEndDetector=CddaEndDetector::new(1);
static mut ENABLED:bool=false;
static mut BEGIN:u32=0;
static mut LAST_POLL:u32=0;
static mut RESTART:bool=false;
static mut LEVEL:u8=10;
static mut FADE:u8=128;
#[no_mangle]pub static mut HK_MUSIC_PLAYING:u32=0;
#[no_mangle]pub static mut HK_MUSIC_WRAPS:u32=0;
#[no_mangle]pub static mut HK_MUSIC_ERROR:u32=0;
#[no_mangle]pub static mut HK_MUSIC_LEVEL:u32=10;
#[no_mangle]pub static mut HK_MUSIC_POSITION_SECTORS:u32=0;
const SPINS:u32=131072;
pub fn set_volume(level:u8){unsafe{LEVEL=level.min(10);HK_MUSIC_LEVEL=LEVEL as u32;}apply_volume();}
pub fn set_fade(gain:u8){unsafe{FADE=gain.min(128);}apply_volume();}
fn apply_volume(){let gain=CdVolume((i32::from(TITLE_GAIN)*i32::from(unsafe{LEVEL})*i32::from(unsafe{FADE})/1280)as i16);spu::set_cd_volume(gain,gain);}
pub fn begin(){
 let now=psx_rt::interrupts::vblank_count();
 unsafe{ENABLED=true;BEGIN=now;LAST_POLL=now;RESTART=false;END.rearm();FADE=128;HK_MUSIC_ERROR=0;HK_MUSIC_WRAPS=0;HK_MUSIC_PLAYING=0;HK_MUSIC_POSITION_SECTORS=0;
  // SDK waits45ticks beforeSetMode. Offset its start so the source's1s delay
  // precedes the handshake; physical spin-up/seek can add further latency.
  STARTER.begin(now.wrapping_add(TITLE_DELAY.saturating_sub(45)));}
 apply_volume();spu::enable_cd_audio(true);
}
fn fail(){unsafe{HK_MUSIC_ERROR=1;HK_MUSIC_PLAYING=0;ENABLED=false;}spu::enable_cd_audio(false);}
pub fn tick(){
 if !unsafe{ENABLED}{return;}
 let now=psx_rt::interrupts::vblank_count();
 if !unsafe{STARTER.started()} {
  if unsafe{STARTER.tick(now,TITLE_TRACK)} {
   // Automatic track-boundary pause prevents playback running into lead-out.
   if cdrom::try_set_mode(cdrom::MODE_CDDA|cdrom::MODE_AUTO_PAUSE,SPINS).is_none(){fail();return;}
   let _=cdrom::try_get_stat(SPINS);
  }else if now.wrapping_sub(unsafe{BEGIN})>600{fail();}
  return;
 }
 if now.wrapping_sub(unsafe{LAST_POLL})<6{return;}
 unsafe{LAST_POLL=now;}
 if unsafe{RESTART}{
  if cdrom::try_play_track(TITLE_TRACK,SPINS).is_some(){unsafe{RESTART=false;END.rearm();HK_MUSIC_WRAPS+=1;}}
  let _=cdrom::try_get_stat(SPINS);return;
 }
 let status=cdrom::try_get_stat(SPINS).and_then(|r|r.bytes().first().copied());
 unsafe{HK_MUSIC_PLAYING=u32::from(status.is_some_and(|s|s&cdrom::STAT_PLAYING!=0));}
 if unsafe{END.poll(status)}{unsafe{RESTART=true;}return;}
 if unsafe{HK_MUSIC_PLAYING}!=0 {
  if let Some(pos)=cdrom::try_get_loc_p(SPINS).and_then(|r|cdrom::PlayPosition::parse(&r)) {unsafe{HK_MUSIC_POSITION_SECTORS=pos.relative_sectors();}}
 }
}
/// Release the drive before any room/data command; Pause completion keeps its
/// motor spinning. Never confuse an accepted Stop with completed hardware stop.
pub fn stop()->bool{
 unsafe{ENABLED=false;HK_MUSIC_PLAYING=0;}
 spu::set_cd_volume(CdVolume::SILENCE,CdVolume::SILENCE);spu::enable_cd_audio(false);
 let okay=cdrom::try_pause_until_complete(4_000_000);
 if !okay{unsafe{HK_MUSIC_ERROR=2;}}
 okay
}
