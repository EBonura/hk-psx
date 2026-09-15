#[path="../../../game/src/focus_audio_state.rs"]mod audio;
use hk_sim::{Focus,FocusClip,FocusInput,FocusParams,VitalParams,Vitals};
const P:FocusParams=FocusParams {hold_ticks:15,start_ticks:15,heal_ticks:12,cancel_ticks:15,finish_ticks:14,
    first_grace_ticks:12,repeat_grace_ticks:27,drain_interval_us:27000,cost:33,heal_amount:1,attack_recovery_ticks:6};
const V:VitalParams=VitalParams {max_health:5,max_soul:99,nail_damage:5,soul_per_hit:11,invulnerable_ticks:79,
    hazard_invulnerable_ticks:40,recoil_ticks:12,freeze_ticks:19,death_ticks:171,recoil_speed:15*65536};
fn phase(f:&Focus)->audio::Phase {match f.animation(){
    None=>audio::Phase::Off,Some((FocusClip::End|FocusClip::GetOnce,age))=>audio::Phase::Fade(age),
    Some(_)=>audio::Phase::Charge,
}}
#[test]fn repeated_heals_keep_one_charge_and_two_tail_voices_suffice(){
    let mut f=Focus::new();let mut a=audio::State::new();let mut v=Vitals::new(V);v.health=2;v.soul=99;
    let(mut starts,mut stops)=(0,0);let mut heals=Vec::new();
    for tick in 0..300 {
        let e=f.step(P,V,FocusInput{held:true,grounded:true,can_start:true},&mut v);
        let s=a.step(phase(&f),e.started,e.completed,20);
        starts+=usize::from(s.start);stops+=usize::from(s.stop);if s.heal {heals.push(tick);}
    }
    assert_eq!((starts,stops),(1,1));assert_eq!(heals.len(),3);assert_eq!(v.health,5);
    // Complete heal is1.567s (95 ticks). Alternating two voices cannot steal
    // a still-playing tail at the no-charm repeated-heal cadence.
    assert!(heals.windows(3).all(|h|h[2]-h[0]>=95));assert_eq!(a.gain(),0);
}
#[test]fn full_health_completion_still_plays_and_release_truncates_fade(){
    let mut f=Focus::new();let mut a=audio::State::new();let mut v=Vitals::new(V);v.soul=99;
    let mut heals=0;let mut saw_fade=false;
    for _ in 0..140 {
        let e=f.step(P,V,FocusInput{held:true,grounded:true,can_start:true},&mut v);
        assert_eq!(e.healed,0);
        let s=a.step(phase(&f),e.started,e.completed,20);heals+=usize::from(s.heal);
        if s.gain>0&&s.gain<128 {saw_fade=true;}
        if s.stop {assert!(saw_fade);assert_eq!(s.gain,0);}
    }
    assert_eq!(heals,1);assert_eq!(a.gain(),0);
}
#[test]fn cancellation_and_damage_never_generate_heal_or_leave_charge_on(){
    for damage in [false,true] {
        let mut f=Focus::new();let mut a=audio::State::new();let mut v=Vitals::new(V);v.soul=99;
        for tick in 0..60 {
            let e=f.step(P,V,FocusInput{held:tick<35,grounded:true,can_start:true},&mut v);
            if tick==35&&damage {f.interrupt();}
            let s=a.step(phase(&f),e.started,e.completed,20);assert!(!s.heal);
            if tick==35&&damage {assert!(s.stop);assert_eq!(s.gain,0);}
        }
        assert_eq!(a.gain(),0);
    }
}
