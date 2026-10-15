//! The resident HKFSM03 kernel, written against the bank `fsm3-sizes` produces
//! (host/hk-fsm/src/v3.rs). Everything a PlayMaker FSM needs that is not an action
//! handler: enter, update, event delivery, transitions, the finished bitset, the
//! delayed-event queue, the tree scheduler, operand access for handlers and the
//! family entry dispatch.
//!
//! It is a measuring kernel: it runs real banks with stub handlers, and its byte count
//! is the number DESIGN.md section 2.2 only budgeted. No i64, no float, no allocation.
#![allow(clippy::missing_safety_doc)]

use crate::arity::{ARITY, BASE};

pub const R_CONT: u16 = 0;
pub const R_FIN: u16 = 1;
/// Handler result for "send event e to this FSM, stop running later actions".
#[inline(always)]
pub const fn r_event(e: u8) -> u16 {
    2 + e as u16
}

pub const T_CONST: u16 = 0;
pub const T_CELL: u16 = 1;
pub const T_BIT: u16 = 2;
pub const T_OBJ: u16 = 3;
pub const T_EVENT: u16 = 4;
pub const T_NONE: u16 = 5;
pub const T_INLINE: u16 = 6;

pub const F_UPDATE: u8 = 1;
pub const F_FIXED: u8 = 2;
pub const F_LATE: u8 = 4;
pub const F_OWNER: u8 = 8;

const HEADER: usize = 16;
const DIR: usize = 32;
const FAMILIES: usize = 14;
pub const QUEUE: usize = 16;
const BUDGET: u8 = 32;

#[repr(C)]
pub struct Fsm {
    pub dir: u16,
    pub state: u8,
    pub prev: u8,
    pub flags: u8,
    pub fin_bytes: u8,
    pub cells: *mut i32,
    pub bits: *mut u8,
    pub objs: *mut u8,
    pub scratch: *mut u32,
    pub fin: *mut u8,
}

pub struct Queue {
    pub n: u8,
    pub ev: [u8; QUEUE],
    pub from: [u8; QUEUE],
    pub delay: [u16; QUEUE],
}

pub struct Tree {
    pub bank: *const u8,
    pub n: u8,
    pub mask: u8,
    pub map_at: u16,
    pub fsms: *mut Fsm,
    pub q: Queue,
    pub budget: u8,
    pub tick: u32,
    pub rng: u32,
    pub ran: u32,
}

/// What a handler sees: the tree, the FSM, the action's operands.
#[repr(C)]
pub struct Ctx {
    pub t: *mut Tree,
    pub f: u8,
    pub flags: u8,
    pub index: u8,
    pub enter: bool,
    pub ops: *const u16,
}

pub type Handler = unsafe extern "C" fn(&mut Ctx, u8) -> u16;

/// The 14 family entry points. Resident data words; in the guest each is bound to the
/// family module's entry, or to the module trap while the module is evicted.
#[no_mangle]
pub static mut FAMILY_ENTRY: [Handler; FAMILIES] = [crate::families::stub; FAMILIES];

#[inline(always)]
unsafe fn rd16(p: *const u8, off: usize) -> usize {
    *(p.add(off) as *const u16) as usize
}

#[inline(always)]
unsafe fn dir(t: &Tree, f: u8) -> *const u8 {
    t.bank.add(HEADER + DIR * f as usize)
}

/// Operand count of one action, from the resident schema table.
#[inline(always)]
fn arity(fam: u8, op: u8, flags: u8) -> usize {
    ARITY[BASE[fam as usize] as usize + op as usize] as usize - (flags & F_OWNER != 0) as usize
}

// ---- operand access (what handlers import) -----------------------------------------

#[inline(never)]
pub unsafe extern "C" fn k_word(c: &Ctx, n: u8) -> u16 {
    *c.ops.add(n as usize)
}

/// Operand as a value. `q16` selects the INLINE reading: a Q16.16 float literal or a plain integer.
#[inline(never)]
pub unsafe extern "C" fn k_val(c: &Ctx, n: u8, q16: bool) -> i32 {
    let w = *c.ops.add(n as usize);
    let p = (w & 0x1fff) as usize;
    let t = &*c.t;
    let f = &*t.fsms.add(c.f as usize);
    match w >> 13 {
        T_CELL => *f.cells.add(p),
        T_INLINE => {
            let v = ((p as i32) << 19) >> 19;
            if q16 {
                v << 16
            } else {
                v
            }
        }
        T_CONST => {
            let d = dir(t, c.f);
            let pool = rd16(d, 18);
            *(t.bank.add(pool + 4 * p) as *const i32)
        }
        _ => 0,
    }
}

#[inline(never)]
pub unsafe extern "C" fn k_set(c: &Ctx, n: u8, v: i32) {
    let w = *c.ops.add(n as usize);
    if w >> 13 == T_CELL {
        let f = &*(*c.t).fsms.add(c.f as usize);
        *f.cells.add((w & 0x1fff) as usize) = v;
    }
}

