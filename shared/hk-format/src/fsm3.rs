//! HKFSM03: the typed PlayMaker bytecode bank (docs/FSM-V2-DESIGN.md section 2.1).
//!
//! One bank holds every FSM of one actor tree. All offsets are 16-bit and
//! relative to the bank start, so a bank is a data package without fixups.
//!
//! ```text
//! header 16 B      "HKF3", version, fsm count, event count, entity count,
//!                  string count u16, queue capacity, spare, bank length u32
//! directory        32 B per FSM (see `Dir`)
//! event map        per tree event, a bitmask of the FSMs that handle it
//! per FSM:
//!   state table    4 B/state  {block offset u16 (in u16 units from the FSM start), actions u8, transitions u8}
//!   state blocks   transitions first (2 B each: event, target), then the actions
//!   globals        2 B each (event, target); searched after a state's own transitions
//!   arrays         u16 words: a count, then that many operands (ARRAY operands address the count)
//!   constant pool  4 B each (aligned)
//!   init           (slot u8|u16, value i32) for non-zero cells, then the bit image, then the object image
//! action           byte 0 = family | flags << 4, byte 1 = op, then `arity` u16 operands
//! operand          tag (3 bits) << 13 | payload (13 bits)
//! ```
//!
//! Operand counts come from the op schema (`ops::ARITY`), not from the stream, so the
//! kernel can step over an action it does not run.
use crate::Error;

pub mod ops;

pub const MAGIC: &[u8; 4] = b"HKF3";
pub const VERSION: u8 = 1;
pub const HEADER: usize = 16;
pub const DIR: usize = 32;
pub const FAMILIES: usize = 14;

// Operand tags.
pub const T_CONST: u16 = 0;
pub const T_CELL: u16 = 1;
pub const T_BIT: u16 = 2;
pub const T_OBJ: u16 = 3;
pub const T_EVENT: u16 = 4;
pub const T_NONE: u16 = 5;
pub const T_INLINE: u16 = 6;
pub const T_ARRAY: u16 = 7;

// Action header flags (the high nibble of byte 0).
pub const F_UPDATE: u8 = 1;
pub const F_FIXED: u8 = 2;
pub const F_LATE: u8 = 4;
/// The first FsmOwnerDefault parameter is "use owner" and its operand is omitted.
pub const F_OWNER: u8 = 8;

/// The FINISHED system event is event 0 of every tree.
pub const FINISHED: u8 = 0;
/// No event / no object.
pub const NONE8: u8 = 0xff;
/// Entity handles with a fixed meaning. Handle 0 is the actor itself.
pub const ENT_SELF: u8 = 0;
pub const ENT_HERO: u8 = 0xfe;
pub const ENT_CAMERA: u8 = 0xfd;

/// Split an operand word into (tag, payload).
#[inline(always)]
pub const fn split(word: u16) -> (u16, u16) {
    (word >> 13, word & 0x1fff)
}
#[inline(always)]
pub const fn operand(tag: u16, payload: u16) -> u16 {
    tag << 13 | payload
}
/// Sign-extend an INLINE payload.
#[inline(always)]
pub const fn inline_value(payload: u16) -> i32 {
    ((payload as i32) << 19) >> 19
}

#[inline(always)]
fn le16(b: &[u8], at: usize) -> usize {
    b[at] as usize | (b[at + 1] as usize) << 8
}

/// One FSM's directory record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dir {
    pub states: u8,
    pub globals: u8,
    pub start: u8,
    pub fin_bytes: u8,
    pub cells: u16,
    pub bits: u16,
    pub objs: u16,
    pub scratch: u8,
    pub wide: bool,
    /// Byte offsets: state table, globals, arrays, pool, init, end.
    pub state_at: u16,
    pub globals_at: u16,
    pub arrays_at: u16,
    pub pool_at: u16,
    pub init_at: u16,
    pub end: u16,
    pub pool_count: u16,
    pub init_count: u16,
    pub array_words: u16,
}

/// A state record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateRec {
    /// Byte offset of the state's block (transitions, then actions).
    pub block: u16,
    pub actions: u8,
    pub transitions: u8,
}

/// A read-only view of one bank.
#[derive(Clone, Copy)]
pub struct Bank<'a> {
    bytes: &'a [u8],
}

