# Mossman_Runner and Mossman_Shaker source contract

Both are the Crossroads Runner's body, `Walker` and attack machinery under older or
different FSMs, so they ride `hk_sim::runner` and `host/runner.py`
(`ZombieSwipeWalker`, variants `Mossman` and `Shaker`).

## Mossman_Runner (five: Fungus1_01, 05, 06, 07 twice)

`Zombie Swipe` is the older FSM: no `Coward` state, no `TOOK DAMAGE` out of `Ready`, an
`Idle` clip in `Reset`, `Lunge Speed` 9, and a `Pt Roll` emitter in place of the Charge Dust.
Its `Walker` has no `alertRange` (the detector's one range is the `Attack Range` child the
FSM checks by name), and the IL of `Walker.UpdateWalking` turns a walking body to face a
Knight in range and in sight behind it only when `alertRange` is set, so this one walks on.
The controller is `Attack::SwipeCalm`. The Fungus1_06 one also carries `Remove if plat
fallen`, whose only collision test sends no event; it is allowed by digest. Walk 1.5,
Runner pauses, hp 15, 3 Geo, `Recoil` 10 for 0.15 s. Corpse `Corpse_Mossman_Runner`: box
0.77 by 1.05, bounce 0.2, `Death Air` and `Death Land` three frames each.

## Mossman_Shaker (eight: Fungus1_01 x2, 02, 05 x2, 06 x2, 19)

`Fungus Zombie Attack`: `Ready` (sight and the `Attack Range` box) to `Attack Delay`
(`WaitRandom` 0 to 0.75 s, the Walker still walking), `Attack Antic` (`StopWalker`, `Attack`
clip, 0.75 s), `Attack` (the `Gas Hit Box` child activated, scale 0.2 then an easeOutCirc
tween to 1 over 0.4 s, 0.8 s in all), `CD` (box off, 0.5 s), `Idle Pause` (0.5 s) and
`Reset` (`StartWalker`). It does not turn to the Knight and does not lunge. The gas box is a
trigger `PolygonCollider2D` of seven points with a `DamageHero` 1 on its own object, 1.42 below
the Shaker; the sim holds the polygon (`runner::gas`) and the recogniser proves every
placement against it. Walk 2, hp 15, 5 Geo.

Corpse `Corpse_Mossman_Shaker` is a `CorpseFungusExplode`: it lands (box 0.8 by 1.0, bounce
0.2), holds for a second, jitters 0.9 s and bursts into the same gas box.

## Not presented

The Walker's footstep loop and the attack sounds are the resident Runner bank's, as the
Leaper's are; the gas, steam and flame particles; the camera shake.
