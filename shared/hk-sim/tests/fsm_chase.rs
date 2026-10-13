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
    let bytes=graph(BOOL_ALL_TRUE,&[(1,0),(1,1),(4,0),(2,1)],&[(0,1),(0,1)],&[]);
    let p=Program::parse(&bytes).unwrap();let mut kernel=Actions(Clock(0));let mut rng=0;
    for (a,b,event) in [(0,0,NO_EVENT),(1,0,NO_EVENT),(0,1,NO_EVENT),(1,1,1)]{
        let mut vars=[a,b];let result=kernel.execute(&p,action(&p),Phase::Update,&mut vars,&mut rng).unwrap();assert_eq!(result.event,event);assert_eq!(vars,[a,b]);
    }
    // A false first condition must not disguise an invalid later boolean.
    assert_eq!(kernel.execute(&p,action(&p),Phase::Update,&mut[0,2],&mut rng),Err(Error::Parameter));
}
#[test]
fn next_frame_finishes_on_first_update_not_on_enter(){
    let bytes=graph(NEXT_FRAME_EVENT,&[(2,1)],&[],&[]);let p=Program::parse(&bytes).unwrap();
    let mut kernel=Actions(Clock(7));let mut vars=[];let mut rng=0;
    assert!(!kernel.execute(&p,action(&p),Phase::Enter,&mut vars,&mut rng).unwrap().finished);
    let result=kernel.execute(&p,action(&p),Phase::Update,&mut vars,&mut rng).unwrap();
    assert!(result.finished);assert_eq!(result.event,1);
}

#[test]
fn bool_all_true_empty_array_does_not_emit_or_write_output() {
    let bytes=graph(BOOL_ALL_TRUE,&[(1,0),(2,1)],&[(1,1)],&[]);
    let p=Program::parse(&bytes).unwrap();let mut vars=[1];let mut rng=0;
    let result=Actions(Clock(0)).execute(&p,action(&p),Phase::Enter,&mut vars,&mut rng).unwrap();
    assert_eq!(result.event,NO_EVENT);assert_eq!(vars,[1]);
}

#[test]
fn next_frame_initial_entry_updates_but_update_transition_only_enters_destination(){
    use hk_sim::fsm::{Cursor,ResultAction};
    fn schedule(start:u16)->Vec<u8>{
        let mut bytes=Vec::from(*hk_format::fsm::MAGIC);
        for n in [3u16,3,2,1,0,0,2,0,start,0]{bytes.extend(n.to_le_bytes());}
        bytes.extend(0u32.to_le_bytes());
        for state in [[0u16,1,0,1],[1,1,1,1],[2,1,2,0]]{for n in state{bytes.extend(n.to_le_bytes());}}
        for (op,start,count) in [(65001u16,0u16,0u8),(NEXT_FRAME_EVENT,0,1),(65002,1,0)]{
            bytes.extend(op.to_le_bytes());bytes.extend(start.to_le_bytes());bytes.extend([count,ENABLED|UPDATE,0,0]);
        }
        for edge in [[1u16,1],[1,2]]{for n in edge{bytes.extend(n.to_le_bytes());}}
        bytes.extend([2,0,1,0]);let n=bytes.len() as u32;bytes[28..32].copy_from_slice(&n.to_le_bytes());bytes
    }
    #[derive(Default)]
    struct Wrapper {hold_updates:u32}
    impl Kernel for Wrapper {
        fn execute(&mut self,p:&Program<'_>,a:Action,phase:Phase,v:&mut[i32],rng:&mut u32)->Result<ResultAction,Error>{
            if a.opcode==NEXT_FRAME_EVENT{return Actions(Clock(0)).execute(p,a,phase,v,rng);}
            if a.opcode==65001{return Ok(ResultAction{finished:false,event:if phase==Phase::Update{1}else{NO_EVENT}});}
            if phase==Phase::Update{self.hold_updates+=1;}Ok(ResultAction::running())
        }
    }
    for start in [0,1]{
        let bytes=schedule(start);let p=Program::parse(&bytes).unwrap();let mut cursor=Cursor::initial(&p);let mut vars=[];let mut rng=0;let mut kernel=Wrapper::default();
        cursor.step(&p,&mut vars,&mut rng,Phase::Update,&mut kernel).unwrap();
        assert_eq!(cursor.state(),if start==0{1}else{2});assert_eq!(kernel.hold_updates,0);
        if start==0{cursor.step(&p,&mut vars,&mut rng,Phase::Update,&mut kernel).unwrap();assert_eq!(cursor.state(),2);assert_eq!(kernel.hold_updates,0);}
    }
}
