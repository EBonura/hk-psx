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

use psx_io::cd::xa::{DriveSpeed, File, Player};
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

/// Plays the poem and its piano over the two first display buffers. `cd` is the drive's token.
pub fn play(skip: &mut Skip, cd: Cd) -> End {
    let [(x0, y0), (x1, y1), _] = player::DEFAULT_BUFFERS;
    let buffers = [(x0, y0), (x1, y1)];
    let mut piano = player::find_root_file("HKPOEM.XA").map(|(lba, bytes)| {
        let file = File::new(lba, bytes / 2048, cooked::poem::XA_FILE, DriveSpeed::Single);
        let mut p = Player::new(cd);
        p.set_volume(0x80, 0x80);
        (p, file.song(cooked::poem::XA_CHANNEL), false)
    });
    psx_spu::set_cd_volume(CdVolume::MAX, CdVolume::MAX);
    psx_spu::enable_cd_audio(true);

    let start = interrupts::vblank_count();
    let mut last = start;
    let mut shown: Option<usize> = None;
    let mut back = 0;
    let (mut frames, mut slowest, mut seen) = (0u32, 0u32, start);
    let end = loop {
        interrupts::wait_vblank();
        let now = interrupts::vblank_count();
        if now == last {
            continue;
        }
        last = now;
        frames += 1;
        slowest = slowest.max(now.wrapping_sub(seen));
        seen = now;
        // The frame drawn last time goes up at the blank edge, before anything else is done.
        if let Some(finished) = shown {
            let (x, y) = buffers[finished];
            write_display_control(0x0500_0000 | u32::from(x) | u32::from(y) << 10);
        }
        let ms = millis(now.wrapping_sub(start));
        if ms >= prompt_text::POEM_END_MS {
            break End::Finished;
        }
        if skip.poll_without_bars() {
            break End::Skipped;
        }
        if let Some((player, song, started)) = piano.as_mut() {
            if !*started && ms >= PIANO_START_MS {
                *started = true;
                // A drive that refuses is a poem without its piano, not a stuck console.
                let _ = player.play(*song, false);
            }
            player.poll();
        }
        let (x, y) = buffers[back];
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
        shown = Some(back);
        back ^= 1;
    };
    tty::print("INTRO poem frames=");
    crate::print_decimal(frames);
    tty::print(" slowest_vblanks=");
    crate::print_decimal(slowest);
    tty::println("");
    if let Some((mut player, _, _)) = piano {
        player.stop();
        let _ = player.release();
    }
    end
}
