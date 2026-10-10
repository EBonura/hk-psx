# A state-driven fight bot for boss_sim

Status: design only, nothing built.

## The problem

A boss tape is a list of pad masks, one per poll, authored against one disc at one
input phase. It is open loop: the Knight does what the tape says whether or not the
boss did what the simulator predicted. `latency2` made the failure concrete. The 50 Hz
input model moved the phase, a tick of cost moved which tick each sample lands on, and
`journey-false-knight` died on a tape that was fine the day before. The cure so far is
to find the phase (`lat/calib.py`), re-run the beam search and re-pin. That works, but
it is a manual round trip after every change that touches per-tick cost or input timing,
and the search output is a tape nobody can read: it does not say why it jumps at tick N.

A bot fixes the cause instead of the symptom. It decides each tick from what the boss
and the Knight are doing, so a shifted phase changes when it presses a button, not
whether the press is right.

## What the bot reads and returns

`Sim` already holds everything a player sees. The observation is a copy of the fields,
not a new model:

- Knight: x, y, vertical speed, grounded, facing, health, soul, whether a nail swing,
  recoil, invulnerability or Focus is in progress.
- Boss: phase (`controller.phase()`), `stunned()`, `head_exposed()`, x, y, facing
  direction, body and head hp.
- Threats: live barrels (x, y, vy), live shockwaves (x, direction), the hitter trigger.

It returns a pad mask for this tick, nothing else. No lookahead into boss RNG: the boss
and the barrel summoner are seeded, so a bot that peeks at the next roll would pass in
the sim and mean nothing on the disc. The policy sees only what is on screen.

## Policy: a priority list, not a search

Hand-written, deterministic, in this order. The first rule that fires wins.

1. Mid-recoil or mid-swing: hold the mask that finishes it (no cancel).
2. A shockwave or barrel will reach the Knight inside its travel time: jump on the
   right tick, away from the side it comes from.
3. Head exposed and the swing reaches it (`reach_gap` already computes this): swing.
4. Head exposed and out of reach: close the gap, jump when inside the vertical window.
5. Body shut and health below full, soul at the Focus cost, nothing inside the danger
   radius: Focus.
6. Otherwise hold the spacing that keeps the boss's next jump or swing short of the
   Knight, and wait.

The danger radii come from the data the sim already loads (swing reach 3.28 ahead,
0.81 up, 1.47 down), not from tuning against one phase.

## How it is used

1. Authoring. `boss_sim bot <world> <phase> <out.pxtape>` runs the policy to the end
   of the fight and writes the masks it chose. This replaces the beam search for the
   normal case. Search stays as the fallback for a fight the policy cannot clear.
2. Robustness. The point of the bot is that it passes at more than one phase, so the
   check is a grid: run it at every `BOSS_SIM_PHASE` (0 to 5) and every reload offset
   `BOSS_SIM_RELOAD` allows, and report the pass rate. A bot that clears 6 of 6 phases
   is a policy. A tape is still open loop and still has to be re-authored when the phase
   moves, but now that is one command with no search and no calibration guesswork.
3. Gate. The shipped tape is still what `validate` replays, so nothing in the gate
   changes. The bot only changes how the tape is made and how fragile the process is.

## Verification

- `tests/test_boss_sim.py` already compares the sim's transitions against a captured
  `route.csv` to within one poll. The bot adds one test: the policy clears the False
  Knight at the calibrated phase and at its two neighbours.
- Every bot tape is replayed on the disc before it is believed, as the header of
  `tools/boss_sim.rs` already requires.

## What this does not do

It does not make the disc replay closed loop. The emulator replays a fixed tape and
nothing on the disc reacts. Closing that loop (the frontend asking an external process
for each poll's mask, given the RAM words the guest already exposes) would let the bot
play the real fight and record the tape it earned, which removes sim drift entirely. That
is an emulator feature and a separate decision; this note does not depend on it.

## Open questions for Manny

- Is a hand-written policy acceptable as the tool, or should it stay a search with a
  nicer scoring function? The policy is more readable and more robust to phase; the
  search is already built.
- Does the Mawlek fight (`mawlek-fight`) want the same treatment, or is it stable enough
  to leave alone? It has one pin today.