#[inline(never)]
pub unsafe extern "C" fn k_bit(c: &Ctx, n: u8) -> bool {
    let w = *c.ops.add(n as usize);
    let p = (w & 0x1fff) as usize;
    if w >> 13 == T_BIT {
        let f = &*(*c.t).fsms.add(c.f as usize);
        *f.bits.add(p >> 3) & (1 << (p & 7)) != 0
    } else {
        p != 0
    }
}

#[inline(never)]
pub unsafe extern "C" fn k_set_bit(c: &Ctx, n: u8, v: bool) {
    let w = *c.ops.add(n as usize);
    if w >> 13 == T_BIT {
        let p = (w & 0x1fff) as usize;
        let f = &*(*c.t).fsms.add(c.f as usize);
        let b = f.bits.add(p >> 3);
        *b = (*b & !(1 << (p & 7))) | ((v as u8) << (p & 7));
    }
}

#[inline(never)]
pub unsafe extern "C" fn k_obj(c: &Ctx, n: u8) -> u8 {
    let w = *c.ops.add(n as usize);
    if w >> 13 == T_OBJ {
        let f = &*(*c.t).fsms.add(c.f as usize);
        *f.objs.add((w & 0x1fff) as usize)
    } else {
        0xff
    }
}

/// An EVENT operand as an event id; NONE is 0xff.
#[inline(never)]
pub unsafe extern "C" fn k_event(c: &Ctx, n: u8) -> u8 {
    let w = *c.ops.add(n as usize);
    if w >> 13 == T_EVENT {
        (w & 0xff) as u8
    } else {
        0xff
    }
}

#[inline(never)]
pub unsafe extern "C" fn k_scratch(c: &Ctx, i: u8) -> *mut u32 {
    (*(*c.t).fsms.add(c.f as usize)).scratch.add(i as usize)
}

#[inline(never)]
pub unsafe extern "C" fn k_rand(c: &Ctx) -> u32 {
    let t = &mut *c.t;
    let mut x = t.rng;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    t.rng = x;
    x
}

/// Send `ev` now (delay 0) or after `delay` ticks, from this FSM.
#[inline(never)]
pub unsafe extern "C" fn k_send(c: &Ctx, ev: u8, delay: u16) {
    send(&mut *c.t, c.f, ev, delay);
}

// ---- the machine ----------------------------------------------------------------------

#[inline(never)]
unsafe fn send(t: &mut Tree, from: u8, ev: u8, delay: u16) {
    if ev == 0xff {
        return;
    }
    if delay == 0 {
        for g in 0..t.n {
            if *t.bank.add(t.map_at as usize + ev as usize * t.mask as usize + (g >> 3) as usize) & (1 << (g & 7)) != 0 {
                deliver(t, g, ev);
            }
        }
        return;
    }
    let q = &mut t.q;
    if (q.n as usize) < QUEUE {
        let i = q.n as usize;
        q.ev[i] = ev;
        q.from[i] = from;
        q.delay[i] = delay;
        q.n += 1;
    }
}

#[inline(never)]
unsafe fn deliver(t: &mut Tree, f: u8, ev: u8) {
    let fsm = &*t.fsms.add(f as usize);
    let d = dir(t, f);
    let sec = rd16(d, 12);
    let rec = t.bank.add(sec + 4 * fsm.state as usize);
    let nt = *rec.add(3) as usize;
    let mut at = sec + 2 * rd16(rec, 0);
    for _ in 0..nt {
        if *t.bank.add(at) == ev {
            let target = *t.bank.add(at + 1);
            return transition(t, f, target);
        }
        at += 2;
    }
    let mut g = rd16(d, 14);
    for _ in 0..*d.add(1) {
        if *t.bank.add(g) == ev {
            let target = *t.bank.add(g + 1);
            return transition(t, f, target);
        }
        g += 2;
    }
}

#[inline(never)]
unsafe fn transition(t: &mut Tree, f: u8, target: u8) {
    if t.budget == 0 {
        return;
    }
    t.budget -= 1;
    enter_state(t, f, target);
}

#[inline(never)]
unsafe fn all_finished(fsm: &Fsm, na: usize) -> bool {
    let mut left = na;
    let mut i = 0;
    while left >= 8 {
        if *fsm.fin.add(i) != 0xff {
            return false;
        }
        i += 1;
        left -= 8;
    }
    left == 0 || *fsm.fin.add(i) & ((1u8 << left) - 1) == (1u8 << left) - 1
}

#[inline(never)]
pub unsafe fn enter_state(t: &mut Tree, f: u8, state: u8) {
    let fsm = &mut *t.fsms.add(f as usize);
    fsm.prev = fsm.state;
    fsm.state = state;
    for i in 0..fsm.fin_bytes as usize {
        *fsm.fin.add(i) = 0;
    }
    run_actions(t, f, true);
}

