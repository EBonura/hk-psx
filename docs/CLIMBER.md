# Crossroads Climber controller

`shared/hk-sim/src/climber.rs` implements the source-derived controller for the
Crossroads wall-crawling enemies. Since build 94 it drives the guest actor pool
through `ActorController::Climber` (`game/src/enemies.rs` `ClimberRuntime`,
recognized by `host/climber.py`); the climb loop audio is not yet presented.
It remains independent of the existing Crawler. Source records and CIL are retained in .hkpsx/climber70/CONTRACT.md.

The controller attaches using the original two-unit point ray and assigns the
whole hit position. It moves at two units/second along cardinal directions,
checks ground before walls, and uses the source quarter-unit distance gate and
axis-drift constraints. Outside corners rotate; inside corners also interpolate
position. A completed turn snaps rotation but preserves the last interpolated
position rather than jumping to the target. Walk animation continues through
turns. The live AudioSource loop is owned separately; the source controller does
not restart or stop it on each turn or stun.

Turns take a quarter second of scaled time. Stun uses the seven-frame/12-fps
clip duration, followed by an explicit end-of-frame callback, although its
LoopSection animation can still be playing. A new accepted freeze restarts that
wait; stale completion tokens are ignored. Freeze during a turn is ignored and
death is terminal. Health, recoil, corpse and once-only Geo payout remain with
the future actor owner.

The API separates walking/ray queries, turn advancement, stun advancement and
end-of-frame resumption. Q16 tick deltas allow zero-time hit-stop and fractional
recovery; nominal helpers advance one60Hz tick. The caller must dispatch the
source's immediate first Walk iteration after attachment or stun, and preserve
the extra yielded frame after a turn. The controller does not invent a physics
step or claim Unity coroutine ordering. Point-ray geometry includes local origin,
rotation and scale for the eventual terrain-query implementation.

Original run runner-crossroads70 captures both actors with complete fields.
Its first recorded previous-position values support the attachment predictions;
they are not samples taken inside StickToGround. It also records cardinal speed
and ten outside turns; one turn
spans extra captures because game time freezes and ramps during a player hit.
Inside-corner movement and stun have source/CIL and deterministic test evidence,
but were not exercised by that run. Guest collision/rotated rendering, shared
art, audio and live reference comparisons remain required before enabling them.

Native state uses56 bytes and transient ordered actions64 bytes. All11 targeted
tests pass; evidence is .hkpsx/climber70/controller-tests-final.log. Original
trace checks and source-bound inputs are in controller-trace-proof.json in that
directory. The module allocates no heap; guest link/stack cost remains unmeasured.
