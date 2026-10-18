//! The original game's opening cinematics, played when a new game starts.
//!
//! The game's image has almost no RAM to spare, so the movie player cannot be
//! part of it. The game carries this program as a blob inside its boot art
//! chunk instead (host/build_guest.py `boot_art`, game/src/intro.rs): when the
//! player starts a new game it reads the blob back, copies it to the RAM the
//! room-module pool owns (nothing lives there at the title) and jumps to its
//! `_start`, leaving the new game's choices in a mailbox at the top of RAM. The
//! blob puts the machine in the state the BIOS leaves it in ([`runtime::quiesce`]),
//! streams the Prologue (`HKINTRO1.STR`) and the Intro (`HKINTRO2.STR`) through
//! the SDK's MDEC player with their XA audio, in the order the source's
//! `Opening_Sequence` plays them, until the last frame or until Start skips the
//! rest, and then has the SDK's chain loader read the disc's boot executable
//! (`PSX.EXE`, the game itself) back over the RAM the player used. The game
//! boots from there as it does from the BIOS, finds the mailbox, and goes
//! straight into the new game without showing its title again.
//!
//! The movies are the ones the original plays, cooked by host/hk-intro-cook
//! from the player's own install. A disc built without them boots a game with
//! no blob.
#![no_std]
#![no_main]

extern crate psx_rt;

use psx_chainloader::paint;
use psx_chainloader::runtime::{self, Presentation};
use psx_fmv::player::{self, Config, Depth, Movie, Stop};
use psx_gpu::display::{DisplayConfig, Resolution, VideoMode};
use psx_gpu::Gpu;
use psx_io::periph::ControllerPort;
use psx_pad::{button, Port};
use psx_rt::{interrupts, tty, Peripherals};
use psx_spu::{self as spu, Spu, Volume};

mod cooked {
    include!("../../.hkpsx/intro/movie.rs");
}

/// One cooked movie: the root file it lives in and the figures the cook
/// measured for it.
struct Cooked {
    file: &'static str,
    frames: u32,
    width: u16,
    height: u16,
    max_chunks: u16,
    rle_words: usize,
    xa: Option<(u8, u8)>,
    latency_vblanks: u32,
}

/// In the order the source plays them.
const MOVIES: [Cooked; cooked::MOVIES] = [
    Cooked {
        file: "HKINTRO1.STR",
        frames: cooked::prologue::FRAMES,
        width: cooked::prologue::WIDTH,
        height: cooked::prologue::HEIGHT,
        max_chunks: cooked::prologue::MAX_CHUNKS,
        rle_words: cooked::prologue::RLE_WORDS,
        xa: cooked::prologue::XA,
        latency_vblanks: cooked::prologue::LATENCY_VBLANKS,
    },
    Cooked {
        file: "HKINTRO2.STR",
        frames: cooked::intro::FRAMES,
        width: cooked::intro::WIDTH,
        height: cooked::intro::HEIGHT,
        max_chunks: cooked::intro::MAX_CHUNKS,
        rle_words: cooked::intro::RLE_WORDS,
        xa: cooked::intro::XA,
        latency_vblanks: cooked::intro::LATENCY_VBLANKS,
    },
];
/// The disc's boot executable, which is the game.
const GAME_FILE: &str = "PSX.EXE";

/// Where the player's buffers sit: low RAM above the kernel, which the game's
/// image overwrites once the movies are over. They end below this program.
const PLAYER_MEMORY: usize = 0x8004_0000;
/// No sector for two seconds ends the movie.
const STALL_VBLANKS: u32 = 120;

extern "C" {
    /// First byte of this program's image (psoxide.ld).
    static __text_start: u8;
}

fn image_start() -> u32 {
    (&raw const __text_start) as u32
}

/// Start, newly pressed since the poll before. The pad starts out counted as
/// held, so a Start still down from the menu that launched the disc does not
/// skip a movie that has not begun.
struct Skip {
    port: ControllerPort,
    held: bool,
    last_vblank: u32,
}

impl Skip {
    fn poll(&mut self) -> bool {
        // The pad answers once per VBlank at most; the player calls this far
        // more often.
        let now = interrupts::vblank_count();
        if now == self.last_vblank {
            return false;
        }
        self.last_vblank = now;
        let held = psx_pad::poll_on(&mut self.port, Port::One)
            .buttons
            .is_held(button::START);
        let pressed = held && !self.held;
        self.held = held;
        pressed
    }
}

/// Plays one movie and says how it ended.
fn play_movie(skip: &mut Skip, cooked: &Cooked, limit: u32) -> Option<Stop> {
    let Some((lba, _)) = player::find_root_file(cooked.file) else {
        tty::println("hk-intro: a movie is missing from this disc");
        return None;
    };
    let movie = Movie {
        lba,
        frames: cooked.frames,
        width: cooked.width,
        height: cooked.height,
        max_chunks: cooked.max_chunks,
        rle_words: cooked.rle_words,
        xa: cooked.xa,
    };
    let config = Config {
        // 15-bit: the picture can be drawn over, and the player leaves the
        // display as it found it.
        depth: Depth::Rgb15,
        buffers: player::DEFAULT_BUFFERS,
        screen_width: 320,
        screen_height: 240,
        // The cook sized the sound's delay for this headroom (hk-intro-cook).
        latency_vblanks: cooked.latency_vblanks,
        stall_vblanks: STALL_VBLANKS,
    };
    let words = player::memory_words(&movie, config.depth);
    if PLAYER_MEMORY + words * 4 > limit as usize {
        tty::println("hk-intro: a movie's buffers would reach the intro's own image");
        return None;
    }
    // SAFETY: low RAM between the kernel's area and this image: nothing lives
    // there until the game is loaded over it, after the player has returned.
    let memory = unsafe { core::slice::from_raw_parts_mut(PLAYER_MEMORY as *mut u32, words) };
    let outcome = player::play(&movie, &config, memory, &mut || skip.poll());
    report(cooked.file, &outcome);
    Some(outcome.stop)
}

