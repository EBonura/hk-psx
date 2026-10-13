//! Lossless storage for the two admitted Vengefly component namespaces.
//! Layout is derived from validated variable kinds; no original scalar is folded.
use crate::fsm::{Cursor, Error, Kernel, Phase};
use hk_format::fsm::{Program, MAX_VARIABLES};
#[derive(Clone, Copy)]
pub struct Variables {
    floats: [i32; 4],
    roles: [u8; 2],
    booleans: u8,
}
impl Variables {
    fn visit(
        programs: [Program<'_>; 2],
        component: usize,
        mut f: impl FnMut(usize, u8, usize) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if component > 1 {
            return Err(Error::Program);
        }
        let mut counts = [0usize; 3];
        for (c, p) in programs.into_iter().enumerate() {
            for i in 0..p.variable_count() {
                let kind = p.variable_kind(i).ok_or(Error::Program)?;
                let slot = kind.checked_sub(1).ok_or(Error::Program)? as usize;
                if slot >= 3 {
                    return Err(Error::Program);
                }
                let index = counts[slot];
                counts[slot] += 1;
                if counts[0] > 5 || counts[1] > 4 || counts[2] > 2 {
                    return Err(Error::Program);
                }
                if c == component {
                    f(i as usize, kind, index)?;
                }
            }
        }
        Ok(())
    }
    pub fn initial(programs: [Program<'_>; 2]) -> Result<Self, Error> {
        let mut store = Self {
            floats: [0; 4],
            roles: [0; 2],
            booleans: 0,
        };
        for component in 0..2 {
            let mut values = [0; MAX_VARIABLES];
            for i in 0..programs[component].variable_count() {
                values[i as usize] = programs[component].initial(i).ok_or(Error::Program)?;
            }
            store.save(
                programs,
                component,
                &values[..programs[component].variable_count() as usize],
            )?;
        }
        Ok(store)
    }
    pub fn load(
        &self,
        programs: [Program<'_>; 2],
        component: usize,
        values: &mut [i32],
    ) -> Result<(), Error> {
        if component > 1 || values.len() != programs[component].variable_count() as usize {
            return Err(Error::Program);
        }
        Self::visit(programs, component, |i, kind, index| {
            values[i] = match kind {
                1 => ((self.booleans >> index) & 1) as i32,
                2 => self.floats[index],
                3 => self.roles[index] as i32,
                _ => return Err(Error::Program),
            };
            Ok(())
        })
    }
    pub fn save(
        &mut self,
        programs: [Program<'_>; 2],
        component: usize,
        values: &[i32],
    ) -> Result<(), Error> {
        if component > 1 || values.len() != programs[component].variable_count() as usize {
            return Err(Error::Program);
        }
        let mut next = *self;
        Self::visit(programs, component, |i, kind, index| {
            let value = values[i];
            match kind {
                1 => {
                    if value != 0 && value != 1 {
                        return Err(Error::Parameter);
                    }
                    next.booleans = (next.booleans & !(1 << index)) | ((value as u8) << index);
                }
                2 => next.floats[index] = value,
                3 => {
                    if !(0..=3).contains(&value) {
                        return Err(Error::Parameter);
                    }
                    next.roles[index] = value as u8;
                }
                _ => return Err(Error::Program),
            }
            Ok(())
        })?;
        *self = next;
        Ok(())
    }
}
const _: () = assert!(core::mem::size_of::<Variables>() == 20);
#[derive(Clone, Copy)]
pub struct Pair {
    variables: Variables,
    cursors: [Cursor; 2],
    rng: u32,
}
impl Pair {
    pub fn initial(programs: [Program<'_>; 2], seed: u32) -> Result<Self, Error> {
        Ok(Self {
            variables: Variables::initial(programs)?,
            cursors: programs.map(|p| Cursor::initial(&p)),
            rng: seed,
        })
    }
    pub fn variable(
        &self,
        programs: [Program<'_>; 2],
        component: usize,
        index: usize,
    ) -> Result<i32, Error> {
        if component > 1 || index >= programs[component].variable_count() as usize {
            return Err(Error::Parameter);
        }
        let mut values = [0; MAX_VARIABLES];
        let values = &mut values[..programs[component].variable_count() as usize];
        self.variables.load(programs, component, values)?;
        Ok(values[index])
    }
    /// External original component messages may change a scalar without
    /// resetting either graph or touching the other variable namespace.
    pub fn set_variable(
        &mut self,
        programs: [Program<'_>; 2],
        component: usize,
        index: usize,
        value: i32,
    ) -> Result<(), Error> {
        if component > 1 || index >= programs[component].variable_count() as usize {
            return Err(Error::Parameter);
        }
        let mut values = [0; MAX_VARIABLES];
        let values = &mut values[..programs[component].variable_count() as usize];
        self.variables.load(programs, component, values)?;
        values[index] = value;
        self.variables.save(programs, component, values)
    }
    pub fn die(&mut self) {
        for cursor in &mut self.cursors {
            cursor.die();
        }
    }
    pub fn reset(&mut self, programs: [Program<'_>; 2], seed: u32) -> Result<(), Error> {
        *self = Self::initial(programs, seed)?;
        Ok(())
    }
    pub fn state(&self, component: usize) -> Option<u16> {
        self.cursors.get(component).map(|c| c.state())
    }
    pub fn step(
        &mut self,
        programs: [Program<'_>; 2],
        component: usize,
        phase: Phase,
        kernel: &mut impl Kernel,
    ) -> Result<(), Error> {
        if component > 1 {
            return Err(Error::Program);
        }
        let p = &programs[component];
        let mut values = [0; MAX_VARIABLES];
        let values = &mut values[..p.variable_count() as usize];
        self.variables.load(programs, component, values)?;
        let result = self.cursors[component].step(p, values, &mut self.rng, phase, kernel);
        self.variables.save(programs, component, values)?;
        result
    }
    pub fn event(
        &mut self,
        programs: [Program<'_>; 2],
        component: usize,
        event: u16,
        kernel: &mut impl Kernel,
    ) -> Result<(), Error> {
        if component > 1 {
            return Err(Error::Program);
        }
        let p = &programs[component];
        let mut values = [0; MAX_VARIABLES];
        let values = &mut values[..p.variable_count() as usize];
        self.variables.load(programs, component, values)?;
        let result = self.cursors[component].event(p, values, &mut self.rng, event, kernel);
        self.variables.save(programs, component, values)?;
        result
    }
}
const _: () = assert!(core::mem::size_of::<Pair>() == 36);

/// Persistent actor payload; sensory queries, rendering and transport remain
/// borrowed game services. Component enablement uses the original ordered bits.
#[derive(Clone, Copy)]
pub struct State {
    pub pair: Pair,
    pub buzz: crate::buzz::IdleBuzz,
    pub velocity: [i32; 2],
    pub queued_force: [i32; 2],
    pub audio_pitch: i32,
    pub animation_tick: u32,
    pub face_pause: u16,
    pub clip: u8,
    pub flags: u8,
}
impl State {
    pub fn initial(
        programs: [Program<'_>; 2],
        position: [i32; 2],
        seed: u32,
        enabled: u8,
        audio_pitch: i32,
    ) -> Result<Self, Error> {
        if enabled & !3 != 0 || audio_pitch <= 0 {
            return Err(Error::Parameter);
        }
        Ok(Self {
            pair: Pair::initial(programs, seed)?,
            buzz: crate::buzz::IdleBuzz::new(position),
            velocity: [0; 2],
            queued_force: [0; 2],
            audio_pitch,
            animation_tick: 0,
            face_pause: 0,
            clip: 0,
            flags: enabled,
        })
    }
    /// The admitted bodies have mass 1 and no gravity or linear damping.
    /// Forces remain queued until the shared original fixed-physics boundary.
    pub fn integrate_queued_force(&mut self) {
        for axis in 0..2 {
            let impulse = (self.queued_force[axis] as i64
                * crate::fsm_chase::FixedClock::FIXED_UNITS as i64)
                / crate::fsm_chase::FixedClock::SOURCE_RATE as i64;
            self.velocity[axis] = self.velocity[axis].saturating_add(impulse as i32);
            self.queued_force[axis] = 0;
        }
    }
    pub fn queue_force_and_clamp(
        &mut self,
        force: [i32; 2],
        limits: [i32; 3],
    ) -> Result<(), Error> {
        if limits.iter().any(|v| *v < 0) {
            return Err(Error::Parameter);
        }
        let mut queued = self.queued_force;
        let mut velocity = self.velocity;
        for axis in 0..2 {
            queued[axis] = queued[axis]
                .checked_add(force[axis])
                .ok_or(Error::Parameter)?;
            velocity[axis] = velocity[axis].clamp(-limits[axis], limits[axis]);
        }
        crate::buzz::clamp(&mut velocity, limits[2]);
        self.queued_force = queued;
        self.velocity = velocity;
        Ok(())
    }
    pub fn decelerate(&mut self, amount: i32) -> Result<(), Error> {
        if amount < 0 {
            return Err(Error::Parameter);
        }
        for axis in &mut self.velocity {
            *axis = axis.signum() * axis.saturating_abs().saturating_sub(amount).max(0);
        }
        Ok(())
    }
    pub fn enabled(&self, component: usize) -> bool {
        component < 2 && self.flags & (1 << component) != 0
    }
    pub fn die(&mut self) {
        self.pair.die();
        self.velocity = [0; 2];
        self.queued_force = [0; 2];
        self.flags &= 3;
    }
}
const _: () = assert!(core::mem::size_of::<State>() == 84);
