# MossmanRunner source adoption boundary

This is one enemy family and five selected instances in already admitted scenes. It does not admit Fungus1_31, Mossman_Shaker, FatFly, PlantTrap, MossCharger or any boss. Current generated port rows explicitly report unsupported movement and unverified Runner FSM variants for all five selected instances.

| Scene and current scene ID | Original GameObject | Zombie Swipe FSM | Canonical HealthManager/spec source |
| --- | --- | --- | --- |
| Fungus1_01, 46 | level128:736 | level128:7539 | level128:7923 |
| Fungus1_05, 49 | level134:110 | level134:6132 | level134:6465 |
| Fungus1_06, 50 | level135:808 | level135:6450 | level135:6820 |
| Fungus1_07, 51 | level136:510 | level136:5414 | level136:5743 |
| Fungus1_07, 51 | level136:1082 | level136:5445 | level136:5742 |

The Rust `mossman-runner-preflight` tool pins each original FSM raw SHA, verifies the original HealthManager belongs to the expected GameObject and matches current generated source/spec keys. It reports original Walker, health, recoil, physics, activation and animation metadata plus current actor bank sizes. The output is source evidence, not runtime admission. It writes via a temporary file and publishes only after the entire audit succeeds. Source/hash drift and mismatched canonical mappings fail closed. The sixth original Mossman_Runner in Fungus1_31 is outside the current admitted scope; the generated port's scene table does not include that scene.

The original five movement graphs have Ready with only ATTACK ALERT -> Left or Right?, no TOOK DAMAGE transition, no Coward state, and authored Lunge Speed9. Fungus1_06 also has Remove if plat fallen, requiring a separate exact source binding and event audit rather than accepting arbitrary extra FSMs. The parked branch's claim that its empty sendEvent prevents firing remains a contract to verify, not an automatic exemption for all platform FSMs.

The selected forward adoption needs these separate checks against current Rust:

1. `host/hk-cook/src/runner.rs`: accept only the audited Mossman movement fingerprint, exact enabled/activator contract, null Walker alertRange and the uniquely bound Attack Range child/LineOfSightDetector. Validate every extra FSM. Preserve existing Runner/Leaper fingerprint pins and sensing behaviour. The parked branch structural fingerprint is7353668aa0259b3ddcfa9046aa4533db332b1573bf02324d83bb5c35e1316b04; a fresh current recognizer preflight must reproduce it rather than trusting the old result.
2. `shared/hk-sim/src/runner.rs`: add only SwipeCalm. It shares ordinary lunge animation timing but ignores Ready TOOK DAMAGE and does not turn its Walker toward a hero behind it because the Walker has no alertRange. Horizontal recoil still resets the swipe controller. Do not include Shaker gas states, counters or polygons.
3. `host/hk-cook/src/actor_specs.rs`: map the explicitly recognized SwipeCalm kind to the new Rust attack variant, never fall back silently to Swipe. Retain the existing Runner ActorController and corpse requirements.
4. `host/hk-cook/src/actor_art.rs`: the current ordinary Runner slots already name Walk, Turn, Idle, Attack Anticipate, Attack Lunge, Attack Cooldown and optional Fall. Audit the original Mossman library's clip modes and Death Air/Death Land corpse clips; no five-family actor art replacement is needed. The parked branch's nine clip-name/mode checks are prospective gates, not measured cooked bank bytes.

No new bank, art, code, static RAM or total disc growth has been measured. Adding five selected source placements is not five new global enemy types. Actor banks remain bounded by the existing256KiB policy and tile/CLUT limits; current linked margin cannot be treated as permission to add an unmeasured family. The preflight makes no terrain recook and does not modify any source data or generated assets.

Next validation: compile the small host preflight against retained dependencies under the shared serial lock and a fresh fit estimate, run against the read-only Steam2 data and source-bound current regions.json, then review the exact Walker/activation/library contracts. After approval, implement only the scoped Rust gates and SwipeCalm, run focused native controller fixtures, and cook only recognition/art/spec for the four selected scenes. Run ordinary-health encounters with earned nail hits, soul/Focus and death/respawn, including behind approach and nonlethal hit while Ready. No invincible aggregate coverage substitutes for these routes.

Manual-only save policy is mandatory. This concern changes no automatic settings, grant, checkpoint or transition card write paths; future integration must retain the manual-only Shaman wake fix and explicit bench Save interaction.