fn report(file: &str, o: &player::Outcome) {
    let stop = match o.stop {
        Stop::End => "end",
        Stop::Skipped => "skipped",
        Stop::Stall => "stall",
        Stop::CdError => "cd-error",
        Stop::Wedged => "wedged",
        Stop::Setup => "setup",
    };
    tty::print("INTRO ");
    tty::print(file);
    tty::print(" ");
    tty::print(stop);
    for (label, v) in [
        ("shown", o.shown),
        ("skipped", o.skipped),
        ("late", o.late),
        ("worst_late", o.worst_late_vblanks),
        ("dropped", o.dropped),
        ("errors", o.decode_errors),
        ("last_frame", o.last_frame),
        ("vblanks", o.vblanks),
        ("kcyc_vlc", o.kcyc_vlc),
        ("kcyc_mdec", o.kcyc_mdec),
        ("kcyc_wait", o.kcyc_wait),
    ] {
        tty::print(" ");
        tty::print(label);
        tty::print("=");
        print_decimal(v);
    }
    tty::println("");
}

fn print_decimal(mut v: u32) {
    let mut digits = [0u8; 10];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    tty::print(core::str::from_utf8(&digits[at..]).unwrap_or("?"));
}

/// A dim track and a bar over it, at the foot of a black screen, while the
/// game's image streams in.
struct Loading;

const BAR_X: i16 = 80;
const BAR_Y: i16 = 222;
const BAR_W: i16 = 160;
const BAR_H: i16 = 4;
const TRACK: u32 = 0x0030_3030;
const FILL: u32 = 0x00B0_B0B0;

impl Presentation for Loading {
    fn setup(&mut self) {
        paint::setup(319, 239);
    }
    fn begin(&mut self) {
        paint::rect(0, 0, 320, 240, 0);
        paint::rect(BAR_X, BAR_Y, BAR_W, BAR_H, TRACK);
        paint::draw_sync();
        paint::show();
    }
    fn update(&mut self, done: u32, total: u32) {
        let width = (done * BAR_W as u32 / total.max(1)).min(BAR_W as u32) as i16;
        paint::rect(BAR_X, BAR_Y, width, BAR_H, FILL);
        paint::draw_sync();
    }
    fn finish(&mut self) {}
    fn diagnostic_mode(&mut self) {
        paint::diagnostic_mode();
    }
    fn no_scratchpad(&mut self) {}
}

#[no_mangle]
fn main() {
    // Entered from the running game, not from the BIOS: put the machine where
    // the BIOS leaves it (SPU silent, interrupts masked, DMA off, GPU reset).
    // SAFETY: nothing else is running; the game that jumped here is gone.
    unsafe { runtime::quiesce() };
    interrupts::install_vblank_counter();
    let peripherals = Peripherals::take().expect("the intro takes the peripherals once, at boot");
    // The player and the chain loader take the drive, the MDEC and the GPU's
    // channel over themselves, as their docs say; the display and the SPU are
    // set up here.
    let _gpu = Gpu::new(
        peripherals.gpu_dma,
        DisplayConfig::new(VideoMode::Ntsc, Resolution::R320X240),
    );
    let _spu = Spu::new(peripherals.spu_dma);
    spu::set_main_volume(Volume::MAX, Volume::MAX);

    let mut skip = Skip {
        port: peripherals.controller_port,
        held: true,
        last_vblank: interrupts::vblank_count(),
    };
    for movie in &MOVIES {
        // Start skips the whole sequence, as holding a button does in the
        // original, not just the movie that is playing.
        if play_movie(&mut skip, movie, image_start()) != Some(Stop::End) {
            break;
        }
    }

    let Some((lba, _)) = player::find_root_file(GAME_FILE) else {
        tty::println("hk-intro: no PSX.EXE on this disc");
        loop {
            interrupts::wait_vblank();
        }
    };
    // The game is on the same disc as this program, so it takes over the
    // bases the launcher gave this one. Passing zero here made a game read
    // its packs at unshifted sectors whenever the disc is a multi-program one.
    let lba_offset = psx_io::disc_base::lba_offset();
    let cdda_track_base = u32::from(psx_io::disc_base::cdda_track_base());
    // SAFETY: the game's payload ends below this image (the game is linked
    // below the pool this blob sits in), the player has returned, and nothing
    // else uses the drive. `run` never returns. No checksum: the disc builder
    // cannot know the boot executable's, and only the read is trusted.
    unsafe { runtime::run(lba, lba_offset, cdda_track_base, 0, image_start(), Loading) }
}
