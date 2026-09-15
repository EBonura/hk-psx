#[path="../../../game/src/cheats.rs"]mod cheats;
#[path="../../../game/src/pause.rs"]mod pause;
fn step(p:&mut pause::State,c:&mut cheats::Settings,b:u16)->bool{p.step(b,&mut 10,&mut 10,c)}
fn tap(p:&mut pause::State,c:&mut cheats::Settings,b:u16)->bool{step(p,c,0);step(p,c,b)}
#[test]fn actions_are_edges_and_back_does_not_resume(){
 let mut p=pause::State::new();let mut c=cheats::Settings::new();
 tap(&mut p,&mut c,0x10);assert_eq!(p.row,4);tap(&mut p,&mut c,0x4000);assert!(p.cheats);
 tap(&mut p,&mut c,0x4000);assert!(c.invincible);
 step(&mut p,&mut c,0x4000);assert!(c.invincible);
 for _ in 0..4{tap(&mut p,&mut c,0x40);}
 tap(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::Restore);
 step(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::None);
 tap(&mut p,&mut c,0x40);tap(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::AddBlue);
 tap(&mut p,&mut c,0x40);tap(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::Reset);assert_eq!(c,cheats::Settings::new());
 assert!(!tap(&mut p,&mut c,0x2000));assert!(!p.cheats);assert_eq!(p.row,4);
 assert!(tap(&mut p,&mut c,0x2000));
}
