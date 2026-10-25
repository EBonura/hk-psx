//! The poem the original shows before its movies, with the piano under it.
//!
//! `Opening_Sequence` starts with `Prologue Text`, a 30 s animation of five lines from Monomon's elegy
//! fading in one after another over a black screen, the `Music` source (the piano, 15.6 s) switched on
//! 3.53 s into it, and a black cover that opens at the start and closes again over everything from
//! 23.6 s to 28.1 s; the sequence goes on to the movies at 28.6 s. host/hk-build/intro.rs lays the lines
//! out in the game's font (wrapped to the picture, centred, a row of glyphs a cell of the font in VRAM)
//! and samples the animation's curves into [`prompt_text::POEM_ALPHA`]; this module draws a frame from
//! them every VBlank into one of two display buffers and flips at the next blank edge.
//!
//! The piano is a sound-only XA file of its own (`HKPOEM.XA`, host/hk-intro-cook): the SDK's XA player
//! starts it a second into the sequence, and the file opens with silence so that its first note lands at
//! 3.53 s whatever the drive's start takes within a few tenths of a second.

use psx_io::cd::xa::{DriveSpeed, Event, File, Player};
use psx_io::gpu::{try_wait_command_ready, write_command, write_display_control};
use psx_io::periph::Cd;
use psx_rt::{interrupts, tty};
use psx_spu::CdVolume;
use psx_vram::{Clut, TextureDepth, TexturePage};

use crate::{cooked, prompt_text, Skip, FONT_CLUT, FULL};
use psx_fmv::player;

/// When the piano's file starts, in milliseconds into the sequence.
const PIANO_START_MS: u32 = 1_000;
/// Spins to wait for the GPU to take a packet, or to finish a frame, before going on (a stuck GPU must not
/// hang the console).
const SPINS: u32 = 1_000_000;

/// GPUSTAT bits 26 and 28: ready for a command, ready for a DMA block (bit 27, VRAM to CPU, is only set
/// while a read-back is pending, so it is no part of idle).
const GPU_IDLE: u32 = 0x1400_0000;

/// How the poem ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum End {
    /// The animation ran out.
    Finished,
    /// The player skipped.
    Skipped,
}

/// Milliseconds of a 60 Hz display's `vblanks` (NTSC runs at 59.94).
fn millis(vblanks: u32) -> u32 {
    vblanks * 1001 / 60
}

/// A table entry of `curve` at `ms`, 0 to 255, interpolated between the table's steps.
fn alpha(curve: usize, ms: u32) -> u32 {
    let table = &prompt_text::POEM_ALPHA[curve];
    let step = ms / prompt_text::POEM_ALPHA_STEP_MS;
    let (a, b) = (
        u32::from(table[(step as usize).min(table.len() - 1)]),
        u32::from(table[(step as usize + 1).min(table.len() - 1)]),
    );
    let part = ms % prompt_text::POEM_ALPHA_STEP_MS;
    // Whole 8-bit values, rounded: no step is visible in a 2 s fade.
    (a * (prompt_text::POEM_ALPHA_STEP_MS - part) + b * part) / prompt_text::POEM_ALPHA_STEP_MS
}

/// Every line's brightness at `ms`, 0 to [`FULL`], under the black cover.
fn brightness(line: usize, ms: u32) -> u32 {
    let line_alpha = alpha(line, ms);
    let open = 255 - alpha(5, ms);
    FULL * line_alpha * open / (255 * 255)
}

/// One packet to the GPU: one wait for the port, then its words back to back (a wait per word is the
/// wrong pacing on silicon, see the chain loader's paint module).
fn packet(words: &[u32]) {
    let _ = try_wait_command_ready(SPINS);
    for &word in words {
        write_command(word);
    }
}

/// Aim drawing at the buffer at (`x`, `y`): its area and offset, no texture window.
fn aim(x: u16, y: u16) {
    packet(&[
        0xE200_0000,
        0xE300_0000 | u32::from(x) | u32::from(y) << 10,
        0xE400_0000 | u32::from(x + 319) | u32::from(y + 239) << 10,
        0xE500_0000 | u32::from(x) | u32::from(y) << 11,
    ]);
}

