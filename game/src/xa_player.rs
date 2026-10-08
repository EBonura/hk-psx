//! XA-ADPCM song player: one channel of an interleaved XA file (the file
//! host/hk-cook/src/xa_music.rs writes) through the drive's own decoder to the
//! SPU's CD input.
//!
//! This is the logic of the SDK's `psx_io::cd::xa::Player` (PSoXide ae6e1ef10,
//! sdk/docs/XA-MUSIC.md), over the drive commands of the SDK this game is
//! pinned to. Delete it for that import when the pin moves past the player:
//! the pin cannot move until the build tools that read the SDK's linker script
//! and hazard patcher are ported (xa-encoder.lock.json says why).
//!
//! The drive is busy while a song plays: it streams the file continuously, so
//! nothing else may read the disc, which is what `disc` arranges for the title
//! and the fights. A song loops by watching the head: at the end of its own
//! audio it seeks back to the top, a gap as long as the seek.
use psx_io::{cdrom,disc_base};

const CMD_SETFILTER:u8=0x0D;
const CMD_READS:u8=0x1B;
/// Setmode: XA-ADPCM on, and only the file and channel Setfilter names (single speed).
const MODE_XA_FILTERED:u8=0x40|0x08;
/// Per-command response spin budget: generous, the drive answers far sooner.
const SPINS:u32=131_072;
/// Spin budget for a pause to finish (up to about a tenth of a second).
const PAUSE_SPINS:u32=2_000_000;
/// Sectors the drive plays per second at single speed.
const SECTORS_PER_SECOND:u32=75;

/// One channel of the XA file, from `lba` (relative to this program's disc
/// image) to `span` sectors on, where its audio ends.
#[derive(Clone,Copy)]
pub struct Song {pub lba:u32,pub span:u32,pub file:u8,pub channel:u8}

#[derive(Clone,Copy,PartialEq,Eq)]
pub enum Event {Idle,Playing,Looped,Finished}

#[derive(Clone,Copy,PartialEq,Eq)]
enum State {Idle,
    /// Started; the head has not been seen inside the song yet. The drive
    /// reports its old position until the seek lands, so an end test now would
    /// fire on the previous run's position.
    Seeking,
    Streaming}

pub struct Player {state:State,song:Option<Song>,looping:bool,pause_pending:bool,start:u32,head:u32}

impl Player {
    pub const fn new()->Self {Self {state:State::Idle,song:None,looping:false,pause_pending:false,start:0,head:0}}
    /// Start `song` from the top (or restart it); with `looping` it restarts by
    /// itself when `poll` sees it end. Blocks for the drive commands, a few
    /// milliseconds on a warm drive. Gives the drive mixer unity.
    #[inline(never)]
    pub fn play(&mut self,song:Song,looping:bool)->bool {
        let ok=cdrom::try_demute(SPINS).is_some()
            && cdrom::try_set_mode(MODE_XA_FILTERED,SPINS).is_some()
            && cdrom::try_command(CMD_SETFILTER,&[song.file,song.channel],SPINS).is_some()
            && self.begin_read(song);
        if ok {
            self.song=Some(song);self.looping=looping;
            cdrom::set_audio_mixer(0x80,0,0x80,0);
        } else {
            self.state=State::Idle;self.song=None;
        }
        ok
    }
    /// Seek to the top of the song and start streaming.
    #[inline(never)]
    fn begin_read(&mut self,song:Song)->bool {
        let start=disc_base::shift_lba(song.lba);
        if cdrom::try_set_loc_lba(start,SPINS).is_none()||cdrom::try_command(CMD_READS,&[],SPINS).is_none() {return false;}
        self.start=start;self.head=start;self.pause_pending=false;self.state=State::Seeking;true
    }
    /// Pause a running drive and forget the song. The drive refuses a pause
    /// during the seek that starts a read, so a refused one is retried by `poll`.
    #[inline(never)]
    pub fn stop(&mut self) {
        if self.state!=State::Idle {self.pause_pending=!cdrom::try_pause_until_complete(PAUSE_SPINS);}
        self.state=State::Idle;self.song=None;
    }
    /// Call about once a second at the least (a frame is better for a short
    /// guard): notices the end of the song and loops it or pauses.
    #[inline(never)]
    pub fn poll(&mut self)->Event {
        let Some(song)=self.song else {
            if self.pause_pending {self.pause_pending=!cdrom::try_pause_until_complete(PAUSE_SPINS);}
            return Event::Idle;
        };
        let end=self.start+song.span;
        if let Some(head)=head_lba() {
            if self.state==State::Seeking&&(self.start..end).contains(&head) {self.state=State::Streaming;}
            if self.state==State::Streaming {self.head=head;}
            if self.state==State::Streaming&&head>=end {return self.reach_end(song);}
        }
        Event::Playing
    }
    #[inline(never)]
    fn reach_end(&mut self,song:Song)->Event {
        if self.looping&&self.begin_read(song) {return Event::Looped;}
        self.stop();Event::Finished
    }
    /// The head has been seen inside the song since the last start: audio is flowing.
    pub fn is_streaming(&self)->bool {self.state==State::Streaming}
    /// Time since the start of the song at the last poll (the head runs a
    /// sector or two ahead of the sound).
    pub fn elapsed_millis(&self)->u32 {self.head.saturating_sub(self.start)*1000/SECTORS_PER_SECOND}
}

/// Absolute LBA the head is reading, if the drive answers (it counts from the
/// start of the lead-in, two seconds before LBA 0).
fn head_lba()->Option<u32> {
    let p=cdrom::PlayPosition::parse(&cdrom::try_get_loc_p(SPINS)?)?;
    ((p.absolute_min as u32*60+p.absolute_sec as u32)*75+p.absolute_frame as u32).checked_sub(150)
}
