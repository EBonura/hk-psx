//! IRQ-driven sector transport using the pinned SDK's proven PIO recipe.
//!
//! The SDK SectorReader has no nonblocking read API and its runtime has no CD
//! callback hook. This local exception wrapper preserves the interrupted CPU
//! context, drains at most one 2048-byte sector, then chains to the SDK's exact
//! VBlank/display-flip/fault handler. No SDK or sibling source is modified.
//! Only an inactive, exclusively owned room arena may be a transfer target.
use core::cell::UnsafeCell;
use psx_io::{cdrom, irq, timers};
const CD: u32 = cdrom::BASE;
const CD_BIT: u32 = 1 << irq::source::CDROM;
const PARAM_POLLS: usize = 32;
const FIFO_POLLS: usize = 256;
const TIMEOUT_VBLANKS: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum Phase {
    Idle,
    Setloc,
    SeekAck,
    SeekComplete,
    Setmode,
    ReadAck,
    Reading,
    PauseAck,
    PauseComplete,
    Done,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Busy,
    Done,
    Failed(u32),
}
struct Transfer {
    phase: Phase,
    destination: *mut u32,
    count: usize,
    received: usize,
    cancelled: bool,
    error: u32,
    stamp: u32,
    location: [u8; 3],
    recovering: bool,
}
struct Shared(UnsafeCell<Transfer>);
unsafe impl Sync for Shared {}
static TRANSFER: Shared = Shared(UnsafeCell::new(Transfer {
    phase: Phase::Idle,
    destination: core::ptr::null_mut(),
    count: 0,
    received: 0,
    cancelled: false,
    error: 0,
    stamp: 0,
    location: [0; 3],
    recovering: false,
}));
#[repr(C, align(16))]
struct Stack([u8; 2048]);
#[no_mangle]
static mut HK_CD_IRQ_STACK: Stack = Stack([0; 2048]);
#[no_mangle]
static mut HK_CD_IRQ_CONTEXT: [u32; 34] = [0; 34];
#[no_mangle]
pub static mut HK_CD_IRQ_MAX_TIMER2_TICKS: u32 = 0;
#[no_mangle]
pub static mut HK_CD_IRQ_COUNT: u32 = 0;
#[no_mangle]
pub static mut HK_CD_IRQ_SECTORS: u32 = 0;
#[no_mangle]
pub static mut HK_CD_DISCARDED_SECTORS: u32 = 0;
#[no_mangle]
pub static mut HK_CD_STREAM_PHASE: u32 = 0;
#[no_mangle]
pub static mut HK_CD_STREAM_ERROR: u32 = 0;

/// gate-tour builds: the last 128 controller events, for cancel/ordering
/// questions (kind<<24 | phase<<16 | flag or command<<8 | received&0xff).
#[cfg(feature = "gate-tour")]
#[no_mangle]
static mut HK_CD_TRACE: [u32; 128] = [0; 128];
#[cfg(feature = "gate-tour")]
#[no_mangle]
static mut HK_CD_TRACE_N: u32 = 0;
#[inline(always)]
fn trace(_kind: u32, _t: &Transfer, _value: u8) {
    #[cfg(feature = "gate-tour")]
    unsafe {
        let n = HK_CD_TRACE_N;
        HK_CD_TRACE_N = n.wrapping_add(1);
        (*(&raw mut HK_CD_TRACE))[(n & 127) as usize] = _kind << 24
            | (_t.phase as u32) << 16
            | (_value as u32) << 8
            | (_t.received as u32 & 0xff);
    }
}
fn exclusive<T>(f: impl FnOnce(&mut Transfer) -> T) -> T {
    let mask = irq::mask();
    irq::set_mask(mask & !CD_BIT);
    // An empty asm is a compiler memory barrier. compiler_fence currently
    // lowers to unsupported MIPS-I SYNC in this pinned LLVM toolchain.
    unsafe {
        core::arch::asm!("", options(nostack, preserves_flags));
    }
    let value = f(unsafe { &mut *TRANSFER.0.get() });
    unsafe {
        core::arch::asm!("", options(nostack, preserves_flags));
    }
    irq::set_mask(mask);
    value
}
unsafe fn index(value: u8) {
    unsafe {
        psx_io::write8(CD, value);
    }
}
unsafe fn output(value: u8) {
    unsafe {
        index(1);
        psx_io::write8(CD + 2, value);
        index(0);
    }
}
fn publish(t: &Transfer) {
    unsafe {
        core::ptr::write_volatile(&raw mut HK_CD_STREAM_PHASE, t.phase as u32);
        core::ptr::write_volatile(&raw mut HK_CD_STREAM_ERROR, t.error);
    }
}
fn change(t: &mut Transfer, phase: Phase) {
    t.phase = phase;
    t.stamp = psx_rt::interrupts::vblank_count();
    publish(t);
}
fn fail(t: &mut Transfer, error: u32) {
    trace(4, t, (error >> 24) as u8);
    trace(5, t, (error >> 8) as u8);
    t.error = error;
    change(t, Phase::Failed);
    unsafe {
        output(0);
    }
    irq::ack(CD_BIT);
}
/// Bound command dispatch, never wait for its ACK or mechanical completion.
fn command(t: &mut Transfer, command: u8, params: &[u8], phase: Phase) {
    trace(6, t, command);
    unsafe {
        output(0);
        index(1);
        psx_io::write8(CD + 3, 0x5f);
        index(0);
    }
    irq::ack(CD_BIT);
    cdrom::discard_response();
    unsafe {
        index(1);
        psx_io::write8(CD + 3, 0x40);
        index(0);
    }
    for &value in params {
        let mut ready = false;
        for _ in 0..PARAM_POLLS {
            if unsafe { psx_io::read8(CD) } & 0x10 != 0 {
                ready = true;
                break;
            }
        }
        if !ready {
            fail(t, 0xfe00_0000 | (command as u32) << 8);
            return;
        }
        unsafe {
            psx_io::write8(CD + 2, value);
        }
    }
    change(t, phase);
    unsafe {
        psx_io::write8(CD + 1, command);
        output(0x1f);
    }
}
fn pause(t: &mut Transfer) {
    command(t, cdrom::CMD_PAUSE, &[], Phase::PauseAck);
}

