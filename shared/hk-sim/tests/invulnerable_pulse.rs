use hk_sim::{Hurt, VitalParams, Vitals, PULSE_TICKS};
const P: VitalParams = VitalParams {
    max_health: 5,
    max_soul: 99,
    nail_damage: 5,
    soul_per_hit: 11,
    invulnerable_ticks: 79,
    hazard_invulnerable_ticks: 40,
    recoil_ticks: 12,
    freeze_ticks: 19,
    death_ticks: 171,
    recoil_speed: 15 * 65536,
};

/// InvulnerablePulse.Update stepped once per tick, as the original does per
/// frame: timer += dt up to pulseDuration (clamp, turn), -= dt down to 0
/// (clamp, turn).
fn reference(steps: u16) -> u16 {
    let (mut timer, mut down) = (0i32, false);
    let p = PULSE_TICKS as i32;
    for _ in 0..steps {
        if !down {
            timer += 1;
            if timer > p {
                timer = p;
                down = true;
            }
        } else {
            timer -= 1;
            if timer < 0 {
                timer = 0;
                down = false;
            }
        }
    }
    timer as u16
}

#[test]
fn pulse_follows_simulation_ticks_and_pauses_in_the_freeze() {
    let mut v = Vitals::new(P);
    assert_eq!(v.invulnerable_pulse(), 0);
    assert_eq!(v.hurt(P, 1, 1, false), Hurt::Recoiling);
    // The damage freeze holds the invulnerability, and the pulse with it.
    for _ in 0..P.freeze_ticks {
        assert_eq!(v.invulnerable_pulse(), 0);
        assert!(v.tick());
    }
    let mut seen = Vec::new();
    while v.invulnerable_ticks != 0 {
        assert!(!v.tick());
        seen.push(v.invulnerable_pulse());
    }
    // One freeze-down tick, then one pulse step per tick until it ends.
    let expected: Vec<u16> = (1..=P.invulnerable_ticks)
        .map(|n| {
            if n == P.invulnerable_ticks {
                0
            } else {
                reference(n - 1)
            }
        })
        .collect();
    assert_eq!(seen, expected);
    assert!(seen.contains(&PULSE_TICKS));
}

#[test]
fn hazard_respawn_pulses_from_its_own_start() {
    let mut v = Vitals::new(P);
    assert_eq!(v.hurt(P, 1, 1, true), Hurt::Hazard);
    v.finish_hazard_respawn(P);
    v.tick();
    v.tick();
    v.tick();
    assert_eq!(v.invulnerable_pulse(), reference(2));
}
