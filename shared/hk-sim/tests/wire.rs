//! The wire form of ActorSpec/CorpseSpec: every controller variant round-trips field by field, a
//! corpse reference resolves to the same record, and malformed bytes are refused.
use hk_sim::runner::{Attack, Params};
use hk_sim::wire::{
    decode_corpse, decode_spec, encode_corpse, encode_spec, Reader, Writer, MAX_SPEC_BYTES,
    NO_CORPSE,
};
use hk_sim::{ActorController as C, ActorSpec, CorpseSpec, EnemyParams, WalkParams};

static CORPSES: [CorpseSpec; 2] = [corpse(1), corpse(2)];

const fn corpse(n: i32) -> CorpseSpec {
    CorpseSpec {
        air_clip: n as u16,
        land_clip: n as u16 + 1,
        bounds: [-n, n + 1, n + 2, -n - 3],
        spawn_offset: [n * 7, -n * 9],
        bounce_factor: 19661 + n,
        fling_speed: 983040,
        gravity: 3145728,
        breaker: n == 2,
        smash_bounces: 3,
        remove_after_land: 77,
        hold_ticks: 5,
    }
}

fn spec(controller: C, corpse: Option<&'static CorpseSpec>) -> ActorSpec {
    ActorSpec {
        controller,
        bounds: [-45056, -69632, 48128, -10240],
        health: EnemyParams {
            health: -3,
            contact_damage: 2,
            evasion_ticks: 12,
            invincible: true,
            damage_override: false,
        },
        walk: WalkParams { speed: -65536, turn_ticks: 9, turn_cooldown_ticks: 60 },
        walk_clip: 19,
        turn_clip: 20,
        corpse,
        recoil_speed: 983040,
        recoil_ticks: 9,
        dream_soul: 33,
    }
}

fn controllers() -> Vec<C> {
    vec![
        C::Crawler { recoil_steps: 8 },
        C::Mawlek { wake: [1, -2, 3, -4] },
        C::GruzMother,
        C::Runner {
            idle_clip: 1,
            anticipate_clip: 2,
            lunge_clip: 3,
            cooldown_clip: 4,
            params: Params::RUNNER,
            alert: [5, 6, -7, 8],
        },
        C::Runner {
            idle_clip: 1,
            anticipate_clip: 2,
            lunge_clip: 3,
            cooldown_clip: 4,
            params: Params::LEAPER,
            alert: [5, 6, -7, 8],
        },
        C::Runner {
            idle_clip: 9,
            anticipate_clip: 8,
            lunge_clip: 7,
            cooldown_clip: 6,
            params: Params { attack: Attack::SwipeCalm, ..Params::RUNNER },
            alert: [0, 0, 0, 0],
        },
        C::Runner {
            idle_clip: 3,
            anticipate_clip: 4,
            lunge_clip: 5,
            cooldown_clip: 6,
            params: Params {
                attack: Attack::Gas { delay: 45, antic_ticks: 45, attack_ticks: 48, cd_ticks: 30, idle_ticks: 30 },
                ..Params::RUNNER
            },
            alert: [5, -6, 7, 8],
        },
        C::Climber { stun_clip: 11 },
        C::Vengefly { startle_clip: 23, chase_clip: 24, turn_fly_clip: 25 },
        C::Gruzzer,
        C::GruzzerReserve { origin: [1 << 20, -(1 << 18)] },
        C::AcidFlyer { amount: -5, speed: 6, lead: [7, -8], shell: [9, 10, 11, -12] },
        C::Mosquito { clips: [1, 2, 3, 4], tile: [-1, -2, -3, -4] },
        C::MossWalker { clips: [4, 3, 2, 1] },
        C::Baldur { start_clip: 1, roll_clip: 2, stop_clip: 3 },
        C::Aspid { fire_clip: 1, shot_clip: 2, impact_clip: 3 },
        C::Hatcher { fire_clip: 29, max_hatched: 5 },
        C::HatcherBaby,
        C::ZombieShield { clips: core::array::from_fn(|i| i as u16 * 3), attack: [1, 2, 3, 4] },
        C::HuskGuard {
            clips: core::array::from_fn(|i| 100 + i as u16),
            spurt_clip: 5,
            slam_clip: 6,
        },
        C::Blocker {
            clips: core::array::from_fn(|i| 200 + i as u16),
            shot_clip: 1,
            impact_clip: 2,
            sleeps: true,
        },
        C::Pigeon { clips: [3, 4] },
        C::FsmActor { family: 7, clips: core::array::from_fn(|i| 300 + i as u16), trigger: [1, 2, 3, 4] },
        C::Static { idle_clip: 42 },
        C::FalseKnight {
            jump_antic_clip: 1,
            land_clip: 2,
            stun_opened_clip: 3,
            attack_clip: 4,
            trigger: [10, 20, 30, 40],
            barrel_clip: 5,
            barrel_spawn_y: -123456,
        },
    ]
}

