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

## The shape to centralize on

One `apply_hurt(source, amount, direction, hazard)` that performs the debit and
the entire response, returning the `Hurt` for the caller's own bookkeeping. The
per-source differences are all inputs to it, not variations of it:

- the amount and direction, which every caller already computes,
- `hazard`, which routes to the respawn marker,
- `player.shadow_dashing`, which every caller already passes and none may forget.

`HK_DEATHS` belongs inside it too: today four call sites increment it and the
fifth would be easy to miss.

## Not in this audit

The source's `damageMode`, parry invulnerability and the `INVUL_TIME_*` variants
beyond the plain and hazard windows. Armour and shields, which no admitted enemy
has: the Zombie Shield and Guard are on the unsupported roster. Hit-stop is
implemented as `freeze_ticks` on the Knight only; the source also freezes the
attacker, which the port does not.
