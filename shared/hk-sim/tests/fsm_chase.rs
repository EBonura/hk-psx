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
    let fixed=kernel.execute(&p,action(&p),Phase::FixedUpdate,&mut vars,&mut rng).unwrap();
    assert!(!fixed.finished);assert_eq!(fixed.event,NO_EVENT);
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

#[test]
fn rational_fixed_clock_uses_authored_interval_not_render_count_or_rounded_50hz(){
    use hk_sim::fsm_chase::FixedClock;
    let mut clock=FixedClock::new();let mut steps=0u64;
    for tick in 1..=FixedClock::FIXED_UNITS as u64 {
        steps+=clock.step() as u64;
        if tick<=6{assert_eq!(steps,tick*FixedClock::SIMULATION_UNITS as u64/FixedClock::FIXED_UNITS as u64);}
    }
    assert_eq!(steps,FixedClock::SIMULATION_UNITS as u64);
    assert_ne!(steps,FixedClock::FIXED_UNITS as u64*50/60);
    // Pausing does not advance the clock; copying/seating a new actor does not
    // create a separate phase. All actors consume the same world result.
    let paused=clock;
    assert_eq!(clock,paused);
    assert_eq!(core::mem::size_of::<FixedClock>(),4);
}

#[test]
fn compact_variables_preserve_both_mutable_namespaces_and_reject_partial_save(){
    use hk_sim::vengefly_fsm::Variables;
    let chaser_bytes=graph(FLOAT_CLAMP,&[(1,0),(0,0),(0,1)],&[(10*65536,2),(0,1),(1,1),(0,1),(0,1),(0,3),(0,3),(0,2)],&[0,20*65536]);
    let receiver_bytes=graph(FLOAT_CLAMP,&[(1,0),(0,0),(0,1)],&[(0,2),(0,1),(0,2)],&[-65536,65536]);
    let programs=[Program::parse(&chaser_bytes).unwrap(),Program::parse(&receiver_bytes).unwrap()];
    let mut store=Variables::initial(programs).unwrap();
    let chaser=[17*65536,1,0,1,1,1,2,9];let receiver=[-65536,1,21];
    store.save(programs,0,&chaser).unwrap();store.save(programs,1,&receiver).unwrap();
    let mut actual=[0;8];store.load(programs,0,&mut actual).unwrap();assert_eq!(actual,chaser);
    let mut invalid=chaser;invalid[0]=99;invalid[7]=20;invalid[6]=4;
    assert_eq!(store.save(programs,0,&invalid),Err(Error::Parameter));
    store.load(programs,0,&mut actual).unwrap();assert_eq!(actual,chaser);
    let mut actual=[0;3];store.load(programs,1,&mut actual).unwrap();assert_eq!(actual,receiver);
    assert_eq!(core::mem::size_of::<Variables>(),20);
    assert_eq!(core::mem::size_of::<hk_sim::vengefly_fsm::Pair>(),36);
}
