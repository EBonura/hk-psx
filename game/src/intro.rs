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
//! Only built with the `intro` feature. The mailbox carries a magic and a
//! check word, so power-on garbage or a stale session cannot be mistaken for a
//! handoff, and it is cleared when read.
use crate::menu::Settings;

/// The last 32 bytes of RAM, above the stack the game and the blob both start from, as eight words:
/// the magic and its complement, the profile and the settings as the game keeps them (five words:
/// a field the title gains is carried without this file knowing it). The magic is written last and
/// cleared first, so power-on garbage or a stale session is no handoff and the settings are only
/// read when they are ours.
const MAILBOX: *mut u32 = 0x801F_FFE0 as *mut u32;
const MAGIC: u32 = 0x484B_4E47; // "HKNG"
const SETTINGS_WORDS: usize = 5;
const _: () = assert!(core::mem::size_of::<Settings>() == SETTINGS_WORDS * 4);
const _: () = assert!(core::mem::align_of::<Settings>() == 4);

/// A new game that came back from the movies: the choices it left. Reads the
/// mailbox once and clears it, so a later boot cannot find a stale handoff.
#[inline(never)]
pub fn take() -> Option<(Settings, usize)> {
    // SAFETY: the mailbox is eight words of RAM nothing else uses. The settings are read only when
    // the magic words say `launch` wrote them from a valid `Settings`, which is five words long.
    unsafe {
        let ours = core::ptr::read_volatile(MAILBOX) == MAGIC
            && core::ptr::read_volatile(MAILBOX.add(1)) == !MAGIC;
        core::ptr::write_volatile(MAILBOX, 0);
        if !ours {
            return None;
        }
        let mut words = [0u32; SETTINGS_WORDS];
        for (i, w) in words.iter_mut().enumerate() {
            *w = core::ptr::read_volatile(MAILBOX.add(3 + i));
        }
        let profile = core::ptr::read_volatile(MAILBOX.add(2)) as usize;
        Some((core::mem::transmute::<[u32; SETTINGS_WORDS], Settings>(words), profile))
    }
}

/// Leave for the movies. Comes back only when this build carries no player, and the new game
/// then starts as it always did. `boot` is the boot art chunk the title was drawn from.
///
/// The blob sits in the chunk as host/build_guest.py `boot_art` wrote it: five words (the address
/// it links at inside the room-module pool, its length in bytes, and the registers it starts
/// with: pc, gp, sp), then its image. The writer checked that it lies inside the pool, which is
/// empty at the title, and that its stack starts under the mailbox.
#[inline(never)]
pub fn launch(boot: &[u8], settings: &Settings, profile: usize) {
    let (at, len) = crate::boot_art::INTRO_ART;
    if len == 0 {
        return;
    }
    // SAFETY: `at` is where the record starts inside `boot` (the chunk was read whole, and its
    // checksum was verified), five words and then the image; the words are read whole, which
    // the check on the address allows.
    let record = unsafe { boot.as_ptr().add(at) };
    if record as usize % 4 != 0 {
        return;
    }
    let word = |i: usize| unsafe { *(record as *const u32).add(i) };
    let (load, bytes, pc, gp, sp) = (word(0), word(1), word(2), word(3), word(4));
    // SAFETY: as `take`; the magic goes last.
    unsafe {
        let words = core::mem::transmute::<Settings, [u32; SETTINGS_WORDS]>(*settings);
        for (i, w) in words.into_iter().enumerate() {
            core::ptr::write_volatile(MAILBOX.add(3 + i), w);
        }
        core::ptr::write_volatile(MAILBOX.add(2), profile as u32);
        core::ptr::write_volatile(MAILBOX.add(1), !MAGIC);
        core::ptr::write_volatile(MAILBOX, MAGIC);
    }
    // SAFETY: the blob's link range, checked by the writer to lie inside the empty pool.
    unsafe { core::ptr::copy_nonoverlapping(record.add(20), load as *mut u8, bytes as usize) };
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
            gp = in(reg) gp,
            sp = in(reg) sp,
            pc = in(reg) pc,
            options(noreturn),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_travel_as_five_words_and_back() {
        let mut s = Settings::new();
        s.sfx = 3;
        s.brightness = -4;
        s.cheats.charms = true;
        s.style = 4;
        s.unlocked = 0b101;
        // SAFETY: the same transmutes `launch` and `take` make.
        let back = unsafe {
            core::mem::transmute::<[u32; SETTINGS_WORDS], Settings>(
                core::mem::transmute::<Settings, [u32; SETTINGS_WORDS]>(s),
            )
        };
        assert_eq!(back, s);
    }
}
