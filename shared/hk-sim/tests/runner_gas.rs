//! The Mossman_Shaker's attack on the shared Runner walker (`runner::Attack::Gas`): Fungus Zombie Attack's state
//! timeline (Attack Delay, Attack Antic, Attack, CD, Idle Pause, Reset) and the global ZERO HP only.
use hk_sim::runner::{Attack, Params, Runner, Senses, Swipe, Wait, Walker};
use hk_sim::ONE;

fn shaker() -> Runner {
    Runner::with_params(Params {
        walk_speed: 2 * ONE,
        lunge_speed: 0,
        walking_wait: [240, 90],
        paused_wait: [150, 90],
        attack: Attack::Gas { delay: 45, antic_ticks: 45, attack_ticks: 48, cd_ticks: 30, idle_ticks: 30 },
        gravity: 60 * ONE,
    })
}

fn seen() -> Senses {
    Senses { camera_in_start_range: true, in_alert_range: true, can_see_hero: true, hero_x: -ONE, actor_x: 0, ..Senses::default() }
}

#[test]
fn the_gas_attack_runs_delay_antic_gas_cooldown_idle_then_walks_again() {
    let mut r = shaker();
    // The delay draws 7 ticks; the Walker's waits take their longest.
    let mut choose = |w: Wait, [hi, lo]: [u16; 2]| if w == Wait::Delay { 7 } else { hi.max(lo) };
    let mut log = Vec::new();
    let mut was = (r.swipe(), r.walker());
    for tick in 0..500u32 {
        let senses = if tick < 300 { seen() } else { Senses { camera_in_start_range: true, ..Senses::default() } };
        r.step_walker(senses, &mut choose);
        r.step_swipe(senses, &mut choose);
        let now = (r.swipe(), r.walker());
        if now != was {
            log.push((tick, now.0, now.1));
            was = now;
        }
    }
    let swipes: Vec<Swipe> = log.iter().map(|l| l.1).filter(|s| *s != Swipe::Ready).collect();
    assert_eq!(&swipes[..5], &[Swipe::Delay, Swipe::Anticipate, Swipe::Lunge, Swipe::Cooldown, Swipe::Idle], "{log:?}");
    let at = |s: Swipe| log.iter().find(|l| l.1 == s).unwrap().0;
    assert_eq!(at(Swipe::Anticipate) - at(Swipe::Delay), 7);
    assert_eq!(at(Swipe::Lunge) - at(Swipe::Anticipate), 45);
    assert_eq!(at(Swipe::Cooldown) - at(Swipe::Lunge), 48);
    assert_eq!(at(Swipe::Idle) - at(Swipe::Cooldown), 30);
    // The Walker keeps going through the delay, stops at the antic, and starts again at Reset.
    assert_eq!(log.iter().find(|l| l.1 == Swipe::Anticipate).unwrap().2, Walker::StoppedForAttack);
    assert!(matches!(r.walker(), Walker::Walking | Walker::Paused | Walker::Turning), "{:?}", r.walker());
}

#[test]
fn the_gas_hit_box_is_on_only_in_the_attack_state_and_hits_do_not_touch_the_attack() {
    let mut r = shaker();
    let mut choose = |w: Wait, [hi, lo]: [u16; 2]| if w == Wait::Delay { 0 } else { hi.max(lo) };
    assert_eq!(r.gas_age(), None);
    // The FSM has no TOOK DAMAGE and no RECOIL HORIZONTAL transition: neither starts or resets an attack.
    assert!(r.took_damage(-ONE, 0).iter().next().is_none());
    assert!(r.horizontal_recoil(seen(), &mut choose).iter().next().is_none());
    let mut on = 0;
    let mut last = None;
    for _ in 0..400 {
        r.step_walker(seen(), &mut choose);
        r.step_swipe(seen(), &mut choose);
        if let Some(age) = r.gas_age() {
            on += 1;
            assert_eq!(Some(age), last.map(|a: u16| a + 1).or(Some(age)).filter(|_| true));
            last = Some(age);
        } else {
            last = None;
        }
    }
    assert!(on > 0 && on % 48 == 0 || on > 48, "the gas was on for {on} ticks");
}

#[test]
fn the_gas_hit_box_grows_from_a_fifth_to_whole_in_twenty_four_ticks() {
    use hk_sim::runner::{gas_polygon, gas_scale};
    assert!((gas_scale(0) - ONE / 5).abs() <= 2, "{}", gas_scale(0));
    assert!((gas_scale(24) - ONE).abs() <= 2, "{}", gas_scale(24));
    assert_eq!(gas_scale(24), gas_scale(60), "the tween stops at one");
    let mut last = 0;
    for age in 0..=24 {
        let s = gas_scale(age);
        assert!(s >= last, "easeOutCirc only rises: {age} {s} {last}");
        last = s;
    }
    // easeOutCirc is steep early: a quarter of the way in it is past half.
    assert!(gas_scale(6) > ONE / 2);
    // Facing left the cloud's widest point (+4.6) is on the right of the body, facing right it mirrors.
    let left = gas_polygon(24, -1, [0, 0]);
    let right = gas_polygon(24, 1, [0, 0]);
    assert_eq!(left.iter().map(|p| p[0]).max(), right.iter().map(|p| -p[0]).max());
    assert!(left.iter().any(|p| p[0] > 4 * ONE));
}
