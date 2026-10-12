use hk_format::fsm::{Program, ENABLED, UPDATE, NO_EVENT};
use hk_sim::fsm::{Error, Kernel, Machine, Phase, ResultAction};

fn bytes(states: &[[u16;4]], actions: &[(u16,u8)], edges: &[[u16;2]]) -> Vec<u8> {
    let mut b = Vec::from(*hk_format::fsm::MAGIC);
    for count in [states.len() as u16,actions.len() as u16,edges.len() as u16,0,1,0,2,0,0,0] {
        b.extend(count.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    for s in states { for v in s { b.extend(v.to_le_bytes()); } }
    for (opcode,flags) in actions {
        b.extend(opcode.to_le_bytes()); b.extend(0u16.to_le_bytes()); b.extend([0,*flags,0,0]);
    }
    for e in edges { for v in e { b.extend(v.to_le_bytes()); } }
    b.extend([0,0,0,0,2,0,0,0]);
    let n=b.len() as u32; b[28..32].copy_from_slice(&n.to_le_bytes());
    b
}
#[derive(Default)]
struct TestKernel { calls: Vec<(u16,Phase)>, wait: u8 }
impl Kernel for TestKernel {
    fn execute(&mut self,_:Program<'_>,a:hk_format::fsm::Action,p:Phase,v:&mut[i32;16],_:&mut u32)->Result<ResultAction,Error> {
        self.calls.push((a.opcode,p));
        if a.opcode==1 { v[0]=7; Ok(ResultAction::finished()) }
        else if a.opcode==2 { Ok(ResultAction{finished:true,event:0}) }
        else if a.opcode==3 { v[0]=90; Ok(ResultAction::finished()) }
        else if a.opcode==4 {
            if p==Phase::Enter { self.wait=2; }
            else if p==Phase::Update { self.wait=self.wait.saturating_sub(1); }
            Ok(ResultAction{finished:self.wait==0,event:NO_EVENT})
        } else if a.opcode==5 { Ok(ResultAction::running()) }
        else { Err(Error::UnsupportedAction) }
    }
}
#[test]
fn immediate_event_interrupts_later_old_state_actions() {
    // A transition between a random/result write and a later override must
    // retain the first value, as original ordered conditional actions require.
    let b=bytes(&[[0,3,0,1],[3,1,1,0]],&[(1,ENABLED),(2,ENABLED),(3,ENABLED),(5,ENABLED)],&[[0,1]]);
    let p=Program::parse(&b).unwrap(); let mut m=Machine::new(p,5); let mut k=TestKernel::default();
    m.step(p,Phase::Enter,&mut k).unwrap();
    assert_eq!(m.state(),1); assert_eq!(m.variables[0],7);
    assert_eq!(k.calls,[(1,Phase::Enter),(2,Phase::Enter),(5,Phase::Enter)]);
}
#[test]
fn asynchronous_action_updates_without_a_serialized_every_frame_property() {
    let b=bytes(&[[0,1,0,1],[1,1,1,0]],&[(4,ENABLED|UPDATE),(5,ENABLED)],&[[0,1]]);
    let p=Program::parse(&b).unwrap(); let mut m=Machine::new(p,1); let mut k=TestKernel::default();
    m.step(p,Phase::Enter,&mut k).unwrap(); assert_eq!(m.state(),0);
    m.step(p,Phase::Update,&mut k).unwrap(); assert_eq!(m.state(),0);
    m.step(p,Phase::Update,&mut k).unwrap(); assert_eq!(m.state(),1);
}
#[test]
fn disabled_actions_do_not_execute_and_unknown_enabled_actions_fail() {
    let b=bytes(&[[0,2,0,0]],&[(999,0),(5,ENABLED)],&[]);
    let p=Program::parse(&b).unwrap(); let mut m=Machine::new(p,0); let mut k=TestKernel::default();
    m.step(p,Phase::Enter,&mut k).unwrap(); assert_eq!(k.calls.len(),1);
    let b=bytes(&[[0,1,0,0]],&[(999,ENABLED)],&[]);
    let p=Program::parse(&b).unwrap();
    assert_eq!(Machine::new(p,0).step(p,Phase::Enter,&mut k),Err(Error::UnsupportedAction));
}
#[test]
fn instantaneous_cycle_is_a_bounded_fault_not_an_infinite_frame() {
    let b=bytes(&[[0,1,0,1]],&[(2,ENABLED)],&[[0,0]]);
    let p=Program::parse(&b).unwrap(); let mut k=TestKernel::default();
    assert_eq!(Machine::new(p,0).step(p,Phase::Enter,&mut k),Err(Error::TransitionBudget));
}
#[test]
fn malformed_table_references_and_truncated_program_are_rejected() {
    let mut b=bytes(&[[0,1,0,1]],&[(5,ENABLED)],&[[0,0]]);
    let edge=32+8+8; b[edge+2..edge+4].copy_from_slice(&1u16.to_le_bytes());
    assert!(Program::parse(&b).is_err()); b.pop(); assert!(Program::parse(&b).is_err());
    let mut b=bytes(&[[0,1,0,0]],&[(5,ENABLED)],&[]); b[16..18].copy_from_slice(&17u16.to_le_bytes());
    assert!(Program::parse(&b).is_err());
}
