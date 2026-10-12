//! Original cooked `Bouncer Control` graph through the shared action runtime.
//! The caller retains camera activation, physics, recoil, damage and corpses.
//! Port RNG remains deterministic LCG rather than Unity's global RNG.
use crate::{ONE,fsm::{Machine,Phase as StepPhase,Error},fsm_actions::{Actions,Environment}};
use hk_format::fsm::Program;
static BANK:[u8;include_bytes!("../../../data/gruzzer.fsm").len()]=*include_bytes!("../../../data/gruzzer.fsm");
pub const SPEED:i32=340787;
pub const CAMERA_RANGE:i32=44*ONE;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Phase {Waiting,Flying,Dead}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Side {Up,Right,Down,Left}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Gruzzer { machine:Machine, facing:i8, dead:bool }
// Replaces the old controller. It fits the existing 88-byte widest Runtime
// payload; the guest's unchanged 92-byte enum assertion remains authoritative.
const _:()=assert!(core::mem::size_of::<Gruzzer>()<=80);
struct Body { camera:[i32;3],position:[i32;3],angle:i32,speed:i32,contacts:u8,facing:i8 }
impl Environment for Body {
    fn position(&self,object:i32)->Result<[i32;3],Error>{match object{1=>Ok(self.position),2=>Ok(self.camera),_=>Err(Error::Parameter)}}
    fn face_direction(&mut self,object:i32,right:bool)->Result<(),Error>{
        if object!=1||right{return Err(Error::Parameter);}
        // The preceding physics velocity is the current angle/speed. This
        // action precedes SetVelocityAsAngle in the original Fly state.
        let angle=self.angle.rem_euclid(360*ONE);
        self.facing=if self.speed!=0&&!(90*ONE..=270*ONE).contains(&angle){1}else{-1};Ok(())
    }
    fn velocity_as_angle(&mut self,object:i32,angle:i32,speed:i32)->Result<(),Error>{
        if object!=1{return Err(Error::Parameter);}self.angle=angle;self.speed=speed;Ok(())
    }
    fn contacts(&self,_:bool,_:bool,_:bool)->u8{self.contacts}
}
impl Gruzzer {
    pub fn new(seed:u32)->Self {
        let p=Program::parse(&BANK).expect("checked original Gruzzer graph");
        Self{machine:Machine::new(p,seed),facing:-1,dead:false}
    }
    pub fn phase(self)->Phase {
        if self.dead {Phase::Dead}else if matches!(self.machine.state(),0|17){Phase::Waiting}else{Phase::Flying}
    }
    pub fn angle(self)->i32{self.machine.variables[4]}
    pub fn facing(self)->i32{self.facing as i32}
    pub fn die(&mut self){self.dead=true;self.machine.die();}
    fn step(&mut self,camera:[i32;3],position:[i32;3],contacts:u8,phase:StepPhase){
        if self.dead{return;}
        // Every instance constructor checked this same immutable included bank.
        let p=unsafe{Program::from_verified(&BANK)};
        let mut k=Actions(Body{camera,position,angle:self.angle(),speed:if self.phase()==Phase::Flying{SPEED}else{0},contacts,facing:self.facing});
        if self.machine.step(p,phase,&mut k).is_err(){self.die();return;}
        self.facing=k.0.facing;
    }
    pub fn tick(&mut self,camera:[i32;3],position:[i32;3]){self.step(camera,position,0,StepPhase::Update);}
    pub fn bonk(&mut self,side:Side){
        if self.phase()!=Phase::Flying{return;}
        let bit=match side{Side::Up=>1,Side::Right=>2,Side::Down=>4,Side::Left=>8};
        self.step([0;3],[0;3],bit,StepPhase::CollisionStay);
    }
}
