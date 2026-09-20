// cheats composes the equipped charms into the live parameters and reconciles
// the all-charms grant. The real module links the cooked catalogue this harness
// has no manifest directory for, and an empty board composes to the base
// anyway, so the contract is stubbed here; tests/charms_runtime.rs exercises
// the real one.
mod charms{
 pub fn vitals(base:hk_sim::VitalParams)->hk_sim::VitalParams{base}
 pub fn grant_all(_:bool){}
 pub fn on_damage(_:&mut hk_sim::Vitals,_:hk_sim::VitalParams){}
}
// And the shop's half of PlayerData, which `cheats::params` composes under
// the charms. The real module `include!`s the cooked stock relative to its own
// crate, which this one is not, and a shop nobody has visited composes to the
// base anyway; tests/shop_runtime.rs exercises the real one.
mod shop{
 pub fn vitals(base:hk_sim::VitalParams)->hk_sim::VitalParams{base}
}
#[path="../../../game/src/cheats.rs"]mod cheats;
use cheats::{Action,Settings,CHEAT_ROWS};
use hk_sim::{Hurt,Vitals,VitalParams,Focus,FocusParams,FocusInput};
const P:VitalParams=VitalParams{max_health:5,max_soul:99,nail_damage:5,soul_per_hit:11,invulnerable_ticks:79,hazard_invulnerable_ticks:40,recoil_ticks:12,freeze_ticks:19,death_ticks:171,recoil_speed:15*65536};
#[test]fn default_settings_match_normal_vitals_and_damage_for_every_small_state(){
 let s=Settings::new();assert_eq!(s.bits(),0);assert_eq!(s.new_vitals(P),Vitals::new(P));
 for hp in 1..=5 {for blue in 0..=6 {for damage in 0..=10 {for hazard in [false,true] {for immunity in [0,79] {
  let mut normal=Vitals::new(P);normal.health=hp;normal.blue_health=blue;normal.invulnerable_ticks=immunity;normal.soul=42;
  let mut tested=normal;s.apply(s,&mut tested,P,Action::None);s.maintain(&mut tested,P);assert_eq!(tested,normal);
  assert_eq!(s.hurt(&mut tested,P,damage,-1,hazard,false),normal.hurt(P,damage,-1,hazard));assert_eq!(tested,normal);
 }}}}}
}
#[test]fn invincibility_preserves_both_pools_but_keeps_hazard_recovery(){
 let s=Settings{invincible:true,..Settings::new()};let mut v=Vitals::new(P);v.health=1;v.blue_health=3;v.soul=8;
 let before=v;assert_eq!(s.hurt(&mut v,P,u16::MAX,1,false,false),Hurt::Ignored);assert_eq!(v,before);
 assert_eq!(s.hurt(&mut v,P,u16::MAX,-9,true,false),Hurt::Hazard);
 assert_eq!((v.health,v.blue_health,v.soul,v.dead,v.hazard_pending),(1,3,8,false,true));
 assert_eq!(s.hurt(&mut v,P,1,1,true,false),Hurt::Ignored);
 v.finish_hazard_respawn(s.params(P));assert!(!v.hazard_pending);assert_eq!(v.invulnerable_ticks,P.hazard_invulnerable_ticks);
 assert_eq!(s.hurt(&mut v,P,1,1,true,false),Hurt::Hazard);assert_eq!((v.health,v.blue_health),(1,3));
}
#[test]fn toggles_actions_and_session_resets_have_bounded_independent_effects(){
 let off=Settings::new();let mut s=off;let mut v=Vitals::new(P);v.health=2;v.soul=3;v.blue_health=2;
 s.adjust(3,1);s.apply(off,&mut v,P,Action::None);assert_eq!((v.health,s.params(P).max_health),(9,9));
 v.health=4;s.apply(s,&mut v,P,Action::None);assert_eq!(v.health,4,"max masks must not act as invincibility");
 for row in 0..4 {s.adjust(row,1);s.adjust(row,1);assert!(s.enabled(row));}assert_eq!(s.bits(),15);
 assert_eq!(s.params(P).nail_damage,21);s.adjust(CHEAT_ROWS,1);s.adjust(1,0);assert_eq!(s.bits(),15);
 let reset=s.new_vitals(P);assert_eq!((reset.health,reset.soul,reset.blue_health),(9,99,0));assert_eq!(s.bits(),15);
 v.health=9;s.apply(s,&mut v,P,Action::Restore);assert_eq!((v.health,v.soul,v.blue_health),(9,99,2));
 for _ in 0..10 {s.apply(s,&mut v,P,Action::AddBlue);}assert_eq!(v.blue_health,20);
 v.blue_health=27;s.apply(s,&mut v,P,Action::AddBlue);assert_eq!(v.blue_health,27);
 off.apply(s,&mut v,P,Action::Reset);assert_eq!((v.health,v.blue_health,v.soul),(5,27,99));
 v.health=2;off.apply(off,&mut v,P,Action::None);assert_eq!(v.health,2);
 v.dead=true;v.health=0;let before=v;s.apply(off,&mut v,P,Action::Restore);assert_eq!(v,before,"actions must not cancel death relocation");
}
#[test]fn infinite_soul_keeps_original_focus_timing_and_nine_mask_cap(){
 let s=Settings{infinite_soul:true,max_masks:true,..Settings::new()};let mut v=s.new_vitals(P);v.health=7;
 let f=FocusParams{hold_ticks:15,start_ticks:15,heal_ticks:12,cancel_ticks:15,finish_ticks:14,first_grace_ticks:12,repeat_grace_ticks:27,drain_interval_us:27000,cost:33,heal_amount:1,attack_recovery_ticks:6};
 let mut focus=Focus::new();let(mut healed,mut drained)=(0,0);
 for _ in 0..300 {s.maintain(&mut v,P);let e=focus.step(f,s.params(P),FocusInput{held:true,grounded:true,can_start:true},&mut v);s.maintain(&mut v,P);healed+=e.healed;drained+=e.drained;assert_eq!(v.soul,99);}
 assert_eq!((v.health,healed,drained),(9,2,66));
 let off=Settings::new();v.soul=12;off.maintain(&mut v,P);assert_eq!(v.soul,12);
}
