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
// menu_state sizes the save screen by the profile count.
mod save{pub const PROFILES:usize=4;}
#[path="../../../game/src/menu_state.rs"]mod menu_state;
use menu_state::*;
fn tap(s:&mut State,b:u16)->bool{assert!(!s.step(0));s.step(b)}
#[test]fn first_confirm_preserves_direct_start_and_default_levels(){
 // Start Game opens the save screen; confirming a slot is what starts.
 for b in [START,CROSS,START|CROSS,START|DOWN]{
  let mut s=State::new();assert!(!s.step(b));assert_eq!(s.page,Page::Profiles);
  assert!(s.step(0)==false);assert!(s.step(CROSS));
  assert_eq!(s.profile,Some(0));assert_eq!(s.settings,Settings::new());}
}
#[test]fn options_apply_actual_settings_and_return_without_starting(){
 let mut s=State::new();tap(&mut s,DOWN);tap(&mut s,CROSS);assert_eq!(s.page,Page::Options);
 tap(&mut s,LEFT);assert_eq!(s.settings.sfx,9);tap(&mut s,DOWN);tap(&mut s,LEFT);assert_eq!(s.settings.ambience,9);
 tap(&mut s,CIRCLE);assert_eq!(s.page,Page::Main);assert_eq!(s.selected,1);
 tap(&mut s,UP);assert!(!tap(&mut s,START));assert!(tap(&mut s,CROSS));
 assert_eq!(s.settings,Settings{sfx:9,ambience:9,..Settings::new()});
}
#[test]fn controls_and_back_are_functional_and_confirm_never_retriggers_while_held(){
 let mut s=State::new();tap(&mut s,UP);tap(&mut s,UP);tap(&mut s,CROSS);assert_eq!(s.page,Page::Controls);
 for _ in 0..100{assert!(!s.step(CROSS));assert_eq!(s.page,Page::Controls);}
 tap(&mut s,START);assert_eq!(s.page,Page::Main);assert_eq!(s.selected,2);
 for _ in 0..100{assert!(!s.step(START));assert_eq!(s.page,Page::Main);}
 assert_eq!(MAIN_ITEMS.len(),4);assert_eq!(OPTION_ITEMS.len(),7);assert_eq!(CONTROL_LINES.len(),12);
}
#[test]fn held_directions_repeat_at_bounded_cadence_and_opposites_cancel(){
 let mut s=State::new();s.step(DOWN);assert_eq!(s.selected,1);
 for _ in 0..19{s.step(DOWN);assert_eq!(s.selected,1);}
 s.step(DOWN);assert_eq!(s.selected,2);
 for _ in 0..4{s.step(DOWN);assert_eq!(s.selected,2);}
 s.step(DOWN);assert_eq!(s.selected,3);
 for _ in 0..60{s.step(UP|DOWN);assert_eq!(s.selected,3);}
 s.step(DOWN);assert_eq!(s.selected,0);
}
#[test]fn volume_limits_are_clamped_and_other_rows_cannot_change_them(){
 let mut s=Settings::new();assert!(!s.adjust(0,1));assert!(!s.adjust(99,-1));
 for _ in 0..20{s.adjust(0,-1);}assert_eq!(s.sfx,0);assert!(!s.adjust(0,-1));assert_eq!(s.ambience,10);
 assert!(s.adjust(0,127));assert_eq!(s.sfx,10);assert!(s.adjust(1,-128));assert_eq!(s.ambience,0);
 let mut m=State::new();tap(&mut m,DOWN);tap(&mut m,CROSS);tap(&mut m,UP);
 assert_eq!(m.selected,OPTION_BACK);let old=m.settings;tap(&mut m,LEFT);assert_eq!(m.settings,old);tap(&mut m,CROSS);assert_eq!(m.page,Page::Main);
}

