# behaviour-parity

The guest's enemy module against the original, scene by scene.

`game/src/enemies.rs` is compiled natively (with the hardware-free stand-ins in
`shared/hk-sim/tests/common/enemy_stubs.rs`, which `tests/enemy_runtime.rs` shares) and run over the real
cooked rooms, scene banks and actor catalogue. Its per-tick state is compared with per-frame traces of
the original that `tools/hkref scenes` records (hero frozen at the entrance, the camera and the Knight
replayed from the original's own trace).

It builds as a test target without the libtest harness, because the guest modules carry the `cfg(test)`
seams their own tests use (a per-thread scene bank, no hardware calls):

```sh
export HKBP_REGIONS_RS=<cooked data>/regions.rs   # build time: the ActorSpec catalogue is lifted from it
export HKBP_DATA=<cooked data>                    # regions.json and regions/chunk_N.hk
export HKBP_BANKS=<.hkpsx>/world-metadata-packed  # scene_N.hkwm
export HKBP_SCENE_NAMES=<json {"scene id": "Scene_Name"}>
cd tools/behaviour-parity
cargo test --release --test behaviour -- static                    # catalogue numbers against the source records
cargo test --release --test behaviour -- compare RUN [SCENE]       # idle patrol against the original
cargo test --release --test behaviour -- poke RUN [SCENE]          # nail strikes and recoil against the original
cargo test --release --test behaviour -- dump RUN SCENE SOURCE_ID FROM TO STEP   # one pair, side by side
cargo test --release --test behaviour -- probe SCENE_ID [TICKS]    # port only
cargo test --release --test behaviour -- edges SCENE_ID X0 Y0 X1 Y1
```

`RUN` is a `tools/hkref scenes` run directory. The idle comparison needs a survey run with a long
`sweep_frames` and `actor_stride` 1; the poke comparison needs one with `og.env`
`{"HK_REFERENCE_SURVEY_POKE": "60"}` (the survey then strikes each distinct enemy instead of touring).
`HKBP_ALL=1` prints every pair instead of the failures.

What the idle comparison can and cannot say: the original's random choices (pauses, flight angles)
differ from the port's deterministic ones, so each pair is judged on the envelope and speed of its
motion over the window, and the family means are the better evidence for the random flyers. The
camera trace includes the scene load because several enemies latch a start condition on it.
