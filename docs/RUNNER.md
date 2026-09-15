# Crossroads Zombie Runner implementation

The controller in `shared/hk-sim/src/runner.rs` is source preparation for the
Crossroads_01 enemies. It is not connected to the guest actor pool or present in
the current provisional69 disc. Crawler behavior is unchanged.

The supported original variant has Coward=false and Reverse=false. Its Walker
pauses and turns at walls/holes; its separate Swipe FSM attacks when both the
alert-range and terrain line-of-sight queries pass, or on an accepted damage
event. Nominal movement speeds are1.5 units/s walking and6 during a lunge.
The caller supplies animation-completion tokens, preserving the original
anticipation/lunge/cooldown sequence instead of advancing it with a second
unrelated timer. Source clip durations at60Hz are25/40/5 ticks, followed by a
15-tick idle. Damage precedes horizontal-recoil reset; vertical recoil does not
trigger that reset. Death is terminal.

`step_walker` and `step_swipe` expose the independent component callbacks so
reference traces can select their observed order. The convenience `step` uses
Walker then Swipe as a deterministic guest policy; original Unity execution
order and50Hz physics phases remain unverified. Source StartMoving can also
reenter Walker.Update synchronously; the isolated model defers that additional
sensing to the scheduled Walker callback, pending reference integration. Pause selection is injected,
with the source's reversed authored endpoints retained. This is not a claim of
Unity random-number or trajectory equivalence.

The state uses20 bytes in the native test build; ordered transient actions use
196 bytes. No heap is used by the controller. A guest link and stack budget are
still required when actor integration makes the code reachable on PS1.

Before enabling these enemies, implement and verify the transformed AlertRange,
three-ray terrain Sweep and LOS; bind shared animation art, health/recoil,
corpse and Geo ownership; bind footsteps/chase/damage/death sounds and Charge
Dust. The two instances share one animation library. Unsupported variants must
remain explicit. Native tests exercise source state/event contracts, not full
original physics or audible/visual parity.

Local source evidence and reproduction live in
`.hkpsx/crossroads68/RUNNER-CONTRACT.md`, `runner-provenance.json` and
`check_runner_contract.py`; retail records and decompilation stay ignored.
