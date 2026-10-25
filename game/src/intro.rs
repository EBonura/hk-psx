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
//! The hand-off block ([`Hand`]) is the last 192 bytes of RAM, above the stack: the mailbox proper is ten
//! words (a magic and its complement, the stack pointer to return with, the address of [`hk_intro_leave`],
//! the arena's address and length, the disc's base sector and CD-DA track base, which the SDK's file lookup
//! needs as a chain loader passes them, and the screen position the title chose), then the mode the game asks
//! for and what the program answers. The magic goes last, so power-on garbage is no hand-off.
//!
//! The poem and the first scene's load overlap. The program is called three times (the mailbox's mode word):
//! to play the poem with its piano, which holds the drive until the piano is over; then, with the drive
//! free again, the game loads what it can of the first scene while the poem goes on (`disc::Cache::preload`:
//! the sound banks, the scene's ambience clips and sound effects, the coverage, the effect art and every
//! atlas, none of it into the scene arena for good), and the loader's cooperative checkpoints (disc.rs
//! `checkpoint`) call [`tick`], which calls the program's step of the poem (`hk_poem_tick_entry`, through
//! the same enter stub) with the game's own VBlank count; then the program plays the movies, which take
//! the scene arena. After them only the scene itself, its world metadata and its code are left to load.
//!
//! Only built with the `intro` feature.

/// The hand-off block in the last 192 bytes of RAM, above the stack the game runs on (its initial stack pointer
/// is 0x801FFF00; the 256 bytes over it are nobody's). The program (intro/src/main.rs) reads and writes the
/// same words: the first 64 bytes are its own state for the poem (poem.rs `Carry`), then come the loader's
/// words for the poem's steps, then the mailbox, where the program has always looked for it.
#[repr(C)]
struct Hand {
    poem_state: [u32; 16],
    /// The program's stack pointer, the VBlank the last step of the poem ran at, and how the poem stands:
    /// `RUNNING` while the loader draws it, then 1 once it ended or 2 when the player skipped. Anything else (the
    /// block holds whatever was in RAM at power-on) means no poem is drawn.
    step_sp: u32,
    step_last: u32,
    step_state: u32,
    spare: [u32; 13],
    /// The mailbox: a magic and its complement, the stack pointer to return with, the address of
    /// [`hk_intro_leave`], the arena's address and length, the disc's base sector and CD-DA track base (the
    /// SDK's file lookup needs them, as a chain loader passes them), the screen position the title chose, what
    /// the game asks (a mode, which a step takes as the VBlank count), what the program answers, and where its
    /// step of the poem is (address and global pointer).
    magic: u32,
    magic_not: u32,
    saved_sp: u32,
    leave_pc: u32,
    arena: u32,
    arena_bytes: u32,
    lba_offset: u32,
    cdda_base: u32,
    screen_x: u32,
    screen_y: u32,
    mode: u32,
    result: u32,
    step_pc: u32,
    step_gp: u32,
    spare2: [u32; 2],
}
const HAND: *mut Hand = 0x801F_FF40 as *mut Hand;
const MAGIC: u32 = 0x484B_4E47; // "HKNG"
// The program's block for the poem sits at the start of `Hand`; the mailbox is 16 words after it, where the
// program has always looked for it.
const _: () = assert!(core::mem::offset_of!(Hand, magic) == 128 && core::mem::size_of::<Hand>() == 192);
/// The block's address as a value the compiler cannot fold into every access: one base register per function
/// and an offset per word, instead of a 32-bit address built for each (a few dozen bytes of the RAM below
/// the pool).
#[inline(always)]
fn hand_base() -> *mut Hand {
    core::hint::black_box(HAND)
}
macro_rules! hand {
    ($base:ident, $field:ident) => {
        &raw mut (*$base).$field
    };
}
// The display buffers the movies (and the poem) draw into; the program has the same file.
include!("../../intro/src/buffers.rs");
const MODE_POEM: u32 = 0;
const MODE_MOVIES: u32 = 1;
const MODE_SCREEN: u32 = 2;
const RESULT_SKIPPED: u32 = 1;
const RESULT_YIELD: u32 = 2;
const RUNNING: u32 = 0x504F_454D; // "POEM"

// The player's buffers are the scene arena: the cook measured what they need (host/hk-intro-cook), the guest
// build read it from the movie constants.
const _: () = assert!(crate::boot_art::INTRO_PLAYER_BYTES <= crate::disc::ARENA_BYTES);

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

