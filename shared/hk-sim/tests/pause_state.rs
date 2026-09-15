#[path="../../../game/src/cheats.rs"]mod cheats;
#[path="../../../game/src/pause.rs"]mod pause;
fn step(p:&mut pause::State,c:&mut cheats::Settings,b:u16)->bool{p.step(b,&mut 10,&mut 10,&mut 10,c)}
fn tap(p:&mut pause::State,c:&mut cheats::Settings,b:u16)->bool{step(p,c,0);step(p,c,b)}
#[test]fn actions_are_edges_and_back_does_not_resume(){
 let mut p=pause::State::new();let mut c=cheats::Settings::new();
 tap(&mut p,&mut c,0x10);assert_eq!(p.row,5);tap(&mut p,&mut c,0x4000);assert!(p.cheats);
 tap(&mut p,&mut c,0x4000);assert!(c.invincible);
 step(&mut p,&mut c,0x4000);assert!(c.invincible);
 for _ in 0..4{tap(&mut p,&mut c,0x40);}
 tap(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::Restore);
 step(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::None);
 tap(&mut p,&mut c,0x40);tap(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::AddBlue);
 tap(&mut p,&mut c,0x40);tap(&mut p,&mut c,0x4000);assert_eq!(p.action,cheats::Action::Reset);assert_eq!(c,cheats::Settings::new());
 assert!(!tap(&mut p,&mut c,0x2000));assert!(!p.cheats);assert_eq!(p.row,5);
 assert!(tap(&mut p,&mut c,0x2000));
}

#[test]fn music_volume_and_adjacent_pages_preserve_each_other(){
 let mut p=pause::State::new();let mut c=cheats::Settings::new();
 let(mut sfx,mut ambience,mut music)=(7,6,10);
 let mut tap=|p:&mut pause::State,c:&mut cheats::Settings,b|{
  assert!(!p.step(0,&mut sfx,&mut ambience,&mut music,c));
  p.step(b,&mut sfx,&mut ambience,&mut music,c)
 };
 for _ in 0..3{tap(&mut p,&mut c,0x40);}assert_eq!(p.row,3);
 for _ in 0..15{tap(&mut p,&mut c,0x80);}
 tap(&mut p,&mut c,0x20);assert!(!tap(&mut p,&mut c,0x4000));assert!(!p.controls&&!p.cheats);
 tap(&mut p,&mut c,0x40);assert_eq!(p.row,4);tap(&mut p,&mut c,0x4000);assert!(p.controls);
 assert!(!tap(&mut p,&mut c,0x2000));assert!(!p.controls);assert_eq!(p.row,4);
 tap(&mut p,&mut c,0x40);assert_eq!(p.row,5);tap(&mut p,&mut c,0x4000);assert!(p.cheats);
 tap(&mut p,&mut c,0x10);assert_eq!(p.row,7);assert!(!tap(&mut p,&mut c,0x4000));
 assert!(!p.cheats);assert_eq!(p.row,5);tap(&mut p,&mut c,0x40);assert_eq!(p.row,0);
 assert!(tap(&mut p,&mut c,0x4000));drop(tap);
 assert_eq!((sfx,ambience,music),(7,6,1));assert_eq!(c,cheats::Settings::new());
}
