//! Shared PlayMaker action kernels. Graphs supply operands and control flow.
use hk_format::fsm::{Action, Operand, Program, MAX_VARIABLES, NO_EVENT, UPDATE};
use crate::fsm::{Error, Kernel, Phase, ResultAction};

pub use hk_format::fsm::opcode::*;

pub trait Environment {
    fn position(&self, object:i32)->Result<[i32;3],Error>;
    fn face_direction(&mut self, object:i32, sprite_faces_right:bool)->Result<(),Error>;
    fn velocity_as_angle(&mut self, object:i32, angle:i32, speed:i32)->Result<(),Error>;
    /// Up, right, down, left bits, filtered by the authored collision category.
    fn contacts(&self, other_layer:bool, enter:bool, ignore_triggers:bool)->u8;
    fn elapsed_ticks(&self,_real_time:bool)->Result<u16,Error>{Err(Error::UnsupportedAction)}
    fn translate(&mut self,_object:i32,_delta:[Option<i32>;3],_local:bool,_per_second:bool)->Result<(),Error>{Err(Error::UnsupportedAction)}
    fn walk(&mut self,_speed:i32,_delay:i32,_left:bool,_right:bool,_keep:bool,_phase:Phase)->Result<(),Error>{Err(Error::UnsupportedAction)}
}
pub struct Actions<E>(pub E);
/// A graph-admitted walking action closure. It cannot reference flying kernels,
/// so a Crawler-only room never acquires the Gruzzer bank or adapter.
pub struct WalkingActions<E>(pub E);
/// Flying graphs retain only their own kernels, including the shared BoolTest.
pub struct FlyingActions<E>(pub E);
impl<E: Environment> Kernel for FlyingActions<E> {
    #[inline(always)]
    fn execute(&mut self, p: &Program<'_>, a: Action, phase: Phase,
        v: &mut [i32; MAX_VARIABLES], rng: &mut u32) -> Result<ResultAction, Error> {
        crate::fsm_fly::execute(&mut self.0, p, a, phase, v, rng)
    }
}
impl<E: Environment> Kernel for WalkingActions<E> {
    #[inline(always)]
    fn execute(&mut self, p: &Program<'_>, a: Action, phase: Phase,
        v: &mut [i32; MAX_VARIABLES], _: &mut u32) -> Result<ResultAction, Error> {
        crate::fsm_walk::execute(&mut self.0, p, a, phase, v)
    }
}
#[inline(always)]
pub(crate) fn operand(p:&Program<'_>,a:Action,index:u8)->Result<Operand,Error> {
    if index>=a.operand_count {return Err(Error::Parameter);}
    p.operand(a.operands+index as u16).ok_or(Error::Parameter)
}
#[inline(always)]
pub(crate) fn read(p:&Program<'_>,o:Operand,v:&[i32;MAX_VARIABLES])->Result<i32,Error> {
    if o.kind==0 {p.constant(o.index).ok_or(Error::Parameter)}
    else if o.kind==1 {v.get(o.index as usize).copied().ok_or(Error::Parameter)}
    else if o.kind==3 {Ok(o.index as i32)} else {Err(Error::Parameter)}
}
#[inline(always)]
pub(crate) fn write(p:&Program<'_>,o:Operand,v:&mut[i32;MAX_VARIABLES],value:i32)->Result<(),Error> {
    if o.kind!=1 {return Err(Error::Parameter);}
    let kind=p.variable_kind(o.index).ok_or(Error::Parameter)?;
    if (kind==1 && value!=0 && value!=1)||(kind==3 && !(0..=3).contains(&value)) {return Err(Error::Parameter);}
    *v.get_mut(o.index as usize).ok_or(Error::Parameter)?=value;Ok(())
}
#[inline(always)]
pub(crate) fn event(o:Operand)->Result<u16,Error> {if o.kind==2 {Ok(o.index)}else{Err(Error::Parameter)}}
#[inline(always)]
pub(crate) fn boolean(p:&Program<'_>,o:Operand,v:&[i32;MAX_VARIABLES])->Result<bool,Error>{match read(p,o,v)?{0=>Ok(false),1=>Ok(true),_=>Err(Error::Parameter)}}
#[inline(always)]
pub(crate) fn value(p:&Program<'_>,a:Action,index:u8,v:&[i32;MAX_VARIABLES])->Result<i32,Error>{read(p,operand(p,a,index)?,v)}
#[inline(always)]
pub(crate) fn flag(p:&Program<'_>,a:Action,index:u8,v:&[i32;MAX_VARIABLES])->Result<bool,Error>{boolean(p,operand(p,a,index)?,v)}
impl<E:Environment> Kernel for Actions<E> {
    #[inline(always)]
    fn execute(&mut self,p:&Program<'_>,a:Action,phase:Phase,v:&mut[i32;MAX_VARIABLES],rng:&mut u32)->Result<ResultAction,Error> {
        execute(&mut self.0, p, a, phase, v, rng)
    }
}
#[inline(always)]
fn execute<E:Environment>(env:&mut E,p:&Program<'_>,a:Action,phase:Phase,v:&mut[i32;MAX_VARIABLES],rng:&mut u32)->Result<ResultAction,Error> {
    if a.opcode==WAIT || a.opcode==TRANSLATE || a.opcode==WALK_LEFT_RIGHT || a.opcode==BOOL_TEST {
        crate::fsm_walk::execute(env,p,a,phase,v)
    } else {
        crate::fsm_fly::execute(env,p,a,phase,v,rng)
    }
}