/// Play the opening: the poem with its piano, then, while the poem goes on, `load` (the first scene's
/// load as far as the movies leave it alone), then the movies. Returns when they have played out or the
/// player skipped them (the game then goes straight into the new game), and at once when this build carries
/// no player. `boot` is the boot art chunk the title was drawn from; the player's image rides in it, and the
/// load and the movies' buffers take the scene arena over, which `boot` is staged in: do not use `boot`
/// after this.
///
/// When they have played, the GPU and display are as the boot sets them up (the program does that before it
/// returns): reset, at the title's screen position (`screen`), both frame buffers black, and no button of the
/// skip still down. `fb` starts over.
///
/// The blob sits in the chunk as host/build_guest.py `boot_art` wrote it: five words (the address it
/// links at inside the room-module pool, its length in bytes, and the registers it starts with: pc, gp,
/// sp), then its image. The writer checked that it lies inside the pool, which is empty at the title,
/// and that its stack, which is the pool's top, leaves it room. Nothing may be installed into the pool
/// until the movies are over.
#[inline(never)]
#[optimize(size)]
pub fn play(
    boot: &[u8],
    fb: &mut psx_gpu::framebuf::FrameBuffer,
    screen: (i8, i8),
    load: fn(),
) {
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
    let (load_at, bytes, pc, gp, sp) = (word(0), word(1), word(2), word(3), word(4));
    let (arena, arena_bytes) = crate::disc::arena();
    let h = hand_base();
    // SAFETY: the hand-off block is RAM nothing else uses; the magic goes last.
    unsafe {
        core::ptr::write_volatile(hand!(h, arena), arena as u32);
        core::ptr::write_volatile(hand!(h, arena_bytes), arena_bytes as u32);
        core::ptr::write_volatile(hand!(h, lba_offset), psx_io::disc_base::lba_offset());
        core::ptr::write_volatile(hand!(h, cdda_base), u32::from(psx_io::disc_base::cdda_track_base()));
        core::ptr::write_volatile(hand!(h, screen_x), screen.0);
        core::ptr::write_volatile(hand!(h, screen_y), screen.1);
        core::ptr::write_volatile(hand!(h, magic_not), !MAGIC);
        core::ptr::write_volatile(hand!(h, magic), MAGIC);
    }
    // SAFETY: the blob's link range, checked by the writer to lie inside the empty pool (no overlap with
    // `boot`, which is in the arena).
    unsafe { core::ptr::copy_nonoverlapping(record.add(20), load_at as *mut u8, bytes as usize) };
    psx_rt::cache::flush_instruction_cache();
    let call = |mode: u32| call_program(pc, gp, sp, mode);
    let mut result = call(MODE_POEM);
    if result == RESULT_YIELD {
        begin_ticks(sp);
        load();
        result = finish_ticks();
    }
    // A skip ends the opening; the screen is put back either way.
    call(if result == RESULT_SKIPPED { MODE_SCREEN } else { MODE_MOVIES });
    // The mailbox is done with: a later session must not find a stale hand-off.
    // SAFETY: as above.
    unsafe { core::ptr::write_volatile(hand!(h, magic), 0) };
    // The blob left the GPU reset, the display where the title put it and the frame buffers black.
    *fb = psx_gpu::framebuf::FrameBuffer::new(320, 240);
}

/// One call of the program in `mode`; what it reports.
#[inline(never)]
#[optimize(size)]
fn call_program(pc: u32, gp: u32, sp: u32, mode: u32) -> u32 {
    // SAFETY: the blob is a program built for this call (intro/src/main.rs): it keeps to its own stack,
    // returns through `hk_intro_leave` with the stack pointer it was given, and restores the interrupt and
    // DMA state it changes. Our callee-saved registers are saved by the stub.
    let h = hand_base();
    unsafe {
        core::ptr::write_volatile(hand!(h, mode), mode);
        core::ptr::write_volatile(hand!(h, result), 0);
        hk_intro_enter(pc, gp, sp, hand!(h, magic));
        core::ptr::read_volatile(hand!(h, result))
    }
}

/// The poem goes on from the loader: the program's step entry is in the mailbox, its stack in `sp`.
#[optimize(size)]
fn begin_ticks(sp: u32) {
    let h = hand_base();
    // SAFETY: the hand-off block.
    unsafe {
        core::ptr::write_volatile(hand!(h, step_sp), sp);
        core::ptr::write_volatile(hand!(h, step_last), psx_rt::interrupts::vblank_count());
        core::ptr::write_volatile(hand!(h, step_state), RUNNING);
    }
}

/// Draw the poem's next frame when a VBlank has come, from the loader's checkpoints (disc.rs `checkpoint`).
/// A call and not inline code at each of them: the checkpoints are the loader's, a few dozen, and a state
/// word's test is not worth a dozen bytes of the RAM below the pool at every one.
#[inline(never)]
#[optimize(size)]
pub fn tick() {
    let now = psx_rt::interrupts::vblank_count();
    let h = hand_base();
    // SAFETY: the hand-off block; the program's step takes the VBlank count in the mode word and leaves how
    // the poem stands in the result.
    unsafe {
        if core::ptr::read_volatile(hand!(h, step_state)) != RUNNING
            || now == core::ptr::read_volatile(hand!(h, step_last))
        {
            return;
        }
        core::ptr::write_volatile(hand!(h, step_last), now);
        let pc = core::ptr::read_volatile(hand!(h, step_pc));
        let gp = core::ptr::read_volatile(hand!(h, step_gp));
        let sp = core::ptr::read_volatile(hand!(h, step_sp));
        match call_program(pc, gp, sp, now) {
            0 => {}
            ended => core::ptr::write_volatile(hand!(h, step_state), ended),
        }
    }
}

/// The load is over: let the poem run out (or be skipped), a VBlank at a time. 0 if it ran out, 1 if the
/// player skipped (the values of the mailbox's result).
#[optimize(size)]
fn finish_ticks() -> u32 {
    let h = hand_base();
    // SAFETY: the hand-off block.
    unsafe {
        while core::ptr::read_volatile(hand!(h, step_state)) == RUNNING {
            psx_rt::interrupts::wait_vblank();
            tick();
        }
        let skipped = core::ptr::read_volatile(hand!(h, step_state)) == 2;
        core::ptr::write_volatile(hand!(h, step_state), 0);
        u32::from(skipped)
    }
}
