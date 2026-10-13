use hk_format::fsm::{Action,Program,opcode::*,ENABLED,UPDATE,NO_EVENT};
use hk_sim::{fsm::{Kernel,Phase,Error},fsm_chase::{Actions,Environment}};
struct Clock(u32);
impl Environment for Clock {fn tick(&self)->u32{self.0}}
fn graph(opcode:u16,operands:&[(u8,u16)],variables:&[(i32,u16)],constants:&[i32])->Vec<u8>{
    let mut bytes=Vec::from(*hk_format::fsm::MAGIC);
    for n in [1u16,1,0,operands.len() as u16,variables.len() as u16,constants.len() as u16,2,0,0,0]{bytes.extend(n.to_le_bytes());}
    bytes.extend(0u32.to_le_bytes());
    for n in [0u16,1,0,0]{bytes.extend(n.to_le_bytes());}
    bytes.extend(opcode.to_le_bytes());bytes.extend(0u16.to_le_bytes());bytes.extend([operands.len() as u8,ENABLED|UPDATE,0,0]);
    for &(kind,index) in operands{bytes.extend([kind,0]);bytes.extend(index.to_le_bytes());}
    for &(value,kind) in variables{bytes.extend(value.to_le_bytes());bytes.extend(kind.to_le_bytes());bytes.extend(0u16.to_le_bytes());}
    for value in constants{bytes.extend(value.to_le_bytes());}
    let length=bytes.len() as u32;bytes[28..32].copy_from_slice(&length.to_le_bytes());bytes
}
fn action(p:&Program<'_>)->Action{p.action(0).unwrap()}
#[test]
fn float_clamp_keeps_signed_bounds_and_rejects_reversed_range(){
    let bytes=graph(FLOAT_CLAMP,&[(1,0),(0,0),(0,1)],&[(0,2)],&[-2*65536,3*65536]);
    let p=Program::parse(&bytes).unwrap();let mut kernel=Actions(Clock(0));let mut rng=0;
    for (input,expected) in [(-4*65536,-2*65536),(-2*65536,-2*65536),(0,0),(3*65536,3*65536),(7*65536,3*65536)]{
        let mut vars=[input];kernel.execute(&p,action(&p),Phase::Update,&mut vars,&mut rng).unwrap();assert_eq!(vars[0],expected);
    }
    let bytes=graph(FLOAT_CLAMP,&[(1,0),(0,1),(0,0)],&[(0,2)],&[-2*65536,3*65536]);let p=Program::parse(&bytes).unwrap();
    assert_eq!(kernel.execute(&p,action(&p),Phase::Update,&mut[0],&mut rng),Err(Error::Parameter));
}
#[test]
fn bool_all_true_only_emits_after_both_conditions_and_validates_each(){
    let bytes=graph(BOOL_ALL_TRUE,&[(1,0),(1,1),(1,2),(2,1)],&[(0,1),(0,1),(0,1)],&[]);
    let p=Program::parse(&bytes).unwrap();let mut kernel=Actions(Clock(0));let mut rng=0;
    for (a,b,event,stored) in [(0,0,NO_EVENT,0),(1,0,NO_EVENT,0),(0,1,NO_EVENT,0),(1,1,1,1)]{
        let mut vars=[a,b,0];let result=kernel.execute(&p,action(&p),Phase::Update,&mut vars,&mut rng).unwrap();assert_eq!(result.event,event);assert_eq!(vars[2],stored);
    }
    // A false first condition must not disguise an invalid later boolean.
    assert_eq!(kernel.execute(&p,action(&p),Phase::Update,&mut[0,2,0],&mut rng),Err(Error::Parameter));
}
#[test]
fn next_frame_never_fires_in_same_entry_frame_and_handles_counter_wrap(){
    let bytes=graph(NEXT_FRAME_EVENT,&[(1,0),(2,1)],&[(0,2)],&[]);let p=Program::parse(&bytes).unwrap();
    for tick in [7,u32::MAX]{
        let mut kernel=Actions(Clock(tick));let mut vars=[0];let mut rng=0;
        assert!(!kernel.execute(&p,action(&p),Phase::Enter,&mut vars,&mut rng).unwrap().finished);
        assert!(!kernel.execute(&p,action(&p),Phase::Update,&mut vars,&mut rng).unwrap().finished);
        kernel.0.0=tick.wrapping_add(1);
        let result=kernel.execute(&p,action(&p),Phase::Update,&mut vars,&mut rng).unwrap();assert!(result.finished);assert_eq!(result.event,1);
    }
}
