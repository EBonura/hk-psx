//! Checked, allocation-free original FSM bytecode tables.
use crate::Error;

pub const MAGIC: &[u8; 8] = b"HKFSM01\0";
pub const HEADER: usize = 32;
pub const MAX_VARIABLES: usize = 16;
pub const MAX_STATE_ACTIONS: usize = 16;
pub const MAX_BYTES: usize = 16 * 1024;
pub const NO_EVENT: u16 = u16::MAX;
pub const FINISHED: u16 = 0;
pub const ENABLED: u8 = 1;
/// Kernel scheduling requirement, including waits/animation completion even
/// when the original action has no serialized `everyFrame` property.
pub const UPDATE: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub actions: u16,
    pub action_count: u16,
    pub transitions: u16,
    pub transition_count: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub opcode: u16,
    pub operands: u16,
    pub operand_count: u8,
    pub flags: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operand {
    // 0 constant, 1 variable, 2 event, 3 object role, 4 absent output.
    pub kind: u8,
    pub index: u16,
}
#[derive(Clone, Copy)]
pub struct Program<'a> {
    bytes: &'a [u8],
    states: u16,
    actions: u16,
    transitions: u16,
    operands: u16,
    variables: u16,
    constants: u16,
    events: u16,
    globals: u16,
    start: u16,
    offsets: [usize; 6],
}
fn word(b: &[u8], p: usize) -> u16 { u16::from_le_bytes([b[p], b[p + 1]]) }
fn integer(b: &[u8], p: usize) -> i32 { i32::from_le_bytes(b[p..p + 4].try_into().unwrap()) }
impl<'a> Program<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER || &bytes[..8] != MAGIC { return Err(Error::Header); }
        let counts = core::array::from_fn::<_, 8, _>(|i| word(bytes, 8 + i * 2));
        let [states, actions, transitions, operands, variables, constants, events, globals] = counts;
        let start = word(bytes, 24);
        if states == 0 || states > 256 || events == 0 || events > 256
            || variables as usize > MAX_VARIABLES || globals > transitions
            || start >= states || word(bytes, 26) != 0 || bytes.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        let sizes = [8usize, 8, 4, 4, 8, 4];
        let mut offsets = [0; 6];
        let mut end = HEADER;
        for i in 0..6 {
            offsets[i] = end;
            end = end.checked_add(counts[i] as usize * sizes[i]).ok_or(Error::Limit)?;
        }
        if end != bytes.len() || integer(bytes, 28) as u32 != bytes.len() as u32 {
            return Err(Error::Truncated);
        }
        let p = Self { bytes, states, actions, transitions, operands, variables,
            constants, events, globals, start, offsets };
        for i in 0..states {
            let s = p.state(i).unwrap();
            if s.action_count as usize > MAX_STATE_ACTIONS
                || s.actions as u32 + s.action_count as u32 > actions as u32
                || s.transitions as u32 + s.transition_count as u32 > (transitions - globals) as u32 {
                return Err(Error::Reference);
            }
        }
        for i in 0..actions {
            let a = p.action(i).unwrap();
            if a.flags & !(ENABLED | UPDATE) != 0
                || a.operands as u32 + a.operand_count as u32 > operands as u32 {
                return Err(Error::Reference);
            }
        }
        for i in 0..transitions {
            let (event, target) = p.transition(i).unwrap();
            if event >= events || target >= states { return Err(Error::Reference); }
        }
        for i in 0..operands {
            let o = p.operand(i).unwrap();
            let valid = if o.kind == 0 { o.index < constants }
                else if o.kind == 1 { o.index < variables }
                else if o.kind == 2 { o.index < events || o.index == NO_EVENT }
                else if o.kind == 3 { o.index <= 3 }
                else { o.kind == 4 && o.index == 0 };
            if !valid { return Err(Error::Reference); }
        }
        for i in 0..variables {
            if !(1..=3).contains(&p.variable_kind(i).unwrap()) { return Err(Error::Reference); }
        }
        Ok(p)
    }
    #[inline(always)]
    pub fn state(self, index: u16) -> Option<State> {
        if index >= self.states { return None; }
        let p = self.offsets[0] + index as usize * 8;
        Some(State { actions: word(self.bytes, p), action_count: word(self.bytes, p + 2),
            transitions: word(self.bytes, p + 4), transition_count: word(self.bytes, p + 6) })
    }
    /// Recreate a cheap view after this exact immutable bank passed `parse`.
    /// # Safety
    /// `bytes` must be the same bytes previously validated by `parse`.
    pub unsafe fn from_verified(bytes: &'a [u8]) -> Self {
        let counts = core::array::from_fn::<_,8,_>(|i|word(bytes,8+i*2));
        let [states,actions,transitions,operands,variables,constants,events,globals]=counts;
        let mut offsets=[0;6];let mut end=HEADER;
        for i in 0..6 { offsets[i]=end;end+=counts[i] as usize*[8,8,4,4,8,4][i]; }
        Self{bytes,states,actions,transitions,operands,variables,constants,events,globals,start:word(bytes,24),offsets}
    }
    pub fn variable_count(self) -> u16 { self.variables }
    pub fn state_count(self) -> u16 { self.states }
    pub fn event_count(self) -> u16 { self.events }
    pub fn action_count(self) -> u16 { self.actions }
    pub fn start_state(self) -> u16 { self.start }
    #[inline(always)]
    pub fn action(self, index: u16) -> Option<Action> {
        if index >= self.actions { return None; }
        let p = self.offsets[1] + index as usize * 8;
        Some(Action { opcode: word(self.bytes, p), operands: word(self.bytes, p + 2),
            operand_count: self.bytes[p + 4], flags: self.bytes[p + 5] })
    }
    #[inline(always)]
    pub fn transition(self, index: u16) -> Option<(u16, u16)> {
        if index >= self.transitions { return None; }
        let p = self.offsets[2] + index as usize * 4;
        Some((word(self.bytes, p), word(self.bytes, p + 2)))
    }
    #[inline(always)]
    pub fn operand(self, index: u16) -> Option<Operand> {
        if index >= self.operands { return None; }
        let p = self.offsets[3] + index as usize * 4;
        Some(Operand { kind: self.bytes[p], index: word(self.bytes, p + 2) })
    }
    #[inline(always)]
    pub fn variable_kind(self, index: u16) -> Option<u8> {
        (index < self.variables).then(|| self.bytes[self.offsets[4] + index as usize * 8 + 4])
    }
    #[inline(always)]
    pub fn initial(self, index: u16) -> Option<i32> {
        (index < self.variables).then(|| integer(self.bytes, self.offsets[4] + index as usize * 8))
    }
    #[inline(always)]
    pub fn constant(self, index: u16) -> Option<i32> {
        (index < self.constants).then(|| integer(self.bytes, self.offsets[5] + index as usize * 4))
    }
    #[inline(always)]
    pub fn target(self, state: u16, event: u16) -> Option<u16> {
        let s = self.state(state)?;
        for i in s.transitions..s.transitions + s.transition_count {
            let (e, target) = self.transition(i)?;
            if e == event { return Some(target); }
        }
        for i in self.transitions - self.globals..self.transitions {
            let (e, target) = self.transition(i)?;
            if e == event { return Some(target); }
        }
        None
    }
}

/// Shared wire opcodes for the supported original action kernels.
pub mod opcode {
pub const GET_OWNER:u16=1;
pub const FIND_OBJECT:u16=5;
pub const BOOL_TEST:u16=13;
pub const GET_DISTANCE:u16=23;
pub const FLOAT_COMPARE:u16=37;
pub const FLOAT_SWITCH:u16=53;
pub const SET_BOOL:u16=71;
pub const SET_FLOAT:u16=97;
pub const RANDOM_FLOAT:u16=127;
pub const FACE_DIRECTION:u16=157;
pub const SET_VELOCITY_ANGLE:u16=191;
pub const COLLISION_STAY:u16=223;
pub const COLLISION_ENTER:u16=251;
pub const IMPLEMENTED_TYPES:usize=13;

}
