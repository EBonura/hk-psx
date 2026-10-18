//! The opening cinematics, on a new game (intro/src/main.rs, host/hk-intro-cook).
//!
//! The player is a separate program, carried as a blob in the boot art chunk
//! (`boot_art::INTRO_ART`), because the game has no RAM for an MDEC player.
//! Starting a new game on a disc that has the movies calls [`launch`]: it
//! writes the choices made on the title (the profile, the options, the cheats)
//! into a mailbox in the last bytes of RAM, copies the blob's payload to its
//! link address (the room-module pool, which is empty at the title) and jumps
//! to it. The blob plays the movies and loads the disc's boot executable, this
//! game, back; [`take`] then finds the mailbox and the game goes into the new
//! game without showing its title a second time.
//!
//! Only built with the `intro` feature. The mailbox is two words and an
//! inverted copy, so power-on garbage or a stale session cannot be mistaken for
//! a handoff, and it is cleared when read.
use crate::cheats;
use crate::menu::Settings;

/// Last 16 bytes of RAM: above the stack the game and the blob both start from.
const MAILBOX: *mut u32 = 0x801F_FFF0 as *mut u32;
const MAGIC: u32 = 0x484B_4E47; // "HKNG"

fn pack_cheats(c: &cheats::Settings) -> u32 {
    [
        c.invincible,
        c.max_nail,
        c.infinite_soul,
        c.max_masks,
        c.dash,
        c.claw,
        c.wings,
        c.heart,
        c.cloak,
        c.dream,
        c.spell,
        c.charms,
    ]
    .iter()
    .enumerate()
    .fold(0, |bits, (i, &on)| bits | (u32::from(on) << i))
}

fn unpack_cheats(bits: u32) -> cheats::Settings {
    let on = |i: u32| bits >> i & 1 != 0;
    cheats::Settings {
        invincible: on(0),
        max_nail: on(1),
        infinite_soul: on(2),
        max_masks: on(3),
        dash: on(4),
        claw: on(5),
        wings: on(6),
        heart: on(7),
        cloak: on(8),
        dream: on(9),
        spell: on(10),
        charms: on(11),
    }
}

/// The choices a new game carries across the movies, in two words.
fn encode(settings: &Settings, profile: usize) -> [u32; 2] {
    let audio = u32::from(settings.sfx)
        | u32::from(settings.ambience) << 4
        | u32::from(settings.music) << 8
        | (settings.brightness as u8 as u32) << 12
        | (settings.screen_x as u8 as u32) << 20;
    [
        audio | (profile as u32) << 28,
        pack_cheats(&settings.cheats) | (settings.screen_y as u8 as u32) << 16,
    ]
}

fn decode(words: [u32; 2]) -> (Settings, usize) {
    let mut s = Settings::new();
    s.sfx = (words[0] & 15) as u8;
    s.ambience = (words[0] >> 4 & 15) as u8;
    s.music = (words[0] >> 8 & 15) as u8;
    s.brightness = (words[0] >> 12 & 255) as u8 as i8;
    s.screen_x = (words[0] >> 20 & 255) as u8 as i8;
    s.screen_y = (words[1] >> 16 & 255) as u8 as i8;
    s.cheats = unpack_cheats(words[1] & 0xFFFF);
    (s, (words[0] >> 28) as usize)
}

/// The four mailbox words for these choices: the magic, the two payload
/// words and a check over all three.
fn mailbox_words(settings: &Settings, profile: usize) -> [u32; 4] {
    let [a, b] = encode(settings, profile);
    [MAGIC, a, b, !(MAGIC ^ a ^ b)]
}

fn parse_mailbox(words: [u32; 4]) -> Option<(Settings, usize)> {
    (words[0] == MAGIC && words[3] == !(MAGIC ^ words[1] ^ words[2]))
        .then(|| decode([words[1], words[2]]))
}

