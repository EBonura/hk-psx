//! Music. The title and the fights play XA-ADPCM through the pinned SDK's
//! player: the drive decodes the song and the CPU never sees the audio, and
//! neither the title nor a fight reads data while it plays. Area music is a
//! mono ADPCM stream on one SPU voice (audio_stream.rs), fed from a RAM FIFO
//! that the CD refills between room loads, so it plays on through a load from
//! what is buffered. The drive itself is scheduled by `disc`: room loads,
//! music refills and XA never overlap.
use psx_io::cdrom;
#[path="xa_player.rs"]mod xa_player;
use xa_player::{Event,Player,Song};
use psx_spu::{self as spu,Adsr,CdVolume,Voice,Volume};
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/xa_music.rs"));
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/area_music.rs"));
use crate::ambience::{MUSIC_VOICE,MUSIC_RING_BASE,MUSIC_RING_BYTES};
#[path="volume.rs"]mod volume;
#[path="audio_stream.rs"]pub mod stream;
use stream::{Cursor,Fifo,CHUNK_SECTORS,HALF_SECTORS};
const _:()=assert!(MUSIC_RING_BYTES==stream::RING_BYTES);
// The controller is driven by this player only while a song plays: `disc` hands
// the drive over (`cdda_acquire`) before a fight, and music::begin owns it at
// the title, so nothing else programs it meanwhile.
static mut PLAYER:Player=Player::new();
fn player()->&'static mut Player{unsafe{&mut *(&raw mut PLAYER)}}
/// Where MUSIC.XA sits on the disc (relative to this program's image), once
/// `disc` has found it in the directory.
static mut XA_LBA:u32=0;
/// Remember the directory entry of MUSIC.XA; refuse one that is not the file the cook wrote.
/// Where MUSIC.XA starts (relative to this program's image), 0 until found.
pub fn xa_lba()->u32{unsafe{XA_LBA}}
pub fn set_xa_file(lba:u32,size_bytes:u32){
 if size_bytes/2048==XA_SECTORS {unsafe{XA_LBA=lba;}} else {unsafe{HK_MUSIC_ERROR=5;}}
}
/// A song of the file as the player takes it: it ends where its own audio does.
fn song(id:u8)->Option<Song>{
 let lba=unsafe{XA_LBA};
 let s=XA_SONGS.get(id as usize)?;
 if lba==0 {return None;}
 Some(Song{lba,span:s.span,file:XA_FILE_NUMBER,channel:s.channel})
}
/// Start song `id` from the top.
fn play(id:u8,looping:bool)->bool{
 let Some(song)=song(id) else {return false};
 player().play(song,looping)
}
static mut ENABLED:bool=false;
static mut LAST_POLL:u32=0;
/// The title's song starts here (the source's one second delay), and has.
static mut START_AT:u32=0;
static mut STARTED:bool=false;
static mut LEVEL:u8=10;
static mut FADE:u8=128;
#[no_mangle]pub static mut HK_MUSIC_PLAYING:u32=0;
#[no_mangle]pub static mut HK_MUSIC_WRAPS:u32=0;
#[no_mangle]pub static mut HK_MUSIC_ERROR:u32=0;
#[no_mangle]pub static mut HK_MUSIC_LEVEL:u32=10;
#[no_mangle]pub static mut HK_MUSIC_POSITION_SECTORS:u32=0;
/// Spins the drive gets to answer a pause.
const SPINS:u32=4_000_000;
pub fn set_volume(level:u8){unsafe{LEVEL=level.min(10);HK_MUSIC_LEVEL=LEVEL as u32;}apply_volume();unsafe{AREA.written=i16::MIN;}}
pub fn set_fade(gain:u8){unsafe{FADE=gain.min(128);}apply_volume();}
fn apply_volume(){
 // The title's fade-out leaves FADE at zero; the fight's song has no fade.
 let (gain,fade)=if unsafe{BOSS.phase}!=Boss::Off {(BOSS_GAIN,unsafe{BOSS_FADE})} else {(TITLE_GAIN,unsafe{FADE})};
 let gain=CdVolume((i32::from(gain)*i32::from(unsafe{LEVEL})*i32::from(fade)/1280)as i16);spu::set_cd_volume(gain,gain);
}
pub fn begin(){
 let now=psx_rt::interrupts::vblank_count();
 unsafe{ENABLED=true;LAST_POLL=now;FADE=128;HK_MUSIC_WRAPS=0;HK_MUSIC_PLAYING=0;HK_MUSIC_POSITION_SECTORS=0;
  HK_MUSIC_ERROR=if XA_LBA==0 {5} else {0};
  // The source's 1s delay before the song starts; spin-up and seek add their own.
  START_AT=now.wrapping_add(TITLE_DELAY);STARTED=false;}
 apply_volume();spu::enable_cd_audio(true);
}
fn fail(){unsafe{HK_MUSIC_ERROR=1;HK_MUSIC_PLAYING=0;ENABLED=false;}spu::enable_cd_audio(false);}
pub fn tick(){
 if !unsafe{ENABLED}{return;}
 let now=psx_rt::interrupts::vblank_count();
 if !unsafe{STARTED} {
  if now.wrapping_sub(unsafe{START_AT})>u32::MAX/2 {return;}
  if !play(TITLE_TRACK,true) {fail();return;}
  unsafe{STARTED=true;LAST_POLL=now;}return;
 }
 // Once a second, as in a fight: the song ends well inside a file that runs on in
 // silence, so noticing the end needs no faster poll, and every GetlocP is a drive command.
 if now.wrapping_sub(unsafe{LAST_POLL})<60{return;}
 unsafe{LAST_POLL=now;}
 // A loop is the player's own restart; the song ends where its audio does, so
 // the head never leaves the file between two polls.
 if player().poll()==Event::Looped {unsafe{HK_MUSIC_WRAPS+=1;}}
 unsafe{HK_MUSIC_PLAYING=u32::from(player().is_streaming());}
 if unsafe{HK_MUSIC_PLAYING}!=0 {unsafe{HK_MUSIC_POSITION_SECTORS=player().elapsed_millis()*75/1000;}}
}
/// Release the drive before any room/data command; Pause completion keeps its
/// motor spinning. Never confuse an accepted Stop with completed hardware stop.
pub fn stop()->bool{
 unsafe{ENABLED=false;HK_MUSIC_PLAYING=0;}
 spu::set_cd_volume(CdVolume::SILENCE,CdVolume::SILENCE);spu::enable_cd_audio(false);
 // The player's pause is refused while a read is still seeking; this one waits.
 player().stop();
 let okay=cdrom::try_pause_until_complete(SPINS);
 if !okay{unsafe{HK_MUSIC_ERROR=2;}}
 okay
}