fn round_trip(spec: &ActorSpec, index: u8) -> ActorSpec {
    let mut buf = [0u8; MAX_SPEC_BYTES];
    let mut w = Writer::new(&mut buf);
    encode_spec(spec, index, &mut w);
    let len = w.finish().expect("spec fits MAX_SPEC_BYTES");
    let mut r = Reader::new(&buf[..len]);
    let out = decode_spec(&mut r, &CORPSES).expect("decodes");
    assert_eq!(r.pos(), len, "decoder consumed exactly what the encoder wrote");
    out
}

#[test]
fn every_controller_round_trips() {
    for (n, controller) in controllers().into_iter().enumerate() {
        for (index, corpse) in [(NO_CORPSE, None), (1, Some(&CORPSES[1]))] {
            let original = spec(controller, corpse);
            let back = round_trip(&original, index);
            assert_eq!(format!("{original:?}"), format!("{back:?}"), "variant {n}");
        }
    }
}

#[test]
fn corpse_reference_resolves_into_the_given_list() {
    let back = round_trip(&spec(C::Gruzzer, Some(&CORPSES[0])), 0);
    assert!(core::ptr::eq(back.corpse.unwrap(), &CORPSES[0]));
}

#[test]
fn corpse_round_trips() {
    let mut buf = [0u8; 64];
    let mut w = Writer::new(&mut buf);
    encode_corpse(&CORPSES[1], &mut w);
    let len = w.finish().unwrap();
    assert_eq!(len, hk_sim::wire::CORPSE_BYTES);
    let back = decode_corpse(&mut Reader::new(&buf[..len]));
    assert_eq!(format!("{:?}", CORPSES[1]), format!("{back:?}"));
}

#[test]
fn malformed_bytes_are_refused() {
    let mut buf = [0u8; MAX_SPEC_BYTES];
    let mut w = Writer::new(&mut buf);
    encode_spec(&spec(C::Gruzzer, None), NO_CORPSE, &mut w);
    let len = w.finish().unwrap();
    // Short.
    assert!(decode_spec(&mut Reader::new(&buf[..len - 1]), &CORPSES).is_none());
    // Unknown controller tag.
    let mut bad = buf;
    bad[0] = 200;
    assert!(decode_spec(&mut Reader::new(&bad[..len]), &CORPSES).is_none());
    // Corpse index past the list.
    let mut w = Writer::new(&mut bad);
    encode_spec(&spec(C::Gruzzer, None), 9, &mut w);
    assert!(decode_spec(&mut Reader::new(&bad[..len]), &CORPSES).is_none());
}

#[test]
fn chunk_round_trips_and_checks_its_frame() {
    let specs: Vec<ActorSpec> = controllers()
        .into_iter()
        .enumerate()
        .map(|(n, c)| spec(c, [None, Some(&CORPSES[0]), Some(&CORPSES[1])][n % 3]))
        .collect();
    let index = |s: &ActorSpec| match s.corpse {
        None => NO_CORPSE,
        Some(c) if std::ptr::eq(c, &CORPSES[0]) => 0,
        Some(_) => 1,
    };
    let refs: Vec<(&ActorSpec, u8)> = specs.iter().map(|s| (s, index(s))).collect();
    let mut buf = vec![0u8; 4096];
    let len = hk_sim::wire::encode_chunk(7, &refs, &[&CORPSES[0], &CORPSES[1]], &mut buf).unwrap();
    let head = hk_sim::wire::read_chunk_head(&buf[..len]).unwrap();
    assert_eq!((head.scene_id, head.specs, head.corpses), (7, specs.len(), 2));
    // A flipped payload byte fails the checksum; padding after the frame is not covered.
    let mut bad = buf.clone();
    bad[30] ^= 1;
    assert!(hk_sim::wire::read_chunk_head(&bad).is_none());
    buf[len + 10] = 0x55;
    assert!(hk_sim::wire::read_chunk_head(&buf).is_some());
    let corpse_pool: &'static mut [CorpseSpec] = Box::leak(vec![corpse(0); 4].into_boxed_slice());
    let mut spec_pool = vec![spec(C::Gruzzer, None); 40];
    let head = hk_sim::wire::decode_chunk(&buf, corpse_pool, &mut spec_pool).unwrap();
    for (a, b) in specs.iter().zip(&spec_pool[..head.specs]) {
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }
    // A pool that is too small is refused, not overrun.
    let small: &'static mut [CorpseSpec] = Box::leak(vec![corpse(0); 1].into_boxed_slice());
    assert!(hk_sim::wire::decode_chunk(&buf, small, &mut spec_pool).is_none());
}