/// Black over the whole buffer (GP0 fill: whole 16-pixel columns, outside the drawing area).
fn clear(x: u16, y: u16) {
    packet(&[0x0200_0000, u32::from(y) << 16 | u32::from(x), 240 << 16 | 320]);
}

/// One frame of the poem at `ms`, into the buffer at (`x`, `y`).
fn draw_frame(x: u16, y: u16, ms: u32) {
    clear(x, y);
    aim(x, y);
    let page = TexturePage::new(prompt_text::FONT_X, prompt_text::FONT_PAGE_Y, TextureDepth::Bit4).uv_word(1);
    packet(&[0xE100_0000 | u32::from(page) | 1 << 10]);
    let clut = u32::from(Clut::new(FONT_CLUT.0, FONT_CLUT.1).uv_word());
    let mut first = 0;
    for (line, &end) in prompt_text::POEM_LINE_END.iter().enumerate() {
        let level = brightness(line, ms);
        if level != 0 {
            let color = 0x6600_0000 | level * 0x0001_0101;
            for &(u, v, gx, gy) in &prompt_text::POEM_GLYPHS[first..end] {
                packet(&[
                    color,
                    // The drawing offset is the buffer's, set by `aim`: positions are the screen's.
                    u32::from(gx as u16) | u32::from(gy as u16) << 16,
                    u32::from(u) | u32::from(v) << 8 | clut << 16,
                    u32::from(prompt_text::GLYPH_W) | u32::from(prompt_text::GLYPH_H) << 16,
                ]);
            }
        }
        first = end;
    }
}

/// How long the piano has to ring out after the drive reached the end of its file before the poem
/// hands the drive and the CPU to the game: the sound is decoded a few sectors behind the head.
const PIANO_RING_OUT_VBLANKS: u32 = 30;

/// What the poem is between VBlanks. In the program's own loop ([`play`]) it lives on the stack; once the
/// piano is over the game takes the machine back to load the first scene while the poem goes on, and
/// the state rides in the hand-off block ([`Carry`]) between the game's calls of [`tick`].
struct Run {
    /// The poem's time, in milliseconds, at VBlank `origin` of whichever counter is in use.
    base_ms: u32,
    origin: u32,
    last: u32,
    /// The display buffer the last frame went into and is waiting to be shown.
    shown: Option<usize>,
    back: usize,
    frames: u32,
    slowest: u32,
    seen: u32,
}

impl Run {
    fn new(now: u32) -> Run {
        Run { base_ms: 0, origin: now, last: now, shown: None, back: 0, frames: 0, slowest: 0, seen: now }
    }

    /// The poem's time at VBlank `now`.
    fn ms(&self, now: u32) -> u32 {
        self.base_ms + millis(now.wrapping_sub(self.origin))
    }

    /// One VBlank of the poem: show the frame drawn last time, end the poem if it is over or skipped, let
    /// `between` act (the piano), then draw the next frame. `None` while it goes on.
    fn frame(&mut self, skip: &mut Skip, now: u32, mut between: impl FnMut(u32)) -> Option<End> {
        let buffers = buffers();
        self.last = now;
        self.frames += 1;
        self.slowest = self.slowest.max(now.wrapping_sub(self.seen));
        self.seen = now;
        // The frame drawn last time goes up at the blank edge, before anything else is done.
        if let Some(finished) = self.shown.take() {
            let (x, y) = buffers[finished];
            write_display_control(0x0500_0000 | u32::from(x) | u32::from(y) << 10);
            self.shown = Some(finished);
        }
        let ms = self.ms(now);
        if ms >= prompt_text::POEM_END_MS {
            return Some(End::Finished);
        }
        if skip.poll_without_bars_at(now) {
            return Some(End::Skipped);
        }
        between(ms);
        // The game's loader may have a transfer to VRAM in flight (its atlases go up while the poem runs);
        // the GPU's port is not ours until it is done, and the frame waits for the next VBlank.
        if !psx_vram::dma_copy_to_vram_done() {
            return None;
        }
        let (x, y) = buffers[self.back];
        draw_frame(x, y, ms);
        let prompt = skip.prompt_brightness(now);
        if prompt != 0 {
            crate::draw_bar(x, y, prompt);
        }
        // The frame is done when the GPU says so; the flip waits for the next edge.
        let mut spins = SPINS;
        while psx_io::gpu::status().bits() & GPU_IDLE != GPU_IDLE && spins != 0 {
            spins -= 1;
        }
        self.shown = Some(self.back);
        self.back ^= 1;
        None
    }