// ---------------------------------------------------------------------------
// Area music.

/// Refill below this many buffered sectors between a load's room reads (3.3 s).
const LOW_SECTORS:usize=20;
/// A cue change fades the old stream out at least this fast.
const CHANGE_FADE:u32=30;
#[no_mangle]pub static mut HK_MUSIC_TRACK:u32=u32::MAX;
#[no_mangle]pub static mut HK_MUSIC_FAMILY:u32=u32::MAX;
#[no_mangle]pub static mut HK_MUSIC_SNAPSHOT:u32=u32::MAX;
#[no_mangle]pub static mut HK_MUSIC_VOLUME:u32=0;
/// Buffered sectors: the least seen while the stream played, and now.
#[no_mangle]pub static mut HK_MUSIC_FIFO_MIN:u32=u32::MAX;
#[no_mangle]pub static mut HK_MUSIC_FIFO_FILL:u32=0;
#[no_mangle]pub static mut HK_MUSIC_READS:u32=0;
#[no_mangle]pub static mut HK_MUSIC_READ_SECTORS:u32=0;
/// Refills a room load made room for because the FIFO ran low mid-load.
#[no_mangle]pub static mut HK_MUSIC_LOAD_READS:u32=0;
#[no_mangle]pub static mut HK_MUSIC_READ_ERRORS:u32=0;
#[no_mangle]pub static mut HK_MUSIC_LOOPS:u32=0;
/// A (family, snapshot) state the cook did not premix: played as silence.
#[no_mangle]pub static mut HK_MUSIC_UNCOOKED:u32=0;
#[no_mangle]pub static mut HK_MUSIC_BOSS:u32=0;
/// The XA channel the last fight started: 1 the False Knight, 2 Brooding Mawlek, 3 the defeat sting.
#[no_mangle]pub static mut HK_MUSIC_BOSS_TRACK:u32=0;
#[no_mangle]pub static mut HK_MUSIC_BOSS_STARTS:u32=0;