impl<'a> Bank<'a> {
    /// Byte `i` of the bank. Every index the kernel uses comes from a bank `parse` accepted, so the read is not
    /// bounds-checked in a release build (a checked build asserts it); that is also what keeps the resident kernel
    /// free of panic paths.
    #[inline(always)]
    pub fn byte(self, i: usize) -> u8 {
        debug_assert!(i < self.bytes.len());
        // SAFETY: see above.
        unsafe { *self.bytes.get_unchecked(i) }
    }
    #[inline(always)]
    pub fn u16_at(self, i: usize) -> usize {
        self.byte(i) as usize | (self.byte(i + 1) as usize) << 8
    }
    /// A view of bytes that `parse` accepted before (the cooker checks every bank and the
    /// module transport hash-checks it at install).
    ///
    /// # Safety
    /// `bytes` must be exactly bytes that passed `Bank::parse`.
    #[inline(always)]
    pub const unsafe fn from_verified(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
    #[inline(always)]
    pub fn bytes(self) -> &'a [u8] {
        self.bytes
    }
    #[inline(always)]
    pub fn fsm_count(self) -> u8 {
        self.byte(5)
    }
    #[inline(always)]
    pub fn event_count(self) -> u8 {
        self.byte(6)
    }
    #[inline(always)]
    pub fn entity_count(self) -> u8 {
        self.byte(7)
    }
    #[inline(always)]
    pub fn string_count(self) -> u16 {
        self.u16_at(8) as u16
    }
    /// Delayed-event queue entries the runtime block reserves.
    #[inline(always)]
    pub fn queue_cap(self) -> u8 {
        self.byte(10)
    }
    /// Bytes of the per-event FSM bitmask.
    #[inline(always)]
    pub fn mask_bytes(self) -> usize {
        (self.fsm_count() as usize).div_ceil(8)
    }
    #[inline(always)]
    pub fn map_at(self) -> usize {
        HEADER + DIR * self.fsm_count() as usize
    }
    /// Whether FSM `f` has a transition for `event`.
    #[inline(always)]
    pub fn handles(self, f: u8, event: u8) -> bool {
        self.byte(self.map_at() + event as usize * self.mask_bytes() + (f >> 3) as usize) & (1 << (f & 7)) != 0
    }
    #[inline(always)]
    pub fn dir_at(self, f: u8) -> usize {
        HEADER + DIR * f as usize
    }
    pub fn dir(self, f: u8) -> Dir {
        let r = self.dir_at(f);
        let u = |k: usize| self.u16_at(r + k) as u16;
        Dir {
            states: self.byte(r),
            globals: self.byte(r + 1),
            start: self.byte(r + 2),
            fin_bytes: self.byte(r + 3),
            cells: u(4),
            bits: u(6),
            objs: u(8),
            scratch: self.byte(r + 10),
            wide: self.byte(r + 11) != 0,
            state_at: u(12),
            globals_at: u(14),
            arrays_at: u(16),
            pool_at: u(18),
            init_at: u(20),
            end: u(22),
            pool_count: u(24),
            init_count: u(26),
            array_words: u(28),
        }
    }
    #[inline(always)]
    pub fn state(self, f: u8, s: u8) -> StateRec {
        let d = self.dir_at(f);
        let sec = self.u16_at(d + 12);
        let t = sec + 4 * s as usize;
        StateRec { block: (sec + 2 * self.u16_at(t)) as u16, actions: self.byte(t + 2), transitions: self.byte(t + 3) }
    }
    /// The constant pool entry `i` of FSM `f`.
    #[inline(always)]
    pub fn pool(self, f: u8, i: usize) -> i32 {
        let at = self.u16_at(self.dir_at(f) + 18) + 4 * i;
        i32::from_le_bytes([self.byte(at), self.byte(at + 1), self.byte(at + 2), self.byte(at + 3)])
    }
    /// The array starting at operand payload `a` of FSM `f`: (count, byte offset of its first operand).
    #[inline(always)]
    pub fn array(self, f: u8, a: usize) -> (usize, usize) {
        let at = self.u16_at(self.dir_at(f) + 16) + 2 * a;
        (self.u16_at(at), at + 2)
    }
    #[inline(always)]
    pub fn word(self, at: usize) -> u16 {
        self.u16_at(at) as u16
    }
    /// Operand count of the action whose header is at byte `at`.
    #[inline(always)]
    pub fn arity_at(self, at: usize) -> usize {
        let h = self.byte(at);
        ops::arity(h & 0xf, self.byte(at + 1), h >> 4)
    }

