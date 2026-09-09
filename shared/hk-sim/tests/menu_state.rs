#[path="../../../game/src/menu_state.rs"]mod menu_state;
use menu_state::*;
fn tap(s:&mut State,b:u16)->bool{assert!(!s.step(0));s.step(b)}
#[test]fn first_confirm_preserves_direct_start_and_default_levels(){
 for b in [START,CROSS,START|CROSS,START|DOWN]{let mut s=State::new();assert!(s.step(b));assert_eq!(s.settings,Settings{sfx:10,ambience:10});}
}
#[test]fn options_apply_actual_settings_and_return_without_starting(){
 let mut s=State::new();tap(&mut s,DOWN);tap(&mut s,CROSS);assert_eq!(s.page,Page::Options);
 tap(&mut s,LEFT);assert_eq!(s.settings.sfx,9);tap(&mut s,DOWN);tap(&mut s,LEFT);assert_eq!(s.settings.ambience,9);
 tap(&mut s,CIRCLE);assert_eq!(s.page,Page::Main);assert_eq!(s.selected,1);
 tap(&mut s,UP);assert!(tap(&mut s,START));assert_eq!(s.settings,Settings{sfx:9,ambience:9});
}
#[test]fn controls_and_back_are_functional_and_confirm_never_retriggers_while_held(){
 let mut s=State::new();tap(&mut s,UP);tap(&mut s,CROSS);assert_eq!(s.page,Page::Controls);
 for _ in 0..100{assert!(!s.step(CROSS));assert_eq!(s.page,Page::Controls);}
 tap(&mut s,START);assert_eq!(s.page,Page::Main);assert_eq!(s.selected,2);
 for _ in 0..100{assert!(!s.step(START));assert_eq!(s.page,Page::Main);}
 assert_eq!(MAIN_ITEMS.len(),3);assert_eq!(OPTION_ITEMS.len(),3);assert_eq!(CONTROL_LINES.len(),9);
}
#[test]fn held_directions_repeat_at_bounded_cadence_and_opposites_cancel(){
 let mut s=State::new();s.step(DOWN);assert_eq!(s.selected,1);
 for _ in 0..19{s.step(DOWN);assert_eq!(s.selected,1);}
 s.step(DOWN);assert_eq!(s.selected,2);
 for _ in 0..4{s.step(DOWN);assert_eq!(s.selected,2);}
 s.step(DOWN);assert_eq!(s.selected,0);
 for _ in 0..60{s.step(UP|DOWN);assert_eq!(s.selected,0);}
 s.step(DOWN);assert_eq!(s.selected,1);
}
#[test]fn volume_limits_are_clamped_and_other_rows_cannot_change_them(){
 let mut s=Settings::new();assert!(!s.adjust(0,1));assert!(!s.adjust(99,-1));
 for _ in 0..20{s.adjust(0,-1);}assert_eq!(s.sfx,0);assert!(!s.adjust(0,-1));assert_eq!(s.ambience,10);
 assert!(s.adjust(0,127));assert_eq!(s.sfx,10);assert!(s.adjust(1,-128));assert_eq!(s.ambience,0);
 let mut m=State::new();tap(&mut m,DOWN);tap(&mut m,CROSS);tap(&mut m,UP);
 assert_eq!(m.selected,2);let old=m.settings;tap(&mut m,LEFT);assert_eq!(m.settings,old);tap(&mut m,CROSS);assert_eq!(m.page,Page::Main);
}