/// Called only with CPU CD interrupts masked and after the previous Pause.
/// Safety: destination owns count*2048 writable bytes until terminal status.
pub unsafe fn start(destination: *mut u32, count: usize, lba: u32) -> Result<(), u32> {
    exclusive(|t| {
        if !matches!(t.phase, Phase::Idle | Phase::Done | Phase::Failed) {
            return Err(0xfc00_0000);
        }
        if destination.is_null() || destination as usize & 3 != 0 || count == 0 {
            return Err(0xfd00_0000);
        }
        trace(3, t, 0);
        let recover = matches!(t.phase, Phase::Failed);
        let absolute = psx_io::disc_base::shift_lba(lba).saturating_add(150);
        let params = [
            cdrom::bin_to_bcd((absolute / (60 * 75)) as u8),
            cdrom::bin_to_bcd(((absolute / 75) % 60) as u8),
            cdrom::bin_to_bcd((absolute % 75) as u8),
        ];
        *t = Transfer {
            phase: Phase::Idle,
            destination,
            count,
            received: 0,
            cancelled: false,
            error: 0,
            stamp: 0,
            location: params,
            recovering: recover,
        };
        if recover {
            // Retain ownership through an asynchronous recovery Pause. Only its
            // completion may start the new seek or permit arena reuse.
            pause(t);
        } else {
            unsafe {
                index(0);
                psx_io::write8(CD + 3, 0);
            }
            command(t, cdrom::CMD_SETLOC, &params, Phase::Setloc);
        }
        if t.phase == Phase::Failed {
            Err(t.error)
        } else {
            Ok(())
        }
    })
}
/// Sectors the current (or last) transfer has stored so far. A scene group
/// read at a gate decodes each chunk as soon as its sectors have landed.
pub fn received() -> usize {
    exclusive(|t| t.received)
}
/// Stop the transfer early: the interrupt handler pauses the drive at the
/// next sector (or as soon as ReadN is acknowledged), the same way a finished
/// transfer pauses. Issuing Pause from here instead, while sectors keep
/// arriving, lost the Pause acknowledge (traced: no interrupt for 600 VBlanks,
/// then a stray completion failed the next read), so it never does.
pub fn cancel() {
    exclusive(|t| {
        trace(2, t, 0);
        t.cancelled = true;
    });
}
pub fn status() -> Status {
    exclusive(|t| {
        if !matches!(t.phase, Phase::Idle | Phase::Done | Phase::Failed)
            && psx_rt::interrupts::vblank_count().wrapping_sub(t.stamp) > TIMEOUT_VBLANKS
        {
            fail(t, 0xff00_0000 | t.phase as u32);
        }
        match t.phase {
            Phase::Idle => Status::Idle,
            Phase::Done => Status::Done,
            Phase::Failed => Status::Failed(t.error),
            _ => Status::Busy,
        }
    })
}