    fn report(&self) {
        tty::print("INTRO poem frames=");
        crate::print_decimal(self.frames);
        tty::print(" slowest_vblanks=");
        crate::print_decimal(self.slowest);
        tty::println("");
    }
}

/// The two display buffers the poem alternates between: the first two of the movies' three.
fn buffers() -> [(u16, u16); 2] {
    let [a, b, _] = crate::movie_buffers();
    [a, b]
}

/// How the first part of the poem ended.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum First {
    /// The poem ended or was skipped.
    Ended(End),
    /// The piano is over and the poem is not: the game goes on loading while [`tick`] draws the rest.
    Yield,
}

/// Plays the poem and its piano over the first two display buffers until the piano is over (or the poem
/// is, or the player skips). `cd` is the drive's token. A `Yield` leaves the poem's state in `carry`.
pub fn play(skip: &mut Skip, cd: Cd, carry: &mut Carry) -> First {
    let mut piano = player::find_root_file("HKPOEM.XA").map(|(lba, bytes)| {
        let file = File::new(lba, bytes / 2048, cooked::poem::XA_FILE, DriveSpeed::Single);
        let mut p = Player::new(cd);
        p.set_volume(0x80, 0x80);
        (p, file.song(cooked::poem::XA_CHANNEL), false)
    });
    psx_spu::set_cd_volume(CdVolume::MAX, CdVolume::MAX);
    psx_spu::enable_cd_audio(true);

    let start = interrupts::vblank_count();
    let mut run = Run::new(start);
    // VBlanks since the drive reached the end of the piano's file (None while it plays).
    let mut ringing: Option<u32> = piano.is_none().then_some(PIANO_RING_OUT_VBLANKS);
    let end = loop {
        interrupts::wait_vblank();
        let now = interrupts::vblank_count();
        if now == run.last {
            continue;
        }
        let over = run.frame(skip, now, |ms| {
            if let Some((player, song, started)) = piano.as_mut() {
                if !*started && ms >= PIANO_START_MS {
                    *started = true;
                    // A drive that refuses is a poem without its piano, not a stuck console.
                    let _ = player.play(*song, false);
                }
                if player.poll() == Event::Finished || (*started && !player.is_playing()) {
                    ringing.get_or_insert(0);
                }
            }
        });
        if let Some(end) = over {
            break First::Ended(end);
        }
        if let Some(n) = ringing.as_mut() {
            *n += 1;
            if *n > PIANO_RING_OUT_VBLANKS {
                break First::Yield;
            }
        }
    };
    run.report();
    if let Some((mut player, _, _)) = piano {
        player.stop();
        let _ = player.release();
    }
    if end == First::Yield {
        *carry = Carry::from_run(&run, skip, interrupts::vblank_count());
    }
    end
}

/// The poem's state between the game's calls of [`tick`], in the hand-off block (the program's own memory is
/// not kept between its calls): every field a word, none of them a counter of this program's VBlank handler,
/// which stops when the program returns, so times are kept as ages.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Carry {
    /// The poem's time at the last yield, in milliseconds.
    base_ms: u32,
    /// The game's VBlank count at the first tick (0 before it).
    origin: u32,
    last: u32,
    /// 0 or 1: the buffer to show at the next edge; `NONE` for none.
    shown: u32,
    back: u32,
    frames: u32,
    slowest: u32,
    held: u32,
    /// The VBlank the skip prompt began at, or `NONE`; until the first tick, how many VBlanks before the yield.
    prompt_at: u32,
    /// The brightness the prompt's bars hold, or `NONE`.
    prompt_drawn: u32,
    /// Set by the first tick.
    started: u32,
    /// `VALID` while the block holds a poem state (it holds whatever was in RAM at power-on otherwise).
    valid: u32,
}

