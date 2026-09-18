use hk_sim::{Params, Player, ONE};
mod render {
    pub static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    pub static OPACITIES: std::sync::Mutex<Vec<(usize, u8)>> = std::sync::Mutex::new(Vec::new());
    pub fn set_opacity(draw: usize, alpha: u8) {
        OPACITIES.lock().unwrap().push((draw, alpha));
    }
}
#[path = "../../../game/src/reveal_masks.rs"]
mod reveal_masks;
use reveal_masks::*;
const P: Params = Params {
    speed: 0,
    jump: 0,
    gravity: 0,
    fall: 0,
    hold_ticks: 0,
    min_ticks: 0, jump_queue_ticks: 0, dash_speed: 0, dash_ticks: 0, dash_cooldown_ticks: 0, dash_queue_ticks: 0,
    half_width: ONE / 4,
    bottom: -ONE,
    top: 0,
};
const POLYGON: &[[i32; 2]] = &[
    [10 * ONE, 10 * ONE],
    [20 * ONE, 10 * ONE],
    [20 * ONE, 20 * ONE],
    [10 * ONE, 20 * ONE],
];
const SPEC: RevealMaskSpec = RevealMaskSpec {
    source_id: 12038,
    trigger: POLYGON,
    fade_ticks: 30,
    initial_opacity: 0,
};
fn inside() -> Player {
    Player::spawn(15 * ONE, 15 * ONE)
}
fn outside() -> Player {
    Player::spawn(0, 15 * ONE)
}

#[test]
fn scene_ready_idle_is_hidden_and_repeated_stay_finishes_source_30_tick_fade() {
    let mut state = State::new();
    state.scene_ready(0, &[SPEC]);
    assert_eq!(state.opacity(0), 0);
    for _ in 0..15 {
        state.tick(&[SPEC], &inside(), P);
    }
    assert_eq!(state.opacity(0), 64);
    for _ in 0..15 {
        state.tick(&[SPEC], &inside(), P);
    }
    assert_eq!(state.opacity(0), 128);
    for _ in 0..60 {
        state.tick(&[SPEC], &inside(), P);
    }
    assert_eq!(state.opacity(0), 128);
    for _ in 0..30 {
        state.tick(&[SPEC], &outside(), P);
    }
    assert_eq!(state.opacity(0), 0); // Activated never latches visibility.
}

#[test]
fn exiting_and_reentering_reverse_from_current_alpha_without_snapping() {
    let mut state = State::new();
    state.scene_ready(0, &[SPEC]);
    for _ in 0..15 {
        state.tick(&[SPEC], &inside(), P);
    }
    state.tick(&[SPEC], &outside(), P);
    assert_eq!(state.opacity(0), 62);
    state.tick(&[SPEC], &inside(), P);
    assert_eq!(state.opacity(0), 64);
    for _ in 0..29 {
        state.tick(&[SPEC], &inside(), P);
    }
    assert_eq!(state.opacity(0), 128);
}

#[test]
fn overlapping_region_rebinds_preserve_progress_scene_leave_freezes_and_reentry_resets() {
    let mut state = State::new();
    state.scene_ready(0, &[SPEC]);
    for _ in 0..15 {
        state.tick(&[SPEC], &inside(), P);
        state.scene_ready(0, &[SPEC]);
    }
    assert_eq!(state.opacity(0), 64);
    state.leave_scene();
    for _ in 0..60 {
        state.tick(&[SPEC], &outside(), P);
    }
    assert_eq!(state.opacity(0), 64);
    state.scene_ready(1, &[]);
    state.scene_ready(0, &[SPEC]);
    assert_eq!(state.opacity(0), 0);
    state.tick(&[SPEC], &inside(), P);
    state.reset_scene(0);
    state.scene_ready(0, &[SPEC]);
    assert_eq!(state.opacity(0), 0);
}

#[test]
fn trigger_uses_hero_body_and_exact_polygon_not_just_origin_or_aabb() {
    let triangle = RevealMaskSpec {
        trigger: &[
            [10 * ONE, 10 * ONE],
            [20 * ONE, 10 * ONE],
            [10 * ONE, 20 * ONE],
        ],
        ..SPEC
    };
    let mut state = State::new();
    state.scene_ready(0, &[triangle]);
    for _ in 0..30 {
        state.tick(&[triangle], &Player::spawn(19 * ONE, 19 * ONE), P);
    }
    assert_eq!(state.opacity(0), 0);
    let touching = Player::spawn(10 * ONE - ONE / 8, 12 * ONE); // Origin outside, collider overlaps.
    for _ in 0..30 {
        state.tick(&[triangle], &touching, P);
    }
    assert_eq!(state.opacity(0), 128);
}