/// Drain one sector only. No long DATA_POLL loop and no CD DMA start-bit risk.
fn sector(t: &mut Transfer) -> bool {
    unsafe {
        index(0);
        psx_io::write8(CD + 3, 0x80);
    }
    let mut ready = false;
    for _ in 0..FIFO_POLLS {
        if unsafe { psx_io::read8(CD) } & 0x40 != 0 {
            ready = true;
            break;
        }
    }
    if !ready {
        fail(t, 0xfa00_0000 | t.phase as u32);
        return false;
    }
    let store =
        matches!(t.phase, Phase::Reading | Phase::ReadAck) && !t.cancelled && t.received < t.count;
    for word in 0..512 {
        let value = unsafe {
            (psx_io::read8(CD + 2) as u32)
                | ((psx_io::read8(CD + 2) as u32) << 8)
                | ((psx_io::read8(CD + 2) as u32) << 16)
                | ((psx_io::read8(CD + 2) as u32) << 24)
        };
        if store {
            unsafe {
                t.destination
                    .add(t.received * 512 + word)
                    .write_volatile(value);
            }
        }
    }
    if store {
        t.received += 1;
        unsafe {
            HK_CD_IRQ_SECTORS = HK_CD_IRQ_SECTORS.wrapping_add(1);
            super::HK_CD_SECTORS_READ = super::HK_CD_SECTORS_READ.wrapping_add(1);
        }
    } else {
        unsafe {
            HK_CD_DISCARDED_SECTORS = HK_CD_DISCARDED_SECTORS.wrapping_add(1);
        }
    }
    t.stamp = psx_rt::interrupts::vblank_count();
    true
}
struct IrqDuration(u16);
impl Drop for IrqDuration {
    fn drop(&mut self) {
        let ticks = timers::counter(timers::Timer::Timer2).wrapping_sub(self.0) as u32;
        unsafe {
            HK_CD_IRQ_MAX_TIMER2_TICKS = HK_CD_IRQ_MAX_TIMER2_TICKS.max(ticks);
        }
    }
}
#[no_mangle]
extern "C" fn hk_cd_interrupt() {
    let _duration = IrqDuration(timers::counter(timers::Timer::Timer2));
    // Exception entry disables CPU interrupts; no foreground Transfer reference
    // can coexist because exclusive() masks the CD source before borrowing it.
    let t = unsafe { &mut *TRANSFER.0.get() };
    unsafe {
        HK_CD_IRQ_COUNT = HK_CD_IRQ_COUNT.wrapping_add(1);
    }
    let flag = cdrom::irq_flag_value();
    trace(1, t, flag);
    if flag == 1 {
        if !sector(t) {
            cdrom::discard_response();
            cdrom::acknowledge_irq(0x1f);
            return;
        }
        cdrom::discard_response();
        cdrom::acknowledge_irq(flag);
        if matches!(t.phase, Phase::Reading | Phase::ReadAck)
            && (t.cancelled || t.received == t.count)
        {
            pause(t);
        }
        return;
    }
    if flag == 5 {
        let a = unsafe { psx_io::read8(CD + 1) };
        let b = unsafe { psx_io::read8(CD + 1) };
        cdrom::discard_response();
        cdrom::acknowledge_irq(0x1f);
        fail(
            t,
            0x0500_0000 | ((a as u32) << 16) | ((t.phase as u32) << 8) | b as u32,
        );
        return;
    }
    cdrom::discard_response();
    cdrom::acknowledge_irq(flag);
    match (t.phase, flag) {
        (Phase::Setloc, 3) => command(t, cdrom::CMD_SEEKL, &[], Phase::SeekAck),
        (Phase::SeekAck, 3) => change(t, Phase::SeekComplete),
        (Phase::SeekComplete, 2) => command(
            t,
            cdrom::CMD_SETMODE,
            &[cdrom::MODE_DOUBLE_SPEED],
            Phase::Setmode,
        ),
        (Phase::Setmode, 3) => command(t, cdrom::CMD_READN, &[], Phase::ReadAck),
        (Phase::ReadAck, 3) => {
            change(t, Phase::Reading);
            if t.cancelled {
                pause(t);
            }
        }
        (Phase::PauseAck, 3) => change(t, Phase::PauseComplete),
        (Phase::PauseComplete, 2) => {
            if t.recovering && !t.cancelled {
                t.recovering = false;
                let params = t.location;
                unsafe {
                    index(0);
                    psx_io::write8(CD + 3, 0);
                }
                command(t, cdrom::CMD_SETLOC, &params, Phase::Setloc);
            } else {
                change(t, Phase::Done);
                unsafe {
                    output(0);
                }
            }
        }
        (_, 0) => {}
        _ => fail(t, 0xf900_0000 | ((t.phase as u32) << 8) | flag as u32),
    }
}