/// Run a state's actions: all of them on entry, then only the UPDATE ones each tick, in
/// authored order; a returned event interrupts the rest.
#[inline(never)]
pub unsafe fn run_actions(t: &mut Tree, f: u8, enter: bool) {
    let fsm = &*t.fsms.add(f as usize);
    let d = dir(t, f);
    let sec = rd16(d, 12);
    let rec = t.bank.add(sec + 4 * fsm.state as usize);
    let na = *rec.add(2) as usize;
    let nt = *rec.add(3) as usize;
    let mut p = t.bank.add(sec + 2 * rd16(rec, 0) + 2 * nt);
    for i in 0..na {
        let h0 = *p;
        let (fam, flags, op) = (h0 & 0xf, h0 >> 4, *p.add(1));
        let n = arity(fam, op, flags);
        let done = *fsm.fin.add(i >> 3) & (1 << (i & 7)) != 0;
        if !done && (enter || flags & F_UPDATE != 0) {
            let mut c = Ctx { t: t as *mut Tree, f, flags, index: i as u8, enter, ops: p.add(2) as *const u16 };
            let r = (core::ptr::read_volatile(&raw const FAMILY_ENTRY[fam as usize]))(&mut c, op);
            t.ran += 1;
            if r == R_FIN {
                *fsm.fin.add(i >> 3) |= 1 << (i & 7);
                if all_finished(fsm, na) {
                    deliver(t, f, 0);
                    return;
                }
            } else if r >= 2 {
                deliver(t, f, (r - 2) as u8);
                return;
            }
        }
        p = p.add(2 + 2 * n);
    }
}

/// Start every FSM in its start state.
#[inline(never)]
pub unsafe fn tree_start(t: &mut Tree) {
    t.budget = BUDGET;
    for f in 0..t.n {
        let s = *dir(t, f).add(2);
        enter_state(t, f, s);
    }
}

/// One simulation tick: due delayed events, then every FSM's update pass in bank order.
#[inline(never)]
pub unsafe fn tree_tick(t: &mut Tree) {
    t.budget = BUDGET;
    t.tick = t.tick.wrapping_add(1);
    let mut i = 0;
    while i < t.q.n as usize {
        if t.q.delay[i] <= 1 {
            let (ev, from) = (t.q.ev[i], t.q.from[i]);
            let last = t.q.n as usize - 1;
            t.q.ev[i] = t.q.ev[last];
            t.q.from[i] = t.q.from[last];
            t.q.delay[i] = t.q.delay[last];
            t.q.n -= 1;
            send(t, from, ev, 0);
        } else {
            t.q.delay[i] -= 1;
            i += 1;
        }
    }
    for f in 0..t.n {
        run_actions(t, f, false);
    }
}

/// Lay a bank out in caller-provided storage and return the tree.
#[inline(never)]
pub unsafe fn tree_init(t: &mut Tree, bank: *const u8, fsms: *mut Fsm, mut arena: *mut u32) {
    t.bank = bank;
    t.n = *bank.add(5);
    let nev = *bank.add(6) as usize;
    t.mask = (t.n as usize).div_ceil(8) as u8;
    t.map_at = (HEADER + DIR * t.n as usize) as u16;
    t.fsms = fsms;
    t.q.n = 0;
    t.rng = 0x1234_5678;
    let _ = nev;
    for f in 0..t.n {
        let d = dir(t, f);
        let cells = rd16(d, 4);
        let bits = rd16(d, 6);
        let objs = rd16(d, 8);
        let scratch = *d.add(10) as usize;
        let fin = *d.add(3) as usize;
        let s = &mut *fsms.add(f as usize);
        s.dir = (HEADER + DIR * f as usize) as u16;
        s.fin_bytes = fin as u8;
        s.state = *d.add(2);
        s.prev = s.state;
        s.flags = 1;
        s.cells = arena as *mut i32;
        arena = arena.add(cells);
        s.scratch = arena;
        arena = arena.add(scratch);
        s.bits = arena as *mut u8;
        arena = arena.add(bits.div_ceil(8).div_ceil(4));
        s.objs = arena as *mut u8;
        arena = arena.add(objs.div_ceil(4));
        s.fin = arena as *mut u8;
        arena = arena.add(fin.div_ceil(4));
        // initial images: cells, then bits and objects from the tail of the section
        let init_at = rd16(d, 20);
        let n_init = rd16(d, 26);
        let wide = *d.add(11) != 0;
        let mut at = init_at;
        for _ in 0..n_init {
            let slot = if wide {
                let v = rd16(bank, at);
                at += 2;
                v
            } else {
                let v = *bank.add(at) as usize;
                at += 1;
                v
            };
            // 5-byte entries (u8 slot, i32 value): the value is not word aligned.
            *s.cells.add(slot) = core::ptr::read_unaligned(bank.add(at) as *const i32);
            at += 4;
        }
        let end = rd16(d, 22);
        let tail = end - at;
        let bi = tail - objs;
        for i in 0..bi {
            *s.bits.add(i) = *bank.add(at + i);
        }
        for i in 0..objs {
            *s.objs.add(i) = *bank.add(at + bi + i);
        }
    }
}
