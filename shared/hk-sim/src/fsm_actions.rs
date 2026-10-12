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
}
pub struct Actions<E>(pub E);
fn operand(p:Program<'_>,a:Action,index:u8)->Result<Operand,Error> {
    if index>=a.operand_count {return Err(Error::Parameter);}
    p.operand(a.operands+index as u16).ok_or(Error::Parameter)
}
fn read(p:Program<'_>,o:Operand,v:&[i32;MAX_VARIABLES])->Result<i32,Error> {
    if o.kind==0 {p.constant(o.index).ok_or(Error::Parameter)}
    else if o.kind==1 {v.get(o.index as usize).copied().ok_or(Error::Parameter)}
    else if o.kind==3 {Ok(o.index as i32)} else {Err(Error::Parameter)}
}
fn write(p:Program<'_>,o:Operand,v:&mut[i32;MAX_VARIABLES],value:i32)->Result<(),Error> {
    if o.kind!=1 {return Err(Error::Parameter);}
    let kind=p.variable_kind(o.index).ok_or(Error::Parameter)?;
    if (kind==1 && value!=0 && value!=1)||(kind==3 && !(0..=3).contains(&value)) {return Err(Error::Parameter);}
    *v.get_mut(o.index as usize).ok_or(Error::Parameter)?=value;Ok(())
}
fn event(o:Operand)->Result<u16,Error> {if o.kind==2 {Ok(o.index)}else{Err(Error::Parameter)}}
impl<E:Environment> Kernel for Actions<E> {
    #[inline(always)]
    fn execute(&mut self,p:Program<'_>,a:Action,phase:Phase,v:&mut[i32;MAX_VARIABLES],rng:&mut u32)->Result<ResultAction,Error> {
        let op=a.opcode;
        let collision=op==COLLISION_STAY||op==COLLISION_ENTER;
        if collision {
            if (op==COLLISION_STAY&&phase!=Phase::CollisionStay)||(op==COLLISION_ENTER&&phase!=Phase::CollisionEnter) {
                return Ok(ResultAction::running());
            }
        } else if phase==Phase::CollisionStay||phase==Phase::CollisionEnter {return Ok(ResultAction::running());}
        let mut result=ResultAction{finished:a.flags&UPDATE==0,event:NO_EVENT};
        if op==GET_OWNER {
            write(p,operand(p,a,0)?,v,1)?;
        } else if op==FIND_OBJECT {
            let role=read(p,operand(p,a,0)?,v)?;
            if !(1..=3).contains(&role){return Err(Error::Parameter);}
            self.0.position(role)?;
            write(p,operand(p,a,1)?,v,role)?;
        } else if op==SET_BOOL||op==SET_FLOAT {
            let value=read(p,operand(p,a,1)?,v)?;
            if op==SET_BOOL&&value!=0&&value!=1{return Err(Error::Parameter);}
            write(p,operand(p,a,0)?,v,value)?;
        } else if op==BOOL_TEST {
            let value=read(p,operand(p,a,0)?,v)?;
            if value!=0&&value!=1{return Err(Error::Parameter);}
            result.event=event(operand(p,a,if value!=0 {1}else{2})?)?;
        } else if op==RANDOM_FLOAT {
            let low=read(p,operand(p,a,0)?,v)?;let high=read(p,operand(p,a,1)?,v)?;
            if high<low{return Err(Error::Parameter);}
            *rng=rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let value=low as i64+(((*rng>>8)as i64*(high as i64-low as i64))>>24);
            write(p,operand(p,a,2)?,v,i32::try_from(value).map_err(|_|Error::Parameter)?)?;
        } else if op==GET_DISTANCE {
            let from=self.0.position(read(p,operand(p,a,0)?,v)?)?;
            let to=self.0.position(read(p,operand(p,a,1)?,v)?)?;
            let mut squared=0u64;
            for i in 0..3 {
                let delta=(from[i]as i64-to[i]as i64).unsigned_abs();
                squared=squared.checked_add(delta.checked_mul(delta).ok_or(Error::Parameter)?).ok_or(Error::Parameter)?;
            }
            write(p,operand(p,a,2)?,v,i32::try_from(psx_math::int32::isqrt_u64(squared)).map_err(|_|Error::Parameter)?)?;
        } else if op==FLOAT_COMPARE {
            let x=read(p,operand(p,a,0)?,v)?;let y=read(p,operand(p,a,1)?,v)?;
            let tolerance=read(p,operand(p,a,2)?,v)?;
            if tolerance<0{return Err(Error::Parameter);}
            let choice=if (x as i64-y as i64).abs()<=tolerance as i64{3}else if x<y{4}else{5};
            result.event=event(operand(p,a,choice)?)?;
        } else if op==FLOAT_SWITCH {
            if a.operand_count<3||a.operand_count%2!=1{return Err(Error::Parameter);}
            let value=read(p,operand(p,a,0)?,v)?;
            let mut index=1;
            while index<a.operand_count {
                if value<read(p,operand(p,a,index)?,v)? {result.event=event(operand(p,a,index+1)?)?;break;}
                index+=2;
            }
        } else if op==FACE_DIRECTION {
            let right=read(p,operand(p,a,1)?,v)?;
            if right!=0&&right!=1{return Err(Error::Parameter);}
            self.0.face_direction(read(p,operand(p,a,0)?,v)?,right!=0)?;
        } else if op==SET_VELOCITY_ANGLE {
            self.0.velocity_as_angle(read(p,operand(p,a,0)?,v)?,read(p,operand(p,a,1)?,v)?,read(p,operand(p,a,2)?,v)?)?;
        } else if collision {
            let other=read(p,operand(p,a,0)?,v)?;let ignore=read(p,operand(p,a,1)?,v)?;
            if ![0,1].contains(&other)||![0,1].contains(&ignore){return Err(Error::Parameter);}
            let bits=self.0.contacts(other!=0,op==COLLISION_ENTER,ignore!=0);
            for side in 0..4 {if bits&(1<<side)!=0 {result.event=event(operand(p,a,side+2)?)?;break;}}
            result.finished=false;
        } else {return Err(Error::UnsupportedAction);}
        Ok(result)
    }
}
