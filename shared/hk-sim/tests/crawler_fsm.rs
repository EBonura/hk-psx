use hk_sim::{ONE,WalkParams,WalkState,fsm::{Machine,Phase,Error},fsm_actions::{Actions,Environment}};
use hk_format::fsm::Program;
use hk_sim::crawler_fsm::{BANK,GRAPHS,Crawler};
#[test]
fn every_original_crawler_graph_requires_only_walking_actions() {
    use hk_format::fsm::{GROUP_CORE, GROUP_WALK};
    for &(_, _, start, len) in GRAPHS {
        let program = Program::parse(&BANK[start as usize..start as usize + len as usize]).unwrap();
        assert_eq!(program.action_groups().unwrap(), GROUP_CORE | GROUP_WALK);
    }
}
#[test]
fn action_group_admission_includes_later_state_actions_and_refuses_unknowns() {
    use hk_format::fsm::{opcode::RANDOM_FLOAT, GROUP_CORE, GROUP_WALK, GROUP_FLY, HEADER};
    let (_, _, start, len) = GRAPHS[0];
    let mut bytes = BANK[start as usize..start as usize + len as usize].to_vec();
    let program = Program::parse(&bytes).unwrap();
    let last = HEADER + program.state_count() as usize * 8 + (program.action_count() as usize - 1) * 8;
    bytes[last..last + 2].copy_from_slice(&RANDOM_FLOAT.to_le_bytes());
    assert_eq!(Program::parse(&bytes).unwrap().action_groups().unwrap(), GROUP_CORE | GROUP_WALK | GROUP_FLY);
    bytes[last..last + 2].copy_from_slice(&65534u16.to_le_bytes());
    assert!(Program::parse(&bytes).unwrap().action_groups().is_err());
}
struct World { walk:WalkState,params:WalkParams,senses:(bool,bool,bool),velocity:i32,position:[i32;3],translations:u32,real_ticks:u16 }
impl World {
    fn new(direction:i32,initial_turn:bool)->Self{Self{walk:WalkState::new(direction,initial_turn),params:WalkParams{speed:4*ONE,turn_ticks:7,turn_cooldown_ticks:60},senses:(true,false,false),velocity:0,position:[0,ONE,0],translations:0,real_ticks:2}}
}
impl Environment for World {
    fn position(&self,o:i32)->Result<[i32;3],Error>{if o==1{Ok(self.position)}else{Err(Error::Parameter)}}
    fn face_direction(&mut self,_:i32,_:bool)->Result<(),Error>{Err(Error::UnsupportedAction)}
    fn velocity_as_angle(&mut self,_:i32,_:i32,_:i32)->Result<(),Error>{Err(Error::UnsupportedAction)}
    fn contacts(&self,_:bool,_:bool,_:bool)->u8{0}
    fn elapsed_ticks(&self,real:bool)->Result<u16,Error>{Ok(if real{self.real_ticks}else{1})}
    fn translate(&mut self,o:i32,delta:[Option<i32>;3],local:bool,per_second:bool)->Result<(),Error>{
        assert_eq!(o,1);assert!(!per_second);assert_eq!(delta[1..],[None,None]);
        let mut x=delta[0].unwrap();if local{x*=-self.walk.direction;}
        self.position[0]+=x;self.translations+=1;Ok(())
    }
    fn walk(&mut self,speed:i32,delay:i32,left:bool,right:bool,keep:bool,phase:Phase)->Result<(),Error>{
        assert_eq!(speed,4*ONE);assert_eq!(delay,ONE);
        if phase==Phase::Enter{if !keep&&(left||right){self.walk=WalkState::new(if left{-1}else{1},false);}}
        else if phase==Phase::Update{self.velocity=self.walk.tick(self.params,self.senses.0,self.senses.1,self.senses.2);}
        Ok(())
    }
}
fn program()->Program<'static>{let (_,_,offset,len)=GRAPHS[0];Program::parse(&BANK[offset as usize..offset as usize+len as usize]).unwrap()}
#[test]
fn every_admitted_instance_selects_its_own_checked_graph(){
    assert_eq!(GRAPHS.len(),10);
    for &(scene,source,_,_) in GRAPHS{assert!(Crawler::new(scene as usize,source,1).is_some());}
    assert!(Crawler::new(0,u32::MAX,1).is_none());
    assert!(Crawler::new(999,GRAPHS[0].1,1).is_none());
    assert_eq!(program().state_count(),5);
}
#[test]
fn real_clock_preserves_stalls_and_counter_wrap(){
    let (s,id,_,_)=GRAPHS[0];let mut c=Crawler::new(s as usize,id,1).unwrap();
    assert_eq!(c.elapsed_real_ticks(u32::MAX-2),0);
    assert_eq!(c.elapsed_real_ticks(1),4);
    assert_eq!(c.elapsed_real_ticks(91),90);
    assert_eq!(c.elapsed_real_ticks(100000),u16::MAX);
}
#[test]
fn paused_reactivation_expires_real_wait_but_preserves_scaled_wait(){
    let p=program();let (scene,id,_,_)=GRAPHS[0];
    for (target,timer,expected) in [(2,1,0),(3,2,29*ONE)]{
        let mut clock=Crawler::new(scene as usize,id,1).unwrap();clock.elapsed_real_ticks(100);
        let mut machine=Machine::new(p,1);let mut kernel=Actions(World::new(-1,false));
        machine.step(&p,Phase::Update,&mut kernel).unwrap();
        let s=p.state(0).unwrap();let event=(s.transitions..s.transitions+s.transition_count).find_map(|i|{let(e,t)=p.transition(i).unwrap();(t==target).then_some(e)}).unwrap();
        machine.event(&p,event,&mut kernel).unwrap();assert_eq!(machine.variables[timer],30*ONE);
        // No simulation updates occur during this 90-VBlank pause.
        kernel.0.real_ticks=clock.elapsed_real_ticks(190);
        machine.step(&p,Phase::Update,&mut kernel).unwrap();
        assert_eq!(machine.variables[timer],expected);
    }
}
#[test]
fn original_walk_graph_matches_previous_turn_and_edge_trajectory(){
    for direction in [-1,1]{for initial_turn in [false,true]{
        let (scene,source,_,_)=GRAPHS[0];let mut actual=Crawler::new(scene as usize,source,9).unwrap();
        let w=World::new(direction,initial_turn);
        let mut kernel=hk_sim::fsm_actions::WalkingActions(hk_sim::fsm_body::Walking{walk:w.walk,position:w.position,params:w.params,senses:w.senses,velocity:0,real_ticks:w.real_ticks,stationary_fixture:false});
        let mut reference=WalkState::new(direction,initial_turn);
        for tick in 0..1200{
            let senses=(tick%19!=0,tick%67==3,tick%83==4);kernel.0.senses=senses;
            let expected=reference.tick(kernel.0.params,senses.0,senses.1,senses.2);
            actual.step(&mut kernel).unwrap();
            assert_eq!(kernel.0.velocity,expected,"tick{tick}");assert_eq!(kernel.0.walk,reference);
            assert_eq!(actual.state(),0);
        }
    }}
}
#[test]
fn shared_production_body_preserves_walk_wait_and_translate_order(){
    use hk_sim::{fsm_body::Walking,fsm_actions::WalkingActions};
    let p=program();
    for target in [None,Some(2),Some(3)]{
        let mut old=Machine::new(p,9);let mut actual=old;
        let mut reference=Actions(World::new(-1,false));
        let w=&reference.0;
        let mut shared=WalkingActions(Walking{walk:w.walk,position:w.position,params:w.params,senses:w.senses,velocity:0,real_ticks:w.real_ticks,stationary_fixture:false});
        old.step(&p,Phase::Update,&mut reference).unwrap();actual.step_walk(&p,Phase::Update,&mut shared).unwrap();
        if let Some(target)=target{
            let state=p.state(0).unwrap();
            let event=(state.transitions..state.transitions+state.transition_count).find_map(|i|{let(e,t)=p.transition(i).unwrap();(t==target).then_some(e)}).unwrap();
            old.event(&p,event,&mut reference).unwrap();actual.event_walk(&p,event,&mut shared).unwrap();
        }
        for tick in 0..1600{
            let senses=(tick%19!=0,tick%67==3,tick%83==4);
            reference.0.senses=senses;
            shared.0.senses=senses;
            old.step(&p,Phase::Update,&mut reference).unwrap();actual.step_walk(&p,Phase::Update,&mut shared).unwrap();
            let w=&shared.0;
            assert_eq!(actual,old);assert_eq!(w.walk,reference.0.walk);
            assert_eq!(w.position,reference.0.position);assert_eq!(w.velocity,reference.0.velocity);
        }
    }
}
#[test]
fn original_wait_updates_without_every_frame_and_translate_runs_once_in_order(){
    let p=program();
    for (target,timer,ticks) in [(2,1,15),(3,2,30)]{
        let mut machine=Machine::new(p,1);let mut kernel=Actions(World::new(-1,false));
        machine.step(&p,Phase::Update,&mut kernel).unwrap();
        let s=p.state(0).unwrap();let event=(s.transitions..s.transitions+s.transition_count).find_map(|i|{let(e,t)=p.transition(i).unwrap();(t==target).then_some(e)}).unwrap();
        machine.event(&p,event,&mut kernel).unwrap();
        assert_eq!(machine.variables[timer],30*ONE);
        assert_eq!(kernel.0.position,[-6554,ONE,0]);assert_eq!(kernel.0.translations,1);
        for tick in 1..=ticks{
            machine.step(&p,Phase::Update,&mut kernel).unwrap();
            assert_eq!(machine.variables[timer]==0,tick==ticks);
            assert_eq!(kernel.0.translations,1);
        }
        // Walk's coroutine never finishes merely because Wait finishes.
        assert_eq!(machine.state(),target);
    }
}