#[test]fn title_cheats_are_explicit_and_confirm_does_not_start_game(){
 let mut s=State::new();tap(&mut s,UP);tap(&mut s,CROSS);assert_eq!(s.page,Page::Cheats);
 assert!(!tap(&mut s,CROSS));assert!(s.settings.cheats.invincible);
 for _ in 0..100{assert!(!s.step(CROSS));}assert!(s.settings.cheats.invincible);
 tap(&mut s,DOWN);tap(&mut s,RIGHT);assert!(s.settings.cheats.max_nail);
 tap(&mut s,CIRCLE);assert_eq!(s.page,Page::Main);assert_eq!(s.selected,3);
 tap(&mut s,DOWN);assert!(!tap(&mut s,CROSS));assert!(s.settings.cheats.invincible);
}

#[test]fn music_is_independent_clamped_and_back_preserves_it(){
 let mut s=State::new();assert_eq!(s.settings.music,10);
 tap(&mut s,DOWN);tap(&mut s,CROSS);tap(&mut s,DOWN);tap(&mut s,DOWN);
 assert_eq!(s.selected,2);assert_eq!(OPTION_ITEMS[s.selected],"Music");
 for _ in 0..15{tap(&mut s,LEFT);}assert_eq!(s.settings.music,0);
 assert_eq!((s.settings.sfx,s.settings.ambience),(10,10));
 tap(&mut s,RIGHT);assert_eq!(s.settings.music,1);
 tap(&mut s,CROSS);assert_eq!(s.page,Page::Options);assert_eq!(s.settings.music,1);
 tap(&mut s,DOWN);assert_eq!(OPTION_ITEMS[s.selected],"Brightness");
 for _ in 0..3{tap(&mut s,DOWN);}assert_eq!(OPTION_ITEMS[s.selected],"Back");
 assert!(!tap(&mut s,CROSS));assert_eq!(s.page,Page::Main);assert_eq!(s.selected,1);
 tap(&mut s,CROSS);tap(&mut s,CIRCLE);tap(&mut s,UP);
 assert!(!tap(&mut s,START));assert!(tap(&mut s,CROSS));assert_eq!(s.settings.music,1);
}

#[test]fn brightness_and_screen_position_step_within_their_limits_and_print_signed(){
 let mut s=Settings::new();assert_eq!((s.brightness,s.screen_x,s.screen_y),(0,0,0));
 assert!(s.adjust(3,1));assert_eq!(s.brightness,1);assert!(s.adjust(3,-127));assert_eq!(s.brightness,-BRIGHT_STEPS);
 assert!(!s.adjust(3,-1));assert!(s.adjust(3,127));assert_eq!(s.brightness,BRIGHT_STEPS);assert!(!s.adjust(3,1));
 for row in [4,5]{
  for _ in 0..40{s.adjust(row,1);}assert!(!s.adjust(row,1));
  for _ in 0..40{s.adjust(row,-1);}assert!(!s.adjust(row,-1));
 }
 assert_eq!((s.screen_x,s.screen_y),(-SCREEN_RANGE,-SCREEN_RANGE));
 // Nothing but its own row moves a value.
 assert_eq!((s.sfx,s.ambience,s.music),(10,10,10));assert!(!s.adjust(6,1));
 let mut out=[0u8;3];
 assert_eq!(signed(0,&mut out),"0");assert_eq!(signed(3,&mut out),"+3");assert_eq!(signed(-2,&mut out),"-2");
 assert_eq!(signed(16,&mut out),"+16");assert_eq!(signed(-16,&mut out),"-16");assert_eq!(signed(-10,&mut out),"-10");
 // Through the page: Down to Brightness, Right twice.
 let mut m=State::new();tap(&mut m,DOWN);tap(&mut m,CROSS);for _ in 0..3{tap(&mut m,DOWN);}
 assert_eq!(OPTION_ITEMS[m.selected],"Brightness");tap(&mut m,RIGHT);tap(&mut m,RIGHT);assert_eq!(m.settings.brightness,2);
 assert_eq!((m.settings.sfx,m.settings.ambience,m.settings.music),(10,10,10));
}