    /// Bytes of runtime memory a tree of this bank needs (`Runtime::layout`).
    pub fn runtime_bytes(self) -> usize {
        let mut n = 8 + 4 * self.queue_cap() as usize + 4 * self.fsm_count() as usize;
        for f in 0..self.fsm_count() {
            n = (n + 3) & !3;
            let d = self.dir(f);
            n += 4 * d.cells as usize + d.scratch as usize;
            n += (d.bits as usize).div_ceil(8) + d.objs as usize + d.fin_bytes as usize;
        }
        (n + 3) & !3
    }

    /// Full structural check: sizes, offsets, every state and action, every operand against its
    /// FSM's storage, every op against the schema table. The guest never runs this.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER || &bytes[..4] != MAGIC || bytes[4] != VERSION {
            return Err(Error::Header);
        }
        if bytes.len() > 0xffff || u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize != bytes.len() {
            return Err(Error::Truncated);
        }
        let b = Self { bytes };
        let n = b.fsm_count() as usize;
        let events = b.event_count() as usize;
        if n == 0 || events == 0 || bytes.len() < b.map_at() + events * b.mask_bytes() {
            return Err(Error::Limit);
        }
        for f in 0..n as u8 {
            let d = b.dir(f);
            let end = d.end as usize;
            if end > bytes.len() || d.states == 0 || d.start >= d.states {
                return Err(Error::Reference);
            }
            if !(b.map_at() <= d.state_at as usize
                && d.state_at <= d.globals_at
                && d.globals_at <= d.arrays_at
                && d.arrays_at <= d.pool_at
                && d.pool_at <= d.init_at
                && d.init_at as usize <= end)
            {
                return Err(Error::Reference);
            }
            if d.pool_at as usize + 4 * d.pool_count as usize > d.init_at as usize
                || d.arrays_at as usize + 2 * d.array_words as usize > d.pool_at as usize
                || d.globals_at as usize + 2 * d.globals as usize > d.arrays_at as usize
            {
                return Err(Error::Reference);
            }
            let check = |w: u16| -> Result<(), Error> {
                let (tag, p) = split(w);
                let ok = match tag {
                    T_CONST => (p as usize) < d.pool_count as usize,
                    T_CELL => p < d.cells,
                    T_BIT => p < d.bits,
                    T_OBJ => p < d.objs,
                    T_EVENT => (p as usize) < events,
                    T_NONE | T_INLINE => true,
                    _ => (p as usize) < d.array_words as usize,
                };
                if ok {
                    Ok(())
                } else {
                    Err(Error::Reference)
                }
            };
            for s in 0..d.states {
                let rec = b.state(f, s);
                if rec.actions as usize > d.fin_bytes as usize * 8 {
                    return Err(Error::Reference);
                }
                let mut at = rec.block as usize;
                for _ in 0..rec.transitions {
                    if at + 2 > end || bytes[at] as usize >= events || bytes[at + 1] >= d.states {
                        return Err(Error::Reference);
                    }
                    at += 2;
                }
                for _ in 0..rec.actions {
                    if at + 2 > end {
                        return Err(Error::Truncated);
                    }
                    let (fam, op) = (bytes[at] & 0xf, bytes[at + 1]);
                    if fam as usize >= FAMILIES || !ops::known(fam, op) {
                        return Err(Error::Reference);
                    }
                    let k = ops::arity(fam, op, bytes[at] >> 4);
                    if at + 2 + 2 * k > end {
                        return Err(Error::Truncated);
                    }
                    for i in 0..k {
                        check(le16(bytes, at + 2 + 2 * i) as u16)?;
                    }
                    at += 2 + 2 * k;
                }
            }
            for g in 0..d.globals as usize {
                let at = d.globals_at as usize + 2 * g;
                if bytes[at] as usize >= events || bytes[at + 1] >= d.states {
                    return Err(Error::Reference);
                }
            }
            // Array words: a count followed by operands; walk them.
            let mut at = d.arrays_at as usize;
            let stop = at + 2 * d.array_words as usize;
            while at < stop {
                let count = le16(bytes, at);
                at += 2;
                if at + 2 * count > stop {
                    return Err(Error::Truncated);
                }
                for i in 0..count {
                    check(le16(bytes, at + 2 * i) as u16)?;
                }
                at += 2 * count;
            }
        }
        Ok(b)
    }
}
