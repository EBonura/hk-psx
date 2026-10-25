//! The opening cinematics, on a new game (intro/src/main.rs, host/hk-intro-cook).
//!
//! The player is a separate program, carried as a blob in the boot art chunk
//! (`boot_art::INTRO_ART`), because the game has no RAM for an MDEC player.
//! Starting a new game on a disc that has the movies calls [`play`]: it copies
//! the blob's payload to its link address (the room-module pool, which is
//! empty at the title), writes a mailbox in the last bytes of RAM and *calls*
//! the blob on a stack of its own. The blob puts the hardware state it
//! changes back before it returns, so the game goes on from where it was:
//! its image, its data and the main loop's stack never left RAM, and there
//! is no reload of the executable and no second boot. The only RAM the
//! blob uses of the game's is the scene arena, which nothing needs between
//! the title and the first scene's load.
//!
//! The mailbox is ten words above the stack: a magic and its complement,
//! the stack pointer to return with, the address of [`hk_intro_leave`], the
//! arena's address and length, the disc's base sector and CD-DA track
//! base (the SDK's file lookup needs them, as a chain loader passes them)
//! and the screen position the title chose.
//! The magic goes last, so power-on garbage is no hand-off.
//!
//! Only built with the `intro` feature.

/// The last 64 bytes of RAM, above the stack the game runs on (its initial stack pointer is 0x801FFF00).
const MAILBOX: *mut u32 = 0x801F_FFC0 as *mut u32;
const MAGIC: u32 = 0x484B_4E47; // "HKNG"

extern "C" {
    /// Saves the callee-saved registers, switches to the blob's stack and gp and jumps to it; comes back
    /// when the blob jumps to `hk_intro_leave` with this call's stack pointer.
    fn hk_intro_enter(pc: u32, gp: u32, sp: u32, mailbox: *mut u32);
}

// Both halves are written out so that the blob's own registers and stack never meet the game's: the
// game's `s0`-`s7`, `fp`, `gp` and `ra` wait on its own stack, and `leave` is entered with `sp` set to it.
#[cfg(target_arch = "mips")]
core::arch::global_asm!(
    r#"
    .set noreorder
    .set noat
    .section .text.hk_intro
    .globl hk_intro_enter
    .globl hk_intro_leave
hk_intro_enter:
    addiu $29, $29, -48
    sw $16, 0($29)
    sw $17, 4($29)
    sw $18, 8($29)
    sw $19, 12($29)
    sw $20, 16($29)
    sw $21, 20($29)
    sw $22, 24($29)
    sw $23, 28($29)
    sw $30, 32($29)
    sw $28, 36($29)
    sw $31, 40($29)
    sw $29, 8($7)
    lui $2, %hi(hk_intro_leave)
    addiu $2, $2, %lo(hk_intro_leave)
    sw $2, 12($7)
    move $25, $4
    move $28, $5
    move $29, $6
    li $4, {magic}
    lw $5, 24($7)
    lw $6, 28($7)
    jr $25
    nop
hk_intro_leave:
    lw $16, 0($29)
    lw $17, 4($29)
    lw $18, 8($29)
    lw $19, 12($29)
    lw $20, 16($29)
    lw $21, 20($29)
    lw $22, 24($29)
    lw $23, 28($29)
    lw $30, 32($29)
    lw $28, 36($29)
    lw $31, 40($29)
    addiu $29, $29, 48
    jr $31
    nop
"#,
    magic = const psx_io::disc_base::HANDOFF_MAGIC,
);

/// Play the opening movies. Returns when they have played out or the player skipped them (the game then
/// goes straight into the new game), and at once when this build carries no player. `boot` is the boot art
/// chunk the title was drawn from; the player's image rides in it, and its movie buffers take the scene
/// arena over, which `boot` is staged in: do not use `boot` after this.
///
/// When they have played, the GPU and display are as the boot sets them up (the program does that before it
/// returns): reset, at the title's screen position (`screen`), both frame buffers black, and no button of the
/// skip still down. `fb` starts over.
///
/// The blob sits in the chunk as host/build_guest.py `boot_art` wrote it: five words (the address it
/// links at inside the room-module pool, its length in bytes, and the registers it starts with: pc, gp,
/// sp), then its image. The writer checked that it lies inside the pool, which is empty at the title,
/// and that its stack, which is the pool's top, leaves it room.
#[inline(never)]
#[optimize(size)]
pub fn play(boot: &[u8], fb: &mut psx_gpu::framebuf::FrameBuffer, screen: (i8, i8)) {
    let screen = (i32::from(screen.0) as u32, i32::from(screen.1) as u32);
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
    let (arena, arena_bytes) = crate::disc::arena();
    // SAFETY: the mailbox is eight words of RAM nothing else uses; the magic goes last.
    unsafe {
        core::ptr::write_volatile(MAILBOX.add(4), arena as u32);
        core::ptr::write_volatile(MAILBOX.add(5), arena_bytes as u32);
        core::ptr::write_volatile(MAILBOX.add(6), psx_io::disc_base::lba_offset());
        core::ptr::write_volatile(MAILBOX.add(7), u32::from(psx_io::disc_base::cdda_track_base()));
        core::ptr::write_volatile(MAILBOX.add(8), screen.0);
        core::ptr::write_volatile(MAILBOX.add(9), screen.1);
        core::ptr::write_volatile(MAILBOX.add(1), !MAGIC);
        core::ptr::write_volatile(MAILBOX, MAGIC);
    }
    // SAFETY: the blob's link range, checked by the writer to lie inside the empty pool (no overlap with
    // `boot`, which is in the arena).
    unsafe { core::ptr::copy_nonoverlapping(record.add(20), load as *mut u8, bytes as usize) };
    psx_rt::cache::flush_instruction_cache();
    // SAFETY: the blob is a program built for this call (intro/src/main.rs): it keeps to its own stack,
    // returns through `hk_intro_leave` with the stack pointer it was given, and restores the interrupt
    // and DMA state it changes. Our callee-saved registers are saved by the stub.
    unsafe { hk_intro_enter(pc, gp, sp, MAILBOX) };
    // The mailbox is done with: a later session must not find a stale hand-off.
    // SAFETY: as above.
    unsafe { core::ptr::write_volatile(MAILBOX, 0) };
    // The blob left the GPU reset, the display where the title put it and the frame buffers black.
    *fb = psx_gpu::framebuf::FrameBuffer::new(320, 240);
}
