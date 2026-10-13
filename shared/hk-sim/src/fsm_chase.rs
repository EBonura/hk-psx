//! Vengefly action closure under construction. Unknown actions stay refused;
//! these scalar kernels do not imply admission of either complete graph.
use crate::{fsm::{Error,Kernel,Phase,ResultAction},fsm_actions::{operand,read,write,event,boolean}};
use hk_format::fsm::{opcode::*,Action,Program,NO_EVENT,UPDATE};

pub trait Environment { fn tick(&self)->u32; }
pub struct Actions<E>(pub E);
impl<E:Environment> Kernel for Actions<E> {
    fn execute(&mut self,p:&Program<'_>,a:Action,phase:Phase,v:&mut[i32],_:&mut u32)->Result<ResultAction,Error> {
        if matches!(phase,Phase::CollisionEnter|Phase::CollisionStay){return Ok(ResultAction::running());}
        let mut result=ResultAction{finished:a.flags&UPDATE==0,event:NO_EVENT};
        match a.opcode {
            FLOAT_CLAMP=>{
                if a.operand_count!=3{return Err(Error::Parameter);}
                let target=operand(p,a,0)?;
                let low=read(p,operand(p,a,1)?,v)?;let high=read(p,operand(p,a,2)?,v)?;
                if low>high{return Err(Error::Parameter);}
                let value=read(p,target,v)?.clamp(low,high);
                write(p,target,v,value)?;
            }
            BOOL_ALL_TRUE=>{
                if a.operand_count<3{return Err(Error::Parameter);}
                let mut all=true;
                for i in 0..a.operand_count-2{all &= boolean(p,operand(p,a,i)?,v)?;}
                let output=operand(p,a,a.operand_count-2)?;
                if output.kind!=4{write(p,output,v,all as i32)?;}
                if all{result.event=event(operand(p,a,a.operand_count-1)?)?;}
            }
            NEXT_FRAME_EVENT=>{
                if a.operand_count!=2{return Err(Error::Parameter);}
                let timer=operand(p,a,0)?;
                let now=self.0.tick();
                result.finished=false;
                if phase==Phase::Enter{write(p,timer,v,now as i32)?;}
                else if phase==Phase::Update && now.wrapping_sub(read(p,timer,v)? as u32)!=0 {
                    result.finished=true;result.event=event(operand(p,a,1)?)?;
                }
            }
            _=>return Err(Error::UnsupportedAction),
        }
        Ok(result)
    }
}