struct Area {
    family:u8,snapshot:u8,scene:u16,inside:u8,
    /// The premix the FIFO is being filled from and the family it belongs to,
    /// and one of the same family to switch to at the next refill, at the
    /// same offset.
    track:u8,track_family:u8,pending:u8,
    /// A premix of another family, waiting for the old stream to fade out,
    /// with the volume and fade it enters with.
    next:u8,next_volume:i16,next_fade:u32,
    fifo:Fifo,cursor:Cursor,
    /// A linear ramp on VBlanks; `fade_start` in the future holds `from`.
    volume:i16,from:i16,target:i16,fade_start:u32,fade_len:u32,
    start_at:u32,written:i16,last:u32,
    /// Sectors of the read in flight, for `read_done`.
    reading:usize,
    /// Fading out ahead of a gate: no more refills for this track.
    draining:bool,
}
static mut AREA:Area=Area {family:MUSIC_KEEP,snapshot:MUSIC_KEEP,scene:u16::MAX,inside:0,track:MUSIC_KEEP,
    track_family:MUSIC_KEEP,pending:MUSIC_KEEP,next:MUSIC_KEEP,next_volume:0,next_fade:0,fifo:Fifo::new(),
    cursor:Cursor::new(0),volume:0,from:0,target:0,fade_start:0,fade_len:0,start_at:0,written:i16::MIN,
    last:u32::MAX,reading:0,draining:false};
#[derive(Clone,Copy,PartialEq,Eq)]enum Boss {Off,Starting,Playing}
struct BossState {want:bool,phase:Boss,poll:u32,fade_start:u32,fade_len:u16,track:u8,once:bool}
static mut BOSS:BossState=BossState {want:false,phase:Boss::Off,poll:0,fade_start:0,fade_len:0,track:BOSS_TRACK,once:false};
/// A boss fight wants or holds the XA song.
pub fn boss_active()->bool {unsafe {BOSS.want||BOSS.phase!=Boss::Off}}
/// The fight's XA gain while `Floor Break`'s snapshot fades it (128 is 1.0).
static mut BOSS_FADE:u8=128;
/// An area snapshot no cooked mix uses: `Silent`, which `mix` answers with
/// silence and which differs from every real snapshot, so the next scene or
/// music region that applies one is heard again.
const SILENT_SNAPSHOT:u8=MUSIC_KEEP-1;
const _:()=assert!(SILENT_SNAPSHOT as usize>=MUSIC_SNAPSHOTS);
/// Where each premix starts on the disc, filled in once the directory is read.
static mut TRACK_LBA:[u32;MUSIC_TRACKS.len()]=[0;MUSIC_TRACKS.len()];
pub fn set_track_lba(track:usize,lba:u32){unsafe{TRACK_LBA[track]=lba;}}