#[test]
fn opacity_application_follows_active_draw_bindings_and_reports_current_counts() {
    let _guard = render::TEST_LOCK.lock().unwrap();
    render::OPACITIES.lock().unwrap().clear();
    let mut state = State::new();
    state.scene_ready(0, &[SPEC]);
    let first = [RevealMaskBinding {
        controller: 0,
        draw: 7,
    }];
    assert_eq!(
        state.apply(&[SPEC], first.iter().copied()),
        Applied {
            hidden: 1,
            partial: 0,
            visible: 0
        }
    );
    for _ in 0..15 {
        state.tick(&[SPEC], &inside(), P);
    }
    state.scene_ready(0, &[SPEC]);
    let overlap = [
        RevealMaskBinding {
            controller: 0,
            draw: 29,
        },
        RevealMaskBinding {
            controller: 0,
            draw: 32,
        },
    ];
    assert_eq!(
        state.apply(&[SPEC], overlap.iter().copied()),
        Applied {
            hidden: 0,
            partial: 2,
            visible: 0
        }
    );
    for _ in 0..15 {
        state.tick(&[SPEC], &inside(), P);
    }
    assert_eq!(
        state.apply(&[SPEC], first.iter().copied()),
        Applied {
            hidden: 0,
            partial: 0,
            visible: 1
        }
    );
    state.reset_scene(0);
    assert_eq!(
        state.apply(&[SPEC], first.iter().copied()),
        Applied {
            hidden: 1,
            partial: 0,
            visible: 0
        }
    );
    assert_eq!(
        *render::OPACITIES.lock().unwrap(),
        vec![(7, 0), (29, 64), (32, 64), (7, 128), (7, 0)]
    );
}

#[test]
fn pool_limit_and_scene_catalogue_mismatch_fail_explicitly() {
    let specs: [RevealMaskSpec; MAX_CONTROLLERS] = core::array::from_fn(|i| RevealMaskSpec {
        source_id: i as u32,
        ..SPEC
    });
    let mut state = State::new();
    state.scene_ready(0, &specs);
    for _ in 0..30 {
        state.tick(&specs, &inside(), P);
    }
    assert_eq!(state.opacity(15), 128);
    let too_many = [SPEC; MAX_CONTROLLERS + 1];
    assert!(std::panic::catch_unwind(|| State::new().scene_ready(0, &too_many)).is_err());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || state.scene_ready(0, &[SPEC])
    ))
    .is_err());
}

#[test]
fn maximum_u16_duration_uses_bounded_unsigned_interpolation_without_overflow() {
    let spec = RevealMaskSpec {
        fade_ticks: u16::MAX,
        ..SPEC
    };
    let mut state = State::new();
    state.scene_ready(0, &[spec]);
    for _ in 0..u16::MAX {
        state.tick(&[spec], &inside(), P);
    }
    assert_eq!(state.opacity(0), 128);
    for _ in 0..u16::MAX {
        state.tick(&[spec], &outside(), P);
    }
    assert_eq!(state.opacity(0), 0);
}

#[test]
fn ordinary_mask_starts_opaque_stay_fades_out_exit_restores_and_reentry_reverses() {
    let spec = RevealMaskSpec {
        source_id: 12056,
        initial_opacity: 128,
        ..SPEC
    };
    let mut state = State::new();
    state.scene_ready(0, &[spec]);
    assert_eq!(state.opacity(0), 128);
    for _ in 0..60 {
        state.tick(&[spec], &outside(), P);
    }
    assert_eq!(state.opacity(0), 128);
    for _ in 0..15 {
        state.tick(&[spec], &inside(), P);
    }
    assert_eq!(state.opacity(0), 64);
    state.tick(&[spec], &outside(), P);
    assert_eq!(state.opacity(0), 66);
    state.tick(&[spec], &inside(), P);
    assert_eq!(state.opacity(0), 64);
    for _ in 0..29 {
        state.tick(&[spec], &inside(), P);
    }
    assert_eq!(state.opacity(0), 0);
    for _ in 0..60 {
        state.tick(&[spec], &inside(), P);
    }
    assert_eq!(state.opacity(0), 0); // Repeated Stay cannot restart the tween.
    for _ in 0..30 {
        state.tick(&[spec], &outside(), P);
    }
    assert_eq!(state.opacity(0), 128);
}

#[test]
fn ordinary_mask_progress_survives_region_rebinding_and_reset_apply_uses_authored_idle() {
    let _guard = render::TEST_LOCK.lock().unwrap();
    let spec = RevealMaskSpec {
        source_id: 12037,
        initial_opacity: 128,
        ..SPEC
    };
    let mut state = State::new();
    state.scene_ready(0, &[spec]);
    for _ in 0..15 {
        state.tick(&[spec], &inside(), P);
        state.scene_ready(0, &[spec]);
    }
    assert_eq!(state.opacity(0), 64);
    state.leave_scene();
    for _ in 0..30 {
        state.tick(&[spec], &outside(), P);
    }
    assert_eq!(state.opacity(0), 64);
    state.reset_scene(0);
    let applied = state.apply(
        &[spec],
        [RevealMaskBinding {
            controller: 0,
            draw: 12,
        }]
        .into_iter(),
    );
    assert_eq!(
        applied,
        Applied {
            hidden: 0,
            partial: 0,
            visible: 1
        }
    );
    state.scene_ready(0, &[spec]);
    assert_eq!(state.opacity(0), 128);
}