/// Hand the controller to polled SDK commands (XA): mask its interrupt at
/// the CPU so this handler cannot take their responses. Only while idle.
pub fn suspend() {
    irq::set_mask(irq::mask() & !CD_BIT);
}
/// Take the controller back after polled commands, dropping whatever they left.
pub fn resume() {
    cdrom::discard_response();
    cdrom::acknowledge_irq(0x1f);
    irq::ack(CD_BIT);
    irq::set_mask(irq::mask() | CD_BIT);
}

extern "C" {
    fn hk_cd_exception_wrapper();
}
/// Install after the SDK's title-time polled header read has completed.
pub fn install() {
    let mask = irq::mask();
    irq::set_mask(mask & !CD_BIT);
    // Timer2 is exclusively reserved for IRQ duration diagnostics. System / 8,
    // free-running, no timer IRQ; ticks wrap after about 15.48 ms.
    timers::set_mode(timers::Timer::Timer2, 0x0200);
    unsafe {
        let stack = (&raw mut HK_CD_IRQ_STACK.0).cast::<u8>();
        for i in 0..2048 {
            stack.add(i).write_volatile(0xa5);
        }
        let address = hk_cd_exception_wrapper as *const () as usize as u32;
        (0x8000_0080 as *mut u32).write_volatile(0x0800_0000 | ((address >> 2) & 0x03ff_ffff));
        (0x8000_0084 as *mut u32).write_volatile(0);
        psx_rt::cache::flush_i_cache();
    }
    irq::ack(CD_BIT);
    irq::set_mask(mask | CD_BIT);
}

core::arch::global_asm!(
    r#"
    .set noreorder
    .set noat
    .section .text.hk_cd_irq
    .globl hk_cd_exception_wrapper
hk_cd_exception_wrapper:
    mfc0 $26, $13
    nop
    andi $26, $26, 0x007c
    bnez $26, 9f
    nop
    lui $26, 0x1f80
    lw $27, 0x1070($26)
    lw $26, 0x1074($26)
    nop
    and $27, $27, $26
    andi $27, $27, 4
    beqz $27, 9f
    nop
    lui $26, %hi(HK_CD_IRQ_CONTEXT)
    addiu $26, $26, %lo(HK_CD_IRQ_CONTEXT)
    sw $1, 0($26)
    sw $2, 4($26)
    sw $3, 8($26)
    sw $4, 12($26)
    sw $5, 16($26)
    sw $6, 20($26)
    sw $7, 24($26)
    sw $8, 28($26)
    sw $9, 32($26)
    sw $10, 36($26)
    sw $11, 40($26)
    sw $12, 44($26)
    sw $13, 48($26)
    sw $14, 52($26)
    sw $15, 56($26)
    sw $16, 60($26)
    sw $17, 64($26)
    sw $18, 68($26)
    sw $19, 72($26)
    sw $20, 76($26)
    sw $21, 80($26)
    sw $22, 84($26)
    sw $23, 88($26)
    sw $24, 92($26)
    sw $25, 96($26)
    sw $28, 100($26)
    sw $29, 104($26)
    sw $30, 108($26)
    sw $31, 112($26)
    mfhi $27
    sw $27, 116($26)
    mflo $27
    sw $27, 120($26)
    lui $29, %hi(HK_CD_IRQ_STACK+2048)
    addiu $29, $29, %lo(HK_CD_IRQ_STACK+2048)
    addiu $29, $29, -16
    jal hk_cd_interrupt
    nop
    lui $26, %hi(HK_CD_IRQ_CONTEXT)
    addiu $26, $26, %lo(HK_CD_IRQ_CONTEXT)
    lw $27, 116($26)
    nop
    mthi $27
    lw $27, 120($26)
    nop
    mtlo $27
    lw $1, 0($26)
    lw $2, 4($26)
    lw $3, 8($26)
    lw $4, 12($26)
    lw $5, 16($26)
    lw $6, 20($26)
    lw $7, 24($26)
    lw $8, 28($26)
    lw $9, 32($26)
    lw $10, 36($26)
    lw $11, 40($26)
    lw $12, 44($26)
    lw $13, 48($26)
    lw $14, 52($26)
    lw $15, 56($26)
    lw $16, 60($26)
    lw $17, 64($26)
    lw $18, 68($26)
    lw $19, 72($26)
    lw $20, 76($26)
    lw $21, 80($26)
    lw $22, 84($26)
    lw $23, 88($26)
    lw $24, 92($26)
    lw $25, 96($26)
    lw $28, 100($26)
    lw $29, 104($26)
    lw $30, 108($26)
    lw $31, 112($26)
    nop
9:
    j __psx_rt_exception_handler
    nop
    .set at
    .set reorder
"#
);
