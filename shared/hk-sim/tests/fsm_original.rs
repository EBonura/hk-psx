use hk_format::fsm::Program;
mod reference_gruzzer;
use reference_gruzzer::{Gruzzer,Side};
use hk_sim::{fsm::{Machine,Phase,Error},fsm_actions::{Actions,Environment},ONE};
#[derive(Default)]
struct World { camera:[i32;3], angle:i32,speed:i32,contacts:u8, facing:i32 }
impl Environment for World {
    fn position(&self,object:i32)->Result<[i32;3],Error>{match object{1=>Ok([0;3]),2=>Ok(self.camera),_=>Err(Error::Parameter)}}
    fn face_direction(&mut self,object:i32,right:bool)->Result<(),Error>{
        if object!=1||right{return Err(Error::Parameter);}
        let a=self.angle.rem_euclid(360*ONE);
        self.facing=if self.speed!=0&&!(90*ONE..=270*ONE).contains(&a){1}else{-1};Ok(())
    }
    fn velocity_as_angle(&mut self,object:i32,angle:i32,speed:i32)->Result<(),Error>{
        if object!=1{return Err(Error::Parameter);}self.angle=angle;self.speed=speed;Ok(())
    }
    fn contacts(&self,_:bool,_:bool,_:bool)->u8{self.contacts}
}
fn graph()->Vec<u8>{std::fs::read(std::env::var("HK_ORIGINAL_GRUZZER_FSM").expect("original cooked FSM required")).unwrap()}
#[test]
fn concrete_flying_dispatch_matches_combined_dispatch_including_cold_facing(){
    use hk_sim::{fsm_actions::FlyingActions,fsm_body::{Body,Flying}};
    let p=Program::parse(include_bytes!("../../../data/gruzzer.fsm")).unwrap();
    for seed in [0,1,0x12345678,u32::MAX]{
        let make=||Flying{camera:[44*ONE,0,0],position:[0;3],angle:0,speed:0,contacts:0,facing:-1};
        let mut before=Actions(Body::Flying(make()));let mut after=FlyingActions(make());
        let mut old=Machine::new(p,seed);let mut actual=old;
        for tick in 0..1026{
            let Body::Flying(previous)=&mut before.0 else{unreachable!()};
            previous.camera=if tick==0{[44*ONE,0,0]}else{[44*ONE-1,0,0]};
            previous.contacts=if tick<2{0}else{1<<((tick-2)%4)};
            after.0.camera=previous.camera;after.0.contacts=previous.contacts;
            if tick>=2{
                old.step(&p,Phase::CollisionStay,&mut before).unwrap();
                actual.step_fly(&p,Phase::CollisionStay,&mut after).unwrap();
            }
            old.step(&p,Phase::Update,&mut before).unwrap();actual.step_fly(&p,Phase::Update,&mut after).unwrap();
            let Body::Flying(previous)=&before.0 else{unreachable!()};
            assert_eq!(actual,old);
            assert_eq!((after.0.angle,after.0.speed,after.0.facing),(previous.angle,previous.speed,previous.facing));
        }
    }
}
#[test]
fn isolated_flying_production_closure_preserves_4096_terrain_bounces(){
    for seed in [0,1,0x12345678,u32::MAX]{
        let mut old=Gruzzer::new(seed);
        let mut actual=hk_sim::gruzzer::Gruzzer::new(seed);
        for camera in [[44*ONE,0,0],[44*ONE-1,0,0]]{
            old.tick(camera,[0;3]);actual.tick(camera,[0;3]);
            // The installed original graph retains its pre-Fly facing on the
            // wake tick; the historical hand-port updates facing immediately.
            // Compare authored angle here and post-physics facing below.
            assert_eq!(actual.angle(),old.angle());
        }
        for step in 0..1024{
            let side=[Side::Up,Side::Right,Side::Down,Side::Left][step%4];
            let actual_side=[hk_sim::gruzzer::Side::Up,hk_sim::gruzzer::Side::Right,hk_sim::gruzzer::Side::Down,hk_sim::gruzzer::Side::Left][step%4];
            old.bonk(side);actual.bonk(actual_side);
            old.tick([0;3],[0;3]);actual.tick([0;3],[0;3]);
            assert_eq!(actual.angle(),old.angle());assert_eq!(actual.facing(),old.facing());
        }
    }
}
fn to_event(p:Program<'_>,state:u16,target:u16)->u16{
    let s=p.state(state).unwrap();
    (s.transitions..s.transitions+s.transition_count).find_map(|i|{let(e,t)=p.transition(i).unwrap();(t==target).then_some(e)}).unwrap()
}
#[test]
#[ignore = "requires locally cooked original Windows Gruzzer graph"]
fn original_camera_wake_and_4096_bonks_match_existing_controller_with_same_port_rng(){
    let b=graph();let p=Program::parse(&b).unwrap();
    assert_eq!((p.state_count(),p.action_count(),p.variable_count()),(23,36,8));
    for seed in [0,1,0x12345678,u32::MAX] {
        let mut m=Machine::new(p,seed);let mut k=Actions(World{camera:[44*ONE,0,0],..World::default()});
        let mut old=Gruzzer::new(seed);
        m.step(&p,Phase::Update,&mut k).unwrap();old.tick(k.0.camera,[0;3]);
        assert_eq!(m.state(),0); // strict <44, equality stays asleep
        k.0.camera=[44*ONE-1,0,0];
        m.step(&p,Phase::Update,&mut k).unwrap();old.tick(k.0.camera,[0;3]);
        assert_eq!(m.state(),16);assert_eq!(m.variables[4],old.angle());
        for step in 0..1024 {
            let side=[Side::Up,Side::Right,Side::Down,Side::Left][step%4];
            k.0.contacts=1<<(step%4);
            m.step(&p,if step%2==0{Phase::CollisionStay}else{Phase::CollisionEnter},&mut k).unwrap();
            old.bonk(side);
            assert_eq!(m.state(),16);assert_eq!(m.variables[4],old.angle(),"seed{seed} step{step}");
            m.step(&p,Phase::Update,&mut k).unwrap();
            assert_eq!(k.0.angle,old.angle());assert_eq!(k.0.speed,hk_sim::gruzzer::SPEED);
            assert_eq!(k.0.facing,old.facing());
        }
    }
}
#[test]
#[ignore = "requires locally cooked original Windows Gruzzer graph"]
fn original_collision_priority_start_up_override_and_global_stop_go(){
    let b=graph();let p=Program::parse(&b).unwrap();
    let mut m=Machine::new(p,7);m.variables[7]=1;
    let mut k=Actions(World::default());m.step(&p,Phase::Update,&mut k).unwrap();
    assert_eq!(m.variables[4],90*ONE); // Start Up overrides initial random
    let mut old=Gruzzer::new(7);old.tick([0;3],[0;3]);
    // Use a second ordinary graph to verify the common collider priority.
    let mut ordinary=Machine::new(p,7);ordinary.step(&p,Phase::Update,&mut k).unwrap();
    k.0.contacts=15;ordinary.step(&p,Phase::CollisionStay,&mut k).unwrap();old.bonk(Side::Up);
    assert_eq!(ordinary.variables[4],old.angle());
    // Original global STOP/GO are graph edges, even though placed flies have no sender.
    let stop=(0..256).find(|e|p.target(16,*e)==Some(17)).unwrap();
    m.event(&p,stop,&mut k).unwrap();assert_eq!(m.state(),17);assert_eq!(m.variables[2],0);
    for (target,low,high) in [(19,20,160),(20,-70,70),(21,110,250),(22,200,340)]{
        let event=(0..256).find(|e|p.target(17,*e)==Some(target)).unwrap();
        m.event(&p,event,&mut k).unwrap();assert_eq!(m.state(),16);
        assert!((low*ONE..high*ONE).contains(&m.variables[4]));
        m.event(&p,stop,&mut k).unwrap();
    }
    let _=to_event(p,16,4);
    m.die();let prior=m.state();m.event(&p,0,&mut k).unwrap();assert_eq!(m.state(),prior);
}
#[test]
#[ignore = "requires locally cooked original Windows Gruzzer graph"]
fn original_float_thresholds_and_tolerance_edges(){
    use hk_sim::fsm::Kernel;
    let b=graph();let p=Program::parse(&b).unwrap();let mut k=Actions(World::default());
    let switch=p.action(p.state(1).unwrap().actions).unwrap();let mut vars=Machine::new(p,0).variables;let mut rng=0;
    for (angle,target) in [(89*ONE,Some(3)),(90*ONE,Some(2)),(269*ONE,Some(2)),(270*ONE,Some(3)),(359*ONE,Some(3)),(360*ONE,None)]{
        vars[4]=angle;let r=k.execute(&p,switch,Phase::Enter,&mut vars,&mut rng).unwrap();
        assert_eq!(p.target(1,r.event),target);
    }
    let compare=p.action(p.state(0).unwrap().actions+4).unwrap();
    for distance in [44*ONE-1,44*ONE,44*ONE+1]{
        vars[3]=distance;let r=k.execute(&p,compare,Phase::Update,&mut vars,&mut rng).unwrap();
        assert_eq!(p.target(0,r.event),if distance<44*ONE{Some(18)}else{None});
    }
    // Reuse the original compare action with only its constant tolerance set
    // to one unit. The original IL chooses equality at abs(diff)==tolerance.
    let mut modified=b.clone();let operand=p.operand(compare.operands+2).unwrap();assert_eq!(operand.kind,0);
    let mut constants=32;
    for (i,stride) in [8,8,4,4,8].iter().enumerate(){let n=u16::from_le_bytes([b[8+i*2],b[9+i*2]]) as usize;constants+=n*stride;}
    let at=constants+operand.index as usize*4;modified[at..at+4].copy_from_slice(&ONE.to_le_bytes());
    let q=Program::parse(&modified).unwrap();
    for distance in [43*ONE,44*ONE,45*ONE]{vars[3]=distance;let r=k.execute(&q,compare,Phase::Update,&mut vars,&mut rng).unwrap();assert_eq!(r.event,hk_format::fsm::NO_EVENT);}
    vars[3]=43*ONE-1;let r=k.execute(&q,compare,Phase::Update,&mut vars,&mut rng).unwrap();assert_eq!(q.target(0,r.event),Some(18));
}
#[test]
#[ignore = "requires locally cooked original Windows Gruzzer graph"]
fn terrain_event_consumes_callback_before_unsupported_layer_actions(){
    struct Checked(World);
    impl Environment for Checked {
        fn position(&self,o:i32)->Result<[i32;3],Error>{self.0.position(o)}
        fn face_direction(&mut self,o:i32,r:bool)->Result<(),Error>{self.0.face_direction(o,r)}
        fn velocity_as_angle(&mut self,o:i32,a:i32,s:i32)->Result<(),Error>{self.0.velocity_as_angle(o,a,s)}
        fn contacts(&self,other:bool,_:bool,ignore:bool)->u8{
            assert!(!other,"a terrain contact must transition before layer24/25 actions");
            assert!(!ignore);self.0.contacts
        }
    }
    let b=graph();let p=Program::parse(&b).unwrap();
    for phase in [Phase::CollisionStay,Phase::CollisionEnter]{for mask in 1..16{
        let mut m=Machine::new(p,1);let mut k=Actions(Checked(World::default()));
        m.step(&p,Phase::Update,&mut k).unwrap();assert_eq!(m.state(),16);
        k.0.0.contacts=mask;m.step(&p,phase,&mut k).unwrap();assert_eq!(m.state(),16);
    }}
}
