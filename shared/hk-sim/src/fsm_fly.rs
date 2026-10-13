//! Gruzzer-only action kernels, loaded only for graphs that require them.
use crate::fsm::{Error, Phase, ResultAction};
use crate::fsm_actions::{event, flag, operand, read, write, Environment};
use hk_format::fsm::{opcode::*, Action, Program, MAX_VARIABLES, NO_EVENT, UPDATE};
// One action dispatcher for every enemy adapter. Borrowing the table view
// avoids copying its offsets and counts at each operand operation.
#[inline(never)]
pub(crate) fn execute<E:Environment>(env:&mut E,p:&Program<'_>,a:Action,phase:Phase,v:&mut[i32;MAX_VARIABLES],rng:&mut u32)->Result<ResultAction,Error> {
        let op=a.opcode;
        let collision=op==COLLISION_STAY||op==COLLISION_ENTER;
        if collision {
            if (op==COLLISION_STAY&&phase!=Phase::CollisionStay)||(op==COLLISION_ENTER&&phase!=Phase::CollisionEnter) {
                return Ok(ResultAction::running());
            }
        } else if phase==Phase::CollisionStay||phase==Phase::CollisionEnter {return Ok(ResultAction::running());}
        let mut result=ResultAction{finished:a.flags&UPDATE==0,event:NO_EVENT};
        if op==BOOL_TEST {
            let value=flag(p,a,0,v)?;
            result.event=event(operand(p,a,if value{1}else{2})?)?;
        } else if op==GET_OWNER {
            write(p,operand(p,a,0)?,v,1)?;
        } else if op==FIND_OBJECT {
            let role=read(p,operand(p,a,0)?,v)?;
            if !(1..=3).contains(&role){return Err(Error::Parameter);}
            env.position(role)?;
            write(p,operand(p,a,1)?,v,role)?;
        } else if op==SET_BOOL||op==SET_FLOAT {
            let value=read(p,operand(p,a,1)?,v)?;
            if op==SET_BOOL&&value!=0&&value!=1{return Err(Error::Parameter);}
            write(p,operand(p,a,0)?,v,value)?;
        } else if op==RANDOM_FLOAT {
            let low=read(p,operand(p,a,0)?,v)?;let high=read(p,operand(p,a,1)?,v)?;
            if high<low{return Err(Error::Parameter);}
            *rng=rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let value=low as i64+(((*rng>>8)as i64*(high as i64-low as i64))>>24);
            write(p,operand(p,a,2)?,v,i32::try_from(value).map_err(|_|Error::Parameter)?)?;
        } else if op==GET_DISTANCE {
            let from=env.position(read(p,operand(p,a,0)?,v)?)?;
            let to=env.position(read(p,operand(p,a,1)?,v)?)?;
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
            env.face_direction(read(p,operand(p,a,0)?,v)?,right!=0)?;
        } else if op==SET_VELOCITY_ANGLE {
            env.velocity_as_angle(read(p,operand(p,a,0)?,v)?,read(p,operand(p,a,1)?,v)?,read(p,operand(p,a,2)?,v)?)?;
        } else if collision {
            let other=read(p,operand(p,a,0)?,v)?;let ignore=read(p,operand(p,a,1)?,v)?;
            if ![0,1].contains(&other)||![0,1].contains(&ignore){return Err(Error::Parameter);}
            let bits=env.contacts(other!=0,op==COLLISION_ENTER,ignore!=0);
            for side in 0..4 {if bits&(1<<side)!=0 {result.event=event(operand(p,a,side+2)?)?;break;}}
            result.finished=false;
        } else {return Err(Error::UnsupportedAction);}
        Ok(result)
}
