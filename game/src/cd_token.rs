//! The CD token for the raw drivers, which have never held one.

/// The CD drive, for one command or register step.
#[inline(always)]
pub(crate) fn cd() -> psx_io::periph::Cd {
    // SAFETY: a token is a logic guard, not a memory-safety one (see
    // `psx_io::periph`); the raw CD drivers drive the drive directly.
    unsafe { psx_io::periph::Cd::steal() }
}
