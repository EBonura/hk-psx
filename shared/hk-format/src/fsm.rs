//! Checked, allocation-free original FSM bytecode tables.
use crate::Error;

pub const MAGIC: &[u8; 8] = b"HKFSM01\0";
/// Cook-normalized operands: signed 31-bit literals, namespace-mapped cells,
/// explicit events and absent outputs. Original state/action order is retained.
pub const NORMAL_MAGIC: &[u8; 8] = b"HKFSM02\0";
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
/// Core ordered executor and bounded operand helpers.
pub const GROUP_CORE: u8 = 1;
/// Wait, Translate and WalkLeftRight kernels; BoolTest is shared core.
pub const GROUP_WALK: u8 = 2;
/// Remaining admitted Gruzzer kernels.
pub const GROUP_FLY: u8 = 4;
/// Vengefly-specific kernels; not an admission of the whole graph.
pub const GROUP_CHASE: u8 = 8;

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
    /// Required kernels across the entire immutable action table, including
    /// every state and global transition destination. Unknown actions refuse
    /// admission rather than relying on which route happened to execute.
    pub fn action_groups(self) -> Result<u8, Error> {
        use opcode::*;
        let chase=self.actions_checked_chase()?;
        if chase{return Ok(GROUP_CORE|GROUP_CHASE);}
        let mut groups = GROUP_CORE;
        for index in 0..self.actions {
            let action = self.action(index).ok_or(Error::Limit)?;
            if action.flags & ENABLED == 0 { continue; }
            let op = action.opcode;
            if op == BOOL_TEST {
                // Both concrete family closures implement this core action.
            } else if op == WAIT || op == TRANSLATE || op == WALK_LEFT_RIGHT {
                groups |= GROUP_WALK;
            } else if op == GET_OWNER || op == FIND_OBJECT || op == SET_BOOL || op == SET_FLOAT
                || op == RANDOM_FLOAT || op == GET_DISTANCE || op == FLOAT_COMPARE || op == FLOAT_SWITCH
                || op == FACE_DIRECTION || op == SET_VELOCITY_ANGLE || op == COLLISION_STAY || op == COLLISION_ENTER {
                groups |= GROUP_FLY;
            } else if op==FLOAT_CLAMP||op==BOOL_ALL_TRUE||op==NEXT_FRAME_EVENT||op==GET_VELOCITY_2D||op==SET_VELOCITY_2D||op==ADD_FORCE_2D||op==DECELERATE||op==CHECK_CAN_SEE_HERO||op==CHECK_ALERT_RANGE||op==FIND_ALERT_RANGE||op==PLAY_ANIMATION||op==PLAY_FRAME||op==WATCH_ANIMATION||op==SET_AUDIO_PITCH||op==IDLE_BUZZ||op==CHASE_OBJECT||op==FACE_OBJECT||op==SET_ROTATION||op==SEND_EVENT_BY_NAME||op==AUDIO_ONE_SHOT {
                groups|=GROUP_CHASE;
            } else { return Err(Error::Limit); }
        }
        Ok(groups)
    }
    fn actions_checked_chase(self)->Result<bool,Error>{
        use opcode::*;
        let mut chase=false;
        for i in 0..self.actions{
            let a=self.action(i).ok_or(Error::Limit)?;
            if a.flags&ENABLED!=0&&matches!(a.opcode,IDLE_BUZZ|CHASE_OBJECT|ADD_FORCE_2D|CHECK_CAN_SEE_HERO|CHECK_ALERT_RANGE|FIND_ALERT_RANGE){chase=true;}
        }
        if chase{
            for i in 0..self.actions{
                let a=self.action(i).ok_or(Error::Limit)?;
                if a.flags&ENABLED==0{continue;}
                if !matches!(a.opcode,GET_OWNER|BOOL_TEST|FACE_DIRECTION|WAIT|GET_VELOCITY_2D|SET_VELOCITY_2D|ADD_FORCE_2D|DECELERATE|FLOAT_CLAMP|BOOL_ALL_TRUE|NEXT_FRAME_EVENT|CHECK_CAN_SEE_HERO|CHECK_ALERT_RANGE|FIND_ALERT_RANGE|PLAY_ANIMATION|PLAY_FRAME|WATCH_ANIMATION|SET_AUDIO_PITCH|IDLE_BUZZ|CHASE_OBJECT|FACE_OBJECT|SET_ROTATION|SEND_EVENT_BY_NAME|AUDIO_ONE_SHOT){return Err(Error::Limit);}
            }
        }Ok(chase)
    }
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER || (&bytes[..8] != MAGIC && &bytes[..8] != NORMAL_MAGIC) { return Err(Error::Header); }
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
        if &bytes[..8] == NORMAL_MAGIC && constants != 0 { return Err(Error::Reference); }
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
            if p.normalized() && a.operand_count > 8 { return Err(Error::Limit); }
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
            let o = p.operand_for_format(i).ok_or(Error::Reference)?;
            let valid = if o.kind == 0 { if p.normalized() { o.index < operands } else { o.index < constants } }
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
    pub fn normalized(self) -> bool { &self.bytes[..8] == NORMAL_MAGIC }
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
    /// Format-aware access is confined to normalized consumers. Existing
    /// walking/flying kernels keep their legacy accessors and code closure.
    #[inline(always)]
    pub fn operand_for_format(self, index: u16) -> Option<Operand> {
        if !self.normalized() { return self.operand(index); }
        if index >= self.operands { return None; }
        let value = integer(self.bytes, self.offsets[3] + index as usize * 4) as u32;
        if value & 0x80000000 == 0 { return Some(Operand { kind: 0, index }); }
        match value >> 16 {
            0x8000 => Some(Operand { kind: 1, index: value as u16 }),
            0x8001 => Some(Operand { kind: 2, index: value as u16 }),
            0x8002 if value as u16 == 0 => Some(Operand { kind: 4, index: 0 }),
            _ => None,
        }
    }
    #[inline(always)]
    pub fn variable_kind(self, index: u16) -> Option<u8> {
        (index < self.variables).then(|| self.bytes[self.offsets[4] + index as usize * 8 + 4])
    }
    #[inline(always)]
    pub fn initial(&self, index: u16) -> Option<i32> {
        (index < self.variables).then(|| integer(self.bytes, self.offsets[4] + index as usize * 8))
    }
    #[inline(always)]
    pub fn constant(self, index: u16) -> Option<i32> {
        (index < self.constants).then(|| integer(self.bytes, self.offsets[5] + index as usize * 4))
    }
    #[inline(always)]
    pub fn constant_for_format(self, index: u16) -> Option<i32> {
        if !self.normalized() { return self.constant(index); }
        if index >= self.operands { return None; }
        let value = integer(self.bytes, self.offsets[3] + index as usize * 4);
        (value >= 0).then(|| (value << 1) >> 1)
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
pub const WAIT:u16=269;
pub const TRANSLATE:u16=283;
pub const WALK_LEFT_RIGHT:u16=307;
pub const GET_VELOCITY_2D:u16=331;
pub const SET_VELOCITY_2D:u16=347;
pub const ADD_FORCE_2D:u16=359;
pub const DECELERATE:u16=449;
pub const CHECK_CAN_SEE_HERO:u16=461;
pub const CHECK_ALERT_RANGE:u16=479;
pub const FIND_ALERT_RANGE:u16=487;
pub const PLAY_ANIMATION:u16=503;
pub const PLAY_FRAME:u16=521;
pub const WATCH_ANIMATION:u16=541;
pub const SET_AUDIO_PITCH:u16=557;
pub const IDLE_BUZZ:u16=569;
pub const CHASE_OBJECT:u16=587;
pub const FACE_OBJECT:u16=601;
pub const SET_ROTATION:u16=613;
pub const SEND_EVENT_BY_NAME:u16=631;
pub const AUDIO_ONE_SHOT:u16=647;
pub const FLOAT_CLAMP:u16=373;
pub const BOOL_ALL_TRUE:u16=419;
pub const NEXT_FRAME_EVENT:u16=431;
// Installed coverage changes only after the complete consumer is qualified.
pub const IMPLEMENTED_TYPES:usize=16;

}

