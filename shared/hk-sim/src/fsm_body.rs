//! Concrete physics adapters share one production FSM specialization.
//! No trait object or indirect call enters the guest action path.
use crate::{ONE,WalkState,WalkParams,fsm::{Error,Phase},fsm_actions::Environment};

pub struct Flying {
    pub camera:[i32;3],pub position:[i32;3],pub angle:i32,
    pub speed:i32,pub contacts:u8,pub facing:i8,
}
pub struct Walking {
    pub walk:WalkState,pub position:[i32;3],pub params:WalkParams,
    pub senses:(bool,bool,bool),pub velocity:i32,pub real_ticks:u16,pub stationary_fixture:bool,
}
pub enum Body { Flying(Flying),Walking(Walking) }
impl Environment for Flying {
    fn position(&self,object:i32)->Result<[i32;3],Error>{
        match object{1=>Ok(self.position),2=>Ok(self.camera),_=>Err(Error::Parameter)}
    }
    fn face_direction(&mut self,object:i32,right:bool)->Result<(),Error>{
        if object!=1||right{return Err(Error::Parameter)}
        let angle=self.angle.rem_euclid(360*ONE);
        self.facing=if self.speed!=0&&!(90*ONE..=270*ONE).contains(&angle){1}else{-1};Ok(())
    }
    fn velocity_as_angle(&mut self,object:i32,angle:i32,speed:i32)->Result<(),Error>{
        if object!=1{return Err(Error::Parameter)}self.angle=angle;self.speed=speed;Ok(())
    }
    fn contacts(&self,_:bool,_:bool,_:bool)->u8{self.contacts}
}
// This concrete adapter is intentionally independent of Body::Flying. A cold
// Crawler image must not retain flying methods through tagged dispatch.
impl Environment for Walking {
    fn position(&self, object:i32)->Result<[i32;3],Error>{
        if object==1 {Ok(self.position)} else {Err(Error::Parameter)}
    }
    fn face_direction(&mut self,_:i32,_:bool)->Result<(),Error>{Err(Error::UnsupportedAction)}
    fn velocity_as_angle(&mut self,_:i32,_:i32,_:i32)->Result<(),Error>{Err(Error::UnsupportedAction)}
    fn contacts(&self,_:bool,_:bool,_:bool)->u8{0}
    fn elapsed_ticks(&self,real:bool)->Result<u16,Error>{Ok(if real{self.real_ticks}else{1})}
    fn translate(&mut self,object:i32,delta:[Option<i32>;3],local:bool,per_second:bool)->Result<(),Error>{
        if object!=1||delta[2].unwrap_or(0)!=0{return Err(Error::Parameter)}
        let mut dx=delta[0].unwrap_or(0);let mut dy=delta[1].unwrap_or(0);
        if local{dx=dx.checked_mul(-self.walk.direction).ok_or(Error::Parameter)?}
        if per_second{dx/=60;dy/=60}
        self.position[0]=self.position[0].checked_add(dx).ok_or(Error::Parameter)?;
        self.position[1]=self.position[1].checked_add(dy).ok_or(Error::Parameter)?;Ok(())
    }
    fn walk(&mut self,speed:i32,delay:i32,left:bool,right:bool,keep:bool,phase:Phase)->Result<(),Error>{
        if speed!=self.params.speed&&!self.stationary_fixture||delay<0||left&&right{return Err(Error::Parameter)}
        if phase==Phase::Enter{
            if !keep&&(left||right){self.walk=WalkState::new(if left{-1}else{1},false)}
        }else if phase==Phase::Update{
            let mut p=self.params;p.turn_cooldown_ticks=u16::try_from((delay as i64*60+65535)/65536).map_err(|_|Error::Parameter)?;
            self.velocity=self.walk.tick(p,self.senses.0,self.senses.1,self.senses.2);
        }
        Ok(())
    }
}
impl Environment for Body {
    fn position(&self,object:i32)->Result<[i32;3],Error>{
        match self{
            Self::Flying(b)=>match object{1=>Ok(b.position),2=>Ok(b.camera),_=>Err(Error::Parameter)},
            Self::Walking(b)=>if object==1{Ok(b.position)}else{Err(Error::Parameter)},
        }
    }
    fn face_direction(&mut self,object:i32,right:bool)->Result<(),Error>{
        let Self::Flying(b)=self else{return Err(Error::UnsupportedAction)};
        if object!=1||right{return Err(Error::Parameter)}
        let angle=b.angle.rem_euclid(360*ONE);
        b.facing=if b.speed!=0&&!(90*ONE..=270*ONE).contains(&angle){1}else{-1};Ok(())
    }
    fn velocity_as_angle(&mut self,object:i32,angle:i32,speed:i32)->Result<(),Error>{
        let Self::Flying(b)=self else{return Err(Error::UnsupportedAction)};
        if object!=1{return Err(Error::Parameter)}b.angle=angle;b.speed=speed;Ok(())
    }
    fn contacts(&self,_:bool,_:bool,_:bool)->u8{match self{Self::Flying(b)=>b.contacts,Self::Walking(_)=>0}}
    fn elapsed_ticks(&self,real:bool)->Result<u16,Error>{
        let Self::Walking(b)=self else{return Err(Error::UnsupportedAction)};
        Ok(if real{b.real_ticks}else{1})
    }
    fn translate(&mut self,object:i32,delta:[Option<i32>;3],local:bool,per_second:bool)->Result<(),Error>{
        let Self::Walking(b)=self else{return Err(Error::UnsupportedAction)};
        if object!=1||delta[2].unwrap_or(0)!=0{return Err(Error::Parameter)}
        let mut dx=delta[0].unwrap_or(0);let mut dy=delta[1].unwrap_or(0);
        if local{dx=dx.checked_mul(-b.walk.direction).ok_or(Error::Parameter)?}
        if per_second{dx/=60;dy/=60}
        b.position[0]=b.position[0].checked_add(dx).ok_or(Error::Parameter)?;
        b.position[1]=b.position[1].checked_add(dy).ok_or(Error::Parameter)?;Ok(())
    }
    fn walk(&mut self,speed:i32,delay:i32,left:bool,right:bool,keep:bool,phase:Phase)->Result<(),Error>{
        let Self::Walking(b)=self else{return Err(Error::UnsupportedAction)};
        if speed!=b.params.speed&&!b.stationary_fixture||delay<0||left&&right{return Err(Error::Parameter)}
        if phase==Phase::Enter{
            if !keep&&(left||right){b.walk=WalkState::new(if left{-1}else{1},false)}
        }else if phase==Phase::Update{
            let mut p=b.params;p.turn_cooldown_ticks=u16::try_from((delay as i64*60+65535)/65536).map_err(|_|Error::Parameter)?;
            b.velocity=b.walk.tick(p,b.senses.0,b.senses.1,b.senses.2);
        }
        Ok(())
    }
}