fn area()->&'static mut Area{unsafe{&mut *(&raw mut AREA)}}
fn mix(family:u8,snapshot:u8)->MusicMix {
    if family==MUSIC_KEEP||snapshot==MUSIC_KEEP||family as usize>=MUSIC_MIXES.len()||snapshot as usize>=MUSIC_SNAPSHOTS {
        return MusicMix{track:MUSIC_KEEP,volume:0};
    }
    MUSIC_MIXES[family as usize][snapshot as usize]
}
fn fade_to(a:&mut Area,target:i16,ticks:u32,at:u32){
    a.from=a.volume;a.target=target;a.fade_start=at;a.fade_len=ticks;
}
fn switch_track(a:&mut Area,track:u8,family:u8){
    stream::stop();a.draining=false;a.fifo=Fifo::new();a.track=track;a.track_family=family;a.pending=MUSIC_KEEP;
    a.cursor=Cursor::new(MUSIC_TRACKS[track as usize].sectors);
    unsafe{HK_MUSIC_TRACK=track as u32;}
}
/// Enter a new (family, snapshot) state, from a scene or a music region.
fn apply(state:MusicState,delay:u32){
    let a=area();let now=psx_rt::interrupts::vblank_count();
    let family=if state.family==MUSIC_KEEP {a.family} else {state.family};
    let snapshot=if state.snapshot==MUSIC_KEEP {a.snapshot} else {state.snapshot};
    if family==a.family&&snapshot==a.snapshot {return;}
    a.family=family;a.snapshot=snapshot;
    unsafe{HK_MUSIC_FAMILY=family as u32;HK_MUSIC_SNAPSHOT=snapshot as u32;}
    let m=mix(family,snapshot);let fade=state.fade_ticks as u32;
    if m.track==MUSIC_KEEP {
        // Silence: fade out and leave the stream where it is, to resume there.
        if family!=MUSIC_KEEP&&snapshot!=MUSIC_KEEP&&m.volume!=0 {unsafe{HK_MUSIC_UNCOOKED+=1;}}
        a.next=MUSIC_KEEP;fade_to(a,0,fade,now);return;
    }
    if a.track==MUSIC_KEEP {
        // Nothing buffered yet: buffer this one and fade it in after the delay.
        switch_track(a,m.track,family);a.volume=0;a.start_at=now.wrapping_add(delay);
        fade_to(a,m.volume,fade,a.start_at);return;
    }
    // Audible again: a gate that faded this stream out for a silent scene
    // stopped its refills, and whatever plays next needs them back.
    a.draining=false;
    if m.track==a.track {a.next=MUSIC_KEEP;a.pending=MUSIC_KEEP;fade_to(a,m.volume,fade,now);return;}
    if family==a.track_family {
        // Same cue, another layer set: the refill switches premix at the same
        // offset, heard once the audio buffered ahead of it has played.
        a.next=MUSIC_KEEP;a.pending=m.track;fade_to(a,m.volume,fade,now);return;
    }
    // Another cue: what plays fades out, then the new premix is buffered and
    // fades in behind it after the scene's own delay.
    a.next=m.track;a.next_volume=m.volume;a.next_fade=fade;a.start_at=now.wrapping_add(delay);
    fade_to(a,0,fade.min(CHANGE_FADE),now);
}
/// A scene gate is loading `scene`. If the stream playing now is not the one
/// that scene continues, fade it out over the load and stop refilling it, so
/// the load's room reads never wait on music nobody will hear.
pub fn begin_gate(scene:usize){
    let a=area();
    if scene>=MUSIC_SCENES.len()||a.track==MUSIC_KEEP {return;}
    let s=MUSIC_SCENES[scene].state;
    let family=if s.family==MUSIC_KEEP {a.family} else {s.family};
    let snapshot=if s.snapshot==MUSIC_KEEP {a.snapshot} else {s.snapshot};
    let m=mix(family,snapshot);
    if m.track!=MUSIC_KEEP&&(m.track==a.track||family==a.track_family) {return;}
    let now=psx_rt::interrupts::vblank_count();
    fade_to(a,0,CHANGE_FADE,now);a.draining=true;
}
/// The scene a gate or boot just made resident: apply its SceneManager state.
pub fn enter_scene(scene:usize){
    let a=area();
    if scene>=MUSIC_SCENES.len()||a.scene==scene as u16 {return;}
    a.scene=scene as u16;a.inside=0;
    let s=MUSIC_SCENES[scene];apply(s.state,s.delay_ticks as u32);
}
/// Once per simulation tick: music regions the Knight enters or leaves.
pub fn tick_regions(x:i32,y:i32){
    let scene=area().scene;let mut index=0u8;
    for r in MUSIC_REGIONS.iter() {
        if r.scene!=scene {continue;}
        let bit=1u8<<index;index+=1;
        let b=r.box_;let inside=x>=b[0]&&x<=b[2]&&y>=b[1]&&y<=b[3];
        let was=area().inside&bit!=0;
        if inside&&!was {area().inside|=bit;apply(r.enter,0);}
        else if !inside&&was {area().inside&=!bit;apply(r.exit,0);}
    }
}
/// The False Knight's fight starting or ending. Acted on by `service`.
pub fn boss(active:bool){
    if active {boss_track(BOSS_TRACK);} else {unsafe{BOSS.want=false;}}
}
/// A fight's own XA song starting: Boss1 for the False Knight, EnemyBattle
/// (`MAWLEK_TRACK`) for Brooding Mawlek. Ended by `boss(false)` or faded out
/// by `boss_silence`.
pub fn boss_track(track:u8){unsafe{BOSS.want=true;BOSS.fade_len=0;BOSS.track=track;BOSS.once=false;}}
/// A victory sting (`BOSS_DEFEAT_TRACK`): the same XA path, played once and
/// then released rather than looped.
pub fn sting(track:u8){unsafe{BOSS.want=true;BOSS.fade_len=0;BOSS.track=track;BOSS.once=true;}}
/// `Floor Break`'s `TransitionToAudioSnapshot` to `Silent` over `ticks`: the
/// fight's XA fades out and stops, and the area music stays silent until a
/// scene or a music region applies a snapshot of its own.
pub fn boss_silence(ticks:u16){
    let now=psx_rt::interrupts::vblank_count();
    unsafe{
        if BOSS.phase!=Boss::Off {BOSS.fade_start=now;BOSS.fade_len=ticks.max(1);}
        else {BOSS.want=false;}
        HK_MUSIC_SNAPSHOT=SILENT_SNAPSHOT as u32;
    }
    let a=area();a.snapshot=SILENT_SNAPSHOT;a.next=MUSIC_KEEP;a.pending=MUSIC_KEEP;fade_to(a,0,ticks as u32,now);
}
fn write_volume(a:&mut Area){
    let v=volume::scale(a.volume,unsafe{LEVEL});
    if v!=a.written {a.written=v;let g=Volume(v);Voice::new(MUSIC_VOICE).set_volume(g,g);}
    unsafe{HK_MUSIC_VOLUME=a.volume as u32;}
}
/// The refill a free drive should make now, as (destination, sectors, LBA).
/// `urgent` asks only for the read a room load should make room for.
pub fn want_read(urgent:bool)->Option<(*mut u32,usize,u32)> {
    let a=area();
    if unsafe{BOSS.phase}!=Boss::Off||unsafe{BOSS.want}||a.track==MUSIC_KEEP||a.reading!=0||a.draining {return None;}
    if a.pending!=MUSIC_KEEP {
        let track=a.pending;a.pending=MUSIC_KEEP;a.track=track;
        let sectors=MUSIC_TRACKS[track as usize].sectors;a.cursor.sectors=sectors;
        if a.cursor.next>=sectors {a.cursor.next=0;}
        unsafe{HK_MUSIC_TRACK=track as u32;}
    }
    let playing=stream::running();
    if urgent&&!(playing&&a.fifo.filled<LOW_SECTORS) {return None;}
    // Whole chunks only while there is time: one seek buys 2.6 s.
    if !urgent&&playing&&a.fifo.free()<CHUNK_SECTORS {return None;}
    let n=a.cursor.span(&a.fifo,CHUNK_SECTORS);
    if n==0 {return None;}
    let lba=unsafe{TRACK_LBA[a.track as usize]};
    if lba==0 {return None;}
    a.reading=n;
    Some((stream::fifo_sector(a.fifo.write),n,lba+a.cursor.next))
}
/// Buffered music a room prefetch may borrow the drive against: a prefetch
/// holds the drive for at most a scene group (about 1.5 s with its seek),
/// so the stream must have well over that queued before one starts.
const PREFETCH_FIFO_MIN:usize=32;
/// True when the stream can go without a refill for a whole room prefetch.
pub fn fifo_comfortable()->bool {
    let a=area();
    !stream::running()||a.fifo.filled>=PREFETCH_FIFO_MIN
}
/// Buffered music outlasts a gate read of `sectors` and the rest of the gate
/// after it, so the refill can wait for the gate's end instead of costing two
/// seeks now. Music plays about 6 sectors a second and the drive reads 150:
/// four sectors (0.65 s) cover the seek and the decode after the read, and
/// one more per 8 read sectors is about twice the read's own time. A read of
/// 128 sectors or more needs LOW_SECTORS, the plain urgency rule.
pub fn outlasts_read(sectors:usize)->bool {
    let a=area();
    !stream::running()||a.fifo.filled>=(4+sectors/8).min(LOW_SECTORS)
}
/// The read `want_read` handed out has finished.
pub fn read_done(ok:bool,during_load:bool){
    let a=area();let n=a.reading;a.reading=0;
    if n==0 {return;}
    if !ok {unsafe{HK_MUSIC_READ_ERRORS+=1;}return;}
    if a.fifo.commit(n)&&a.cursor.advance(n) {unsafe{HK_MUSIC_LOOPS+=1;}}
    unsafe{HK_MUSIC_READS+=1;HK_MUSIC_READ_SECTORS+=n as u32;if during_load{HK_MUSIC_LOAD_READS+=1;}}
}
/// The read `want_read` handed out was stopped after `landed` of its sectors
/// (a gate took the drive): keep those, the rest is read again later.
pub fn read_cut(landed:usize){
    let a=area();let n=a.reading;a.reading=0;
    if n==0||landed==0 {return;}
    if a.fifo.commit(landed.min(n))&&a.cursor.advance(landed.min(n)) {unsafe{HK_MUSIC_LOOPS+=1;}}
    unsafe{HK_MUSIC_READS+=1;HK_MUSIC_READ_SECTORS+=landed.min(n) as u32;HK_MUSIC_CUT_READS+=1;}
}
#[no_mangle]pub static mut HK_MUSIC_CUT_READS:u32=0;
/// Every checkpoint; does its work at most once per VBlank.
pub fn service(){
    let now=psx_rt::interrupts::vblank_count();
    let a=area();
    if stream::running() {
        if matches!(stream::service(&mut a.fifo,now),stream::Service::Fault) {a.written=i16::MIN;}
        unsafe{HK_MUSIC_FIFO_MIN=HK_MUSIC_FIFO_MIN.min(a.fifo.filled as u32);}
    }
    if a.last==now {return;}
    a.last=now;
    unsafe{HK_MUSIC_FIFO_FILL=a.fifo.filled as u32;}
    boss_service(now);
    // The ramp runs on VBlanks, so a fade keeps going through a load.
    let elapsed=now.wrapping_sub(a.fade_start);
    a.volume=if elapsed>u32::MAX/2 {a.from} else if a.fade_len==0||elapsed>=a.fade_len {a.target}
        else {(a.from as i32+(a.target as i32-a.from as i32)*elapsed as i32/a.fade_len as i32) as i16};
    if a.next!=MUSIC_KEEP&&a.volume==0&&a.reading==0 {
        let (next,family)=(a.next,a.family);a.next=MUSIC_KEEP;switch_track(a,next,family);
        a.volume=0;fade_to(a,a.next_volume,a.next_fade,if a.start_at.wrapping_sub(now)>u32::MAX/2 {now} else {a.start_at});
    }
    if a.volume==0&&a.target==0&&stream::running() {
        // Silent under its snapshot: stop feeding the voice. The FIFO keeps
        // what it holds, so the cue resumes where it went quiet.
        stream::stop();
    }
    let idle=unsafe{BOSS.phase}==Boss::Off&&!unsafe{BOSS.want};
    // Start with the ring's two halves and a whole refill chunk behind them,
    // or the whole loop when it is shorter: a start never leaves the FIFO
    // thinner than what one refill brings back.
    let primed=a.fifo.filled>=(2*HALF_SECTORS+CHUNK_SECTORS).min(a.cursor.sectors as usize);
    if idle&&a.track!=MUSIC_KEEP&&!stream::running()&&a.target!=0&&primed&&a.fifo.filled>=2*HALF_SECTORS
        &&now.wrapping_sub(a.start_at)<u32::MAX/2 {
        let voice=Voice::new(MUSIC_VOICE);voice.set_volume(Volume::SILENCE,Volume::SILENCE);
        voice.set_adsr(Adsr::sample_one_shot());a.written=i16::MIN;
        let _=stream::start(&mut a.fifo,MUSIC_RING_BASE);
    }
    if stream::running() {write_volume(a);}
    crate::disc::pump();
}
fn boss_service(now:u32){
    let b=unsafe{&mut *(&raw mut BOSS)};
    if b.fade_len!=0&&b.phase!=Boss::Off {
        let elapsed=now.wrapping_sub(b.fade_start);
        if elapsed>=b.fade_len as u32 {b.want=false;b.fade_len=0;}
        else {unsafe{BOSS_FADE=(128*(b.fade_len as u32-elapsed)/b.fade_len as u32) as u8;}apply_volume();}
    }
    match (b.want,b.phase) {
        (true,Boss::Off)=>{
            // The fight reads nothing, so XA can have the drive once any
            // refill in flight has landed.
            if !crate::disc::cdda_acquire() {return;}
            stream::stop();area().written=i16::MIN;
            b.phase=Boss::Starting;unsafe{HK_MUSIC_BOSS=1;HK_MUSIC_BOSS_TRACK=b.track as u32;BOSS_FADE=128;}
            apply_volume();spu::enable_cd_audio(true);
        }
        (true,Boss::Starting)=>{
            if !play(b.track,!b.once) {unsafe{HK_MUSIC_ERROR=3;}}
            b.phase=Boss::Playing;b.poll=now;
            unsafe{HK_MUSIC_BOSS_STARTS+=1;}
        }
        (true,Boss::Playing)=>{
            // Rare, bounded polls: the song ends where its audio does and the
            // file runs on in silence for seconds after the longest one, so
            // the player sees the end and loops it, or pauses a one-shot.
            if now.wrapping_sub(b.poll)<60 {return;}
            b.poll=now;
            if player().poll()==Event::Finished && b.once {b.want=false;}
        }
        (false,Boss::Starting|Boss::Playing)=>{
            spu::set_cd_volume(CdVolume::SILENCE,CdVolume::SILENCE);spu::enable_cd_audio(false);
            player().stop();
            if !cdrom::try_pause_until_complete(SPINS){unsafe{HK_MUSIC_ERROR=4;}}
            b.phase=Boss::Off;unsafe{HK_MUSIC_BOSS=0;}
            crate::disc::cdda_release();
        }
        (false,Boss::Off)=>{}
    }
}
/// Room loads stop XA first: a fight that ends in a death reloads.
pub fn release_cdda(){
    unsafe{BOSS.want=false;BOSS.fade_len=0;}
    if unsafe{BOSS.phase}!=Boss::Off {boss_service(psx_rt::interrupts::vblank_count());}
}
