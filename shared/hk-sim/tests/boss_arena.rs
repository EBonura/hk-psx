use hk_sim::boss::*;

/// One visit, from the hero crossing the trigger to the gates reopening.
#[test]
fn gates_close_on_entry_and_open_two_seconds_after_the_last_death() {
    let mut arena = Arena::new(false);
    assert_eq!(arena.phase(), Phase::Waiting);
    let enter = arena.enter();
    assert!(enter.contains(Action::CameraLock(false)));
    assert!(!enter.contains(Action::QuickOpenGates));

    let start = arena.hero_entered();
    assert_eq!(arena.phase(), Phase::Fighting);
    assert_eq!(arena.enemies(), 1);
    assert!(start.contains(Action::CloseGates));
    assert!(start.contains(Action::StartBattle));
    assert!(start.contains(Action::CameraLock(true)));

    // The room's ordinary enemies die when the boss lands, and only then can
    // the every-frame counter comparison in `Kill Zombies` run at all.
    assert!(arena.tick().is_empty());
    let cleared = arena.kill_all_enemies();
    assert!(cleared.contains(Action::KillMinions));
    assert_eq!(arena.phase(), Phase::Clearing);
    assert!(arena.tick().is_empty());

    arena.enemy_died();
    let ending = arena.tick();
    assert_eq!(arena.phase(), Phase::Ending);
    assert!(ending.contains(Action::Persist));
    assert!(
        arena.activated(),
        "the save taken during the wait must count as cleared"
    );
    for _ in 0..END_WAIT_TICKS - 1 {
        assert!(arena.tick().is_empty());
        assert_eq!(arena.phase(), Phase::Ending);
    }
    let opened = arena.tick();
    assert_eq!(arena.phase(), Phase::Open);
    assert!(opened.contains(Action::OpenGates));
    assert!(arena.tick().is_empty());
}

/// Reload after the fight: the gates are placed open with no fight and no
/// animation, and the floor the boss broke is activated.
#[test]
fn an_activated_arena_quick_opens_on_reload_and_never_fights_again() {
    let mut arena = Arena::new(true);
    assert_eq!(arena.phase(), Phase::Open);
    let enter = arena.enter();
    assert!(enter.contains(Action::QuickOpenGates));
    assert!(enter.contains(Action::ActivateFloor));
    assert!(!enter.contains(Action::CloseGates));
    assert!(arena.hero_entered().is_empty());
    assert_eq!(arena.phase(), Phase::Open);
    for _ in 0..600 {
        assert!(arena.tick().is_empty());
    }
}

/// A save taken mid-fight reloads as an unfought arena, which is what the
/// source's `Activated` bool means: the gates reopen and the battle restarts.
#[test]
fn a_save_taken_mid_fight_reloads_as_an_unfought_arena() {
    let mut arena = Arena::new(false);
    arena.enter();
    arena.hero_entered();
    arena.kill_all_enemies();
    assert!(!arena.activated());
    let reloaded = Arena::new(arena.activated());
    assert_eq!(reloaded.phase(), Phase::Waiting);
    assert!(reloaded.enter().contains(Action::CameraLock(false)));
}

/// `Start` has no every-frame counter comparison; only `Kill Zombies` does, so
/// an arena whose boss dies before it clears the room must not open early.
#[test]
fn the_arena_cannot_end_before_the_boss_clears_the_room() {
    let mut arena = Arena::new(false);
    arena.enter();
    arena.hero_entered();
    arena.enemy_died();
    for _ in 0..600 {
        assert!(arena.tick().is_empty());
        assert_eq!(arena.phase(), Phase::Fighting);
    }
    arena.kill_all_enemies();
    assert!(arena.tick().contains(Action::Persist));
}

/// The trigger only arms the fight once.
#[test]
fn re_entering_the_trigger_during_the_fight_does_nothing() {
    let mut arena = Arena::new(false);
    arena.enter();
    arena.hero_entered();
    assert!(arena.hero_entered().is_empty());
    assert_eq!(arena.enemies(), 1);
}
