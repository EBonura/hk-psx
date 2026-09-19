# Damage resolution

P15 step 4 asks for damage resolution to be centralized: hit source and type,
damage amount, invulnerability, contact rules, hit-stop, multi-hit cooldown and
death ordering. This is the audit of what the port does today, written before
changing anything, plus the one defect the audit found.

## Where damage is decided

Two separate systems, correctly separate.

**The Knight taking damage** goes through `Vitals::hurt`, which owns the whole
rule set: blue masks absorb first, the invulnerability window refuses a second
hit, `freeze_ticks` is the hit-stop, `recoil_ticks` and `recoil_speed` are the
knockback, `hazard_pending` routes a hazard to its respawn marker instead of a
recoil, and `death_ticks` orders the death. Every caller reaches it through
`cheats::Settings::hurt`, which layers two things on top: the Invincibility
cheat, which keeps hazard recovery but takes no damage, and the Shade Cloak,
which returns `Hurt::Ignored` outright because the source's `TakeDamage` returns
immediately while `cState.shadowDashing`.

**An enemy taking damage** goes through `ActorHealth::hit`, which owns its own
rules: `evasion_ticks` is the multi-hit cooldown, `invincible` blocks, and
`damage_override` clamps to one. `SubtractHealth` clamps overkill at -50 rather
than wrapping, which the port matches.

## Every path that hurts the Knight

| Source | Call site | Amount |
| --- | --- | --- |
| Enemy contact | `enemies.rs`, in the actor loop | the actor's `contact_damage` |
| Enemy projectile | `enemies.rs`, through `tick_shots` | 1 |
| Static hazard volume | `main.rs`, after `world::hazard_contact` | the cooked volume's |
| The Hollow Shade | `main.rs`, on `shade_events.touched` | 1 |
| Falling out of the world | `main.rs`, when no region contains the Knight | 1, as a hazard |

## The defect

Four of those five call sites run the same response after a hit that was not
ignored:

    dialogue::cancel(); audio::hurt(); nail = Nail::new();
    nail_response = NailResponse::new(); focus.interrupt(); focus_audio::interrupt();

The fifth, falling out of the world, omits `dialogue::cancel()`. So a dialogue
open when the Knight leaves the world survives the hazard respawn, while the
same dialogue is cancelled by every other kind of damage. It is a narrow window,
since a dialogue is only open at a tablet or an NPC, but it is reachable and it
is wrong.

That is not a typo worth fixing in isolation. It is what a duplicated response
block produces: five copies, and the fifth drifted. The centralization step 4
asks for is the fix, because a new damage source then cannot forget a step.

## What it centralizes on

Two functions in `main.rs`, and no damage path outside them.

`respond_to_hurt(hurt, ...)` owns everything that follows a hit the Knight
actually took, including the `HK_DEATHS` count. The enemy paths compute their
`Hurt` inside `EnemyWorld::tick`, so they reach the response through this
directly.

`apply_hurt(..., damage, direction, hazard)` is the debit plus that response,
and it is what every other source calls. The per-source differences are inputs,
not variations: the amount and direction the caller already computes, `hazard`
to route to the respawn marker, and `player.shadow_dashing`, which is read off
the player rather than passed because no caller may forget it.

Grepping `main.rs` for `cheats.hurt(` or `HK_DEATHS` now returns two lines, both
inside these helpers. The fall-out-of-the-world path cancels dialogue like every
other, because it no longer has its own copy to drift.

## Not in this audit

The source's `damageMode`, parry invulnerability and the `INVUL_TIME_*` variants
beyond the plain and hazard windows. Armour and shields, which no admitted enemy
has: the Zombie Shield and Guard are on the unsupported roster. Hit-stop is
implemented as `freeze_ticks` on the Knight only; the source also freezes the
attacker, which the port does not.
