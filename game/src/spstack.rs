//! Who owns the CPU scratchpad when, and the stack simulation runs on.
//!
//! The R3000A has no data cache: a main-RAM load stalls for about six cycles
//! and a scratchpad load for none. Simulation code spills and reloads a lot
//! (world-bank views, terrain sweeps, iterator state), so running a stack-heavy
//! call on psx-rt's `ScratchpadStack` keeps the same instructions and moves
//! only where their frames live.
//!
//! Two kinds of call run on it, never at once and never nested in each other:
//!
//! * Simulation: calls made from `frame::simulate`, which main runs between
//!   frames.
//! * Scenery: both `render::scenery` passes and `render::prepare_back_prefix`.
//!   Their loop spills dozens of words per source draw; on the RAM stack those
//!   reloads were the largest single stall in CPU-bound frames. The renderer's
//!   own working storage (`render::RenderScratch`) is in main RAM, so nothing
//!   in the scratchpad outlives one call.
//!
//! No DMA channel reaches the scratchpad, and neither exception path writes
//! below the interrupted `$sp`: psx-rt's handler uses only `$k0`/`$k1`, and
//! `cd_stream`'s wrapper saves registers to `HK_CD_IRQ_CONTEXT` and runs on
//! `HK_CD_IRQ_STACK`. `host/hazards.py::stack_guard` proves after every link
//! that each call tree fits `SimStack::BUDGET`, has no call it cannot bound
//! and never flushes the I-cache (which unmaps the scratchpad).
use psx_rt::scratchpad::{assert_disjoint, ScratchpadStack};

/// The simulation phase's stack: all of the scratchpad.
pub type SimStack = ScratchpadStack<0, 1024>;

// Nothing else lives in the scratchpad while a call runs on SimStack.
const _: () = assert_disjoint(&[SimStack::REGION]);

/// Run `f` with its frames in the scratchpad.
///
/// # Safety
/// Call only for one of the two kinds of call above; no scratchpad data may
/// be live across it. The build's stack guard checks the rest.
#[inline(always)]
pub unsafe fn sim<R>(f: impl FnOnce() -> R) -> R {
    // SAFETY: the caller keeps the render lease dead; stack_guard bounds f.
    unsafe { SimStack::run(f) }
}