#[cfg(test)]
mod normalized_operand_tests {
    use super::*;
    extern crate std;
    use std::vec::Vec;
    fn bank(words:&[u32])->Vec<u8> {
        let mut b=Vec::from(*NORMAL_MAGIC);
        for n in [1u16,1,0,words.len() as u16,1,0,2,0,0,0] { b.extend(n.to_le_bytes()); }
        b.extend(0u32.to_le_bytes());
        for n in [0u16,1,0,0] { b.extend(n.to_le_bytes()); }
        b.extend(opcode::BOOL_TEST.to_le_bytes());b.extend(0u16.to_le_bytes());b.extend([words.len() as u8,ENABLED,0,0]);
        for word in words {b.extend(word.to_le_bytes());}
        b.extend(1i32.to_le_bytes());b.extend([1,0,0,0]);
        let len=b.len() as u32;b[28..32].copy_from_slice(&len.to_le_bytes());b
    }
    #[test]
    fn signed_inline_literals_events_cells_and_none_remain_distinct() {
        let bytes=bank(&[(-655294464i32 as u32)&0x7fffffff,655294464,0x80000000,0x8001ffff,0x80020000]);
        let p=Program::parse(&bytes).unwrap();
        assert_eq!(p.constant_for_format(0),Some(-655294464));
        assert_eq!(p.constant_for_format(1),Some(655294464));
        for (at,kind,index) in [(2,1,0),(3,2,NO_EVENT),(4,4,0)] {assert_eq!(p.operand_for_format(at),Some(Operand{kind,index}));}
        assert_eq!(p.constant_for_format(2),None);
    }
    #[test]
    fn malformed_tags_aliases_event_ids_and_absent_payloads_are_refused() {
        for word in [0x80030000,0x80000001,0x80010002,0x80020001] {assert!(Program::parse(&bank(&[word])).is_err());}
    }
}