/// A new game that came back from the movies: the choices it left. Reads the
/// mailbox once and clears it, so a later boot cannot find a stale handoff.
pub fn take() -> Option<(Settings, usize)> {
    // SAFETY: the mailbox is four words of RAM nothing else uses.
    let words = [0, 1, 2, 3].map(|i| unsafe { core::ptr::read_volatile(MAILBOX.add(i)) });
    [0, 1, 2, 3].map(|i| unsafe { core::ptr::write_volatile(MAILBOX.add(i), 0) });
    parse_mailbox(words)
}

/// Leave for the movies. Comes back only when this build carries no player (or
/// the blob is not one this RAM can hold), and the new game then starts as it
/// always did. `boot` is the boot art chunk the title was drawn from.
pub fn launch(boot: &[u8], settings: &Settings, profile: usize) {
    let (at, len) = crate::boot_art::INTRO_ART;
    if len == 0 || boot.len() < at + len {
        return;
    }
    let exe = &boot[at..at + len];
    let word = |o: usize| u32::from_le_bytes([exe[o], exe[o + 1], exe[o + 2], exe[o + 3]]);
    if exe.len() < 0x800 || &exe[..8] != b"PS-X EXE" {
        return;
    }
    let (pc0, gp0, t_addr, t_size) = (word(0x10), word(0x14), word(0x18), word(0x1C));
    let sp = word(0x30).wrapping_add(word(0x34));
    // The pool is empty at the title; the blob has to sit inside it and its
    // stack has to start under the mailbox.
    if t_addr < 0x801D_B000
        || t_addr.saturating_add(t_size) > 0x801F_3F00
        || exe.len() < 0x800 + t_size as usize
        || sp > MAILBOX as u32
    {
        return;
    }
    for (i, w) in mailbox_words(settings, profile).into_iter().enumerate() {
        // SAFETY: as `take`.
        unsafe { core::ptr::write_volatile(MAILBOX.add(i), w) };
    }
    let payload = &exe[0x800..0x800 + t_size as usize];
    for (i, chunk) in payload.chunks_exact(4).enumerate() {
        let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        // SAFETY: the blob's link range, checked above to lie inside the empty pool.
        unsafe { core::ptr::write_volatile((t_addr as *mut u32).add(i), w) };
    }
    psx_rt::cache::flush_instruction_cache();
    let lba = psx_io::disc_base::lba_offset();
    let cdda = u32::from(psx_io::disc_base::cdda_track_base());
    // SAFETY: control never returns here. The blob's `_start` takes the
    // handover registers the way a chain loader gives them (psx_io::disc_base).
    unsafe {
        core::arch::asm!(
            "move $4, {magic}",
            "move $5, {lba}",
            "move $6, {cdda}",
            "move $28, {gp}",
            "move $29, {sp}",
            "jr {pc}",
            "nop",
            magic = in(reg) psx_io::disc_base::HANDOFF_MAGIC,
            lba = in(reg) lba,
            cdda = in(reg) cdda,
            gp = in(reg) gp0,
            sp = in(reg) sp,
            pc = in(reg) pc0,
            options(noreturn),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_survive_the_mailbox() {
        let mut s = Settings::new();
        s.sfx = 3;
        s.ambience = 7;
        s.music = 10;
        s.brightness = -4;
        s.screen_x = 5;
        s.screen_y = -2;
        s.cheats.invincible = true;
        s.cheats.charms = true;
        s.cheats.spell = true;
        for profile in 0..4 {
            assert_eq!(parse_mailbox(mailbox_words(&s, profile)), Some((s, profile)));
        }
    }

    #[test]
    fn garbage_and_a_cleared_mailbox_are_no_handoff() {
        assert_eq!(parse_mailbox([0; 4]), None);
        assert_eq!(parse_mailbox([MAGIC, 1, 2, 3]), None);
        let mut words = mailbox_words(&Settings::new(), 1);
        words[1] ^= 1;
        assert_eq!(parse_mailbox(words), None);
    }
}
