//! The game's hardware state, taken before the program uses the machine and put back before it returns.
//!
//! The program is a call from the running game (game/src/intro.rs), not a replacement of it: the game's
//! code, data, stack and everything it keeps in RAM stay where they are, so what has to come back is what
//! the program changes outside RAM. [`Saved::capture`] reads that state once; [`Saved::restore`] writes it
//! back and re-enables interrupts last, after the game's exception vector is in place again.
//!
//! What the program changes and this puts back: the exception vector (the program installs its own
//! VBlank handler there; the game's, which wraps the CD interrupt, must be in front again), the interrupt
//! mask and COP0 status, the DMA priority register, the root counters' modes, and the SPU's control
//! register and volumes (the program silences it and turns the CD input on for the sound of the movies).
//! What it leaves alone on purpose: the SPU's voices and RAM (the game's sound effects stay loaded), the
//! GPU (the game starts its next frame from a reset, see game/src/intro.rs) and the VRAM it never touched.

use psx_io::periph::Cd;
use psx_io::{irq, read_u16, read_u32, write_u16, write_u32};

const VECTOR: u32 = 0x8000_0080;
const DPCR: u32 = 0x1F80_10F0;
/// Mode registers of the three root counters.
const TIMER_MODES: [u32; 3] = [0x1F80_1104, 0x1F80_1114, 0x1F80_1124];
/// The SPU registers the program touches and the game's own driver reads back: the main volume (left,
/// right), the CD input volume (left, right), the external input volume and SPUCNT.
const SPU_REGISTERS: [u32; 7] = [
    0x1F80_1D80,
    0x1F80_1D82,
    0x1F80_1DB0,
    0x1F80_1DB2,
    0x1F80_1DB4,
    0x1F80_1DB6,
    0x1F80_1DAA,
];

/// Everything [`Saved::restore`] writes back.
pub struct Saved {
    status: u32,
    mask: u32,
    dpcr: u32,
    vector: [u32; 2],
    timers: [u32; 3],
    spu: [u16; 7],
}

/// COP0 status (SR), the register that holds the interrupt enable.
fn read_status() -> u32 {
    let sr: u32;
    // SAFETY: reads COP0 SR into $8; the nop covers MFC0's load delay (see runtime::quiesce).
    unsafe { core::arch::asm!("mfc0 $8, $12", "nop", out("$8") sr, options(nostack, preserves_flags)) };
    sr
}

fn write_status(sr: u32) {
    // SAFETY: writes SR from $8, the value read by `read_status` at capture.
    unsafe { core::arch::asm!("mtc0 $8, $12", "nop", in("$8") sr, options(nostack, preserves_flags)) };
}

impl Saved {
    /// The state as the game has it now.
    ///
    /// # Safety
    ///
    /// Reads hardware registers: call with the game stopped in its call to the program, nothing else
    /// driving the SPU or the DMA controller.
    pub unsafe fn capture() -> Saved {
        // SAFETY: plain aligned register reads, as the contract says.
        unsafe {
            Saved {
                status: read_status(),
                mask: irq::mask(),
                dpcr: read_u32(DPCR),
                vector: [read_u32(VECTOR), read_u32(VECTOR + 4)],
                timers: TIMER_MODES.map(|a| read_u32(a)),
                spu: SPU_REGISTERS.map(|a| read_u16(a)),
            }
        }
    }

    /// Put the game's state back. `cd` is the controller's token, to drop whatever the drive still
    /// reports so the game's interrupt handler, which sees the controller next, finds it quiet.
    ///
    /// # Safety
    ///
    /// The program must be finished with every device: no DMA running, the drive paused.
    pub unsafe fn restore(&self, cd: &mut Cd) {
        // Nothing may interrupt while the vector changes hands.
        write_status(self.status & !1);
        irq::set_mask(0);
        irq::acknowledge(u32::MAX);
        cd.discard_response();
        cd.acknowledge_irq(0x1F);
        // SAFETY: register writes of values read at capture, with interrupts off.
        unsafe {
            write_u32(DPCR, self.dpcr);
            for (address, mode) in TIMER_MODES.iter().zip(self.timers) {
                write_u32(*address, mode);
            }
            // The program's own sound is over: no voice of it keys on (it uses none), the SPU is
            // as the game left it.
            write_u16(0x1F80_1D8C, 0xFFFF);
            write_u16(0x1F80_1D8E, 0x00FF);
            for (address, value) in SPU_REGISTERS.iter().zip(self.spu) {
                write_u16(*address, value);
            }
            write_u32(VECTOR, self.vector[0]);
            write_u32(VECTOR + 4, self.vector[1]);
        }
        psx_rt::cache::flush_instruction_cache();
        // The vector is the game's again and the controller is quiet (acknowledged above).
        irq::acknowledge(u32::MAX);
        irq::set_mask(self.mask);
        write_status(self.status);
    }
}