const NONE: u32 = u32::MAX;
const VALID: u32 = 0x504F_454D;

impl Carry {
    fn from_run(run: &Run, skip: &Skip, now: u32) -> Carry {
        Carry {
            base_ms: run.ms(now),
            origin: 0,
            last: 0,
            shown: run.shown.map_or(NONE, |b| b as u32),
            back: run.back as u32,
            frames: run.frames,
            slowest: run.slowest,
            held: u32::from(skip.held),
            prompt_at: skip.prompt.since.map_or(NONE, |at| now.wrapping_sub(at)),
            prompt_drawn: skip.prompt.drawn.unwrap_or(NONE),
            started: 0,
            valid: VALID,
        }
    }
}

/// What the poem drew while the game loaded the first scene behind it: frames and the longest gap, in VBlanks,
/// between two steps (the loader's checkpoints are not that regular). Printed when the program is called again.
pub fn report_carried(carry: &mut Carry) {
    if carry.valid != VALID || carry.started == 0 {
        return;
    }
    carry.valid = 0;
    tty::print("INTRO poem during the load frames=");
    crate::print_decimal(carry.frames);
    tty::print(" slowest_vblanks=");
    crate::print_decimal(carry.slowest);
    tty::println("");
}

/// One step of the poem, called by the game from inside its loader (game/src/intro.rs) with the game's own
/// VBlank count: does nothing until a new VBlank has come, then shows the frame drawn last time, polls the
/// pad and draws the next frame. Returns 0 while the poem goes on, 1 when it ended and 2 when the player
/// skipped. The state lives in `carry`; the pad's and the GPU's registers are all it touches.
pub fn tick(carry: &mut Carry, now: u32) -> u32 {
    if carry.started == 0 {
        carry.started = 1;
        carry.origin = now;
        carry.last = now;
        if carry.prompt_at != NONE {
            // It was an age; from now on it is a VBlank of the game's counter.
            carry.prompt_at = now.wrapping_sub(carry.prompt_at);
        }
    }
    if now == carry.last {
        return 0;
    }
    // SAFETY: the program owns the pad port while the poem runs; the game's input sampler is idle until
    // the first scene is up.
    let mut skip = Skip {
        port: unsafe { psx_io::periph::ControllerPort::steal() },
        held: carry.held as u16,
        last_vblank: carry.last,
        prompt: crate::Prompt {
            since: (carry.prompt_at != NONE).then_some(carry.prompt_at),
            drawn: (carry.prompt_drawn != NONE).then_some(carry.prompt_drawn),
        },
        bars: false,
    };
    let mut run = Run {
        base_ms: carry.base_ms,
        origin: carry.origin,
        last: carry.last,
        shown: (carry.shown != NONE).then_some(carry.shown as usize),
        back: carry.back as usize,
        frames: carry.frames,
        slowest: carry.slowest,
        seen: carry.last,
    };
    let over = run.frame(&mut skip, now, |_| {});
    carry.last = now;
    carry.shown = run.shown.map_or(NONE, |b| b as u32);
    carry.back = run.back as u32;
    carry.frames = run.frames;
    carry.slowest = run.slowest;
    carry.held = u32::from(skip.held);
    carry.prompt_at = skip.prompt.since.unwrap_or(NONE);
    carry.prompt_drawn = skip.prompt.drawn.unwrap_or(NONE);
    match over {
        None => 0,
        Some(End::Finished) => 1,
        Some(End::Skipped) => {
            // The game finishes its load before it puts the screen back: black meanwhile, not the frame the
            // player skipped over.
            if psx_vram::dma_copy_to_vram_done() {
                let (x, y) = buffers()[run.back];
                clear(x, y);
                write_display_control(0x0500_0000 | u32::from(x) | u32::from(y) << 10);
            }
            2
        }
    }
}
