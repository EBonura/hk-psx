# HKSCR01: the shared script IR

P10 step 2 asks for a compact typed intermediate representation for the source
behaviour the port supports: state transitions, conditions, timers, variables,
references and calls into native systems. This is that choice, made against the
measured inventory rather than in the abstract.

## What the numbers say

`tools/playmaker_inventory.py` over the 45 admitted scenes:

| | |
| --- | ---: |
| FSM instances | 2,028 |
| Distinct FSM names | 200 |
| Enabled actions | 48,037 |
| Distinct action types | 280 |
| Instances in families the guest already runs natively | 15,731 |

Three facts shape the design.

**Instances outnumber definitions ten to one.** 2,028 instances share 200
names, and most of those names are one authored FSM reused per placement. The
program is therefore cooked once per *definition* and bound per *instance*, and
the instance record holds only what actually differs: the owner object, the
initial variable values and the source identity.

**280 action types is too many to implement and far more than is needed.** The
top four actions carry the declarative flow and appear in every scene:
`SendEventByName` 2,332, `Wait` 2,040, `BoolTest` 1,993 and
`Tk2dPlayAnimation` 1,731. The long tail is mostly one-off Unity plumbing. So
the IR has a small fixed opcode set for the flow and a single typed escape into
native code for everything else, rather than one opcode per PlayMaker action.

**A third of the work is already native.** The Hero's Superdash, Spell Control,
Dream Nail and Nail Arts, plus `damages_enemy` and `Geo Rock`, already run as
native controllers and must not be re-run by a script executor. Definitions are
tagged at cook time and never compiled.

## Shape

One cooked bank, `HKSCR01`, holding four aligned sections.

- **Definitions**: name id, state span, variable span, flags.
- **States**: op span, transition span, source state name id.
- **Transitions**: event id, target state index.
- **Ops**: a fixed 8-byte record, `{code: u8, flags: u8, a: u16, b: u16, c: u16}`.

Operands are indices, never pointers: into the definition's variable slots, the
constant pool, the interned event table or the native call table. A fixed-width
op keeps the guest's decode a table index rather than a parser, and keeps the
whole program addressable with 16-bit fields at the sizes measured above.

Variables are typed slots, one byte of type plus a 4-byte payload: bool, i32,
Q16.16 fixed, event id, object reference. No floats and no strings at runtime;
a string is an index into the source-derived localization table, which is what
`GetLanguageString` (1,067 instances) needs anyway.

## Execution

Each active instance holds a current state, a wait counter and its variable
block. A tick runs the current state's ops from where it left off until it
blocks on a wait, takes a transition, or spends its share of the operation
budget.

Bounds are fixed and checked, as step 7 requires: active instances, pending
events, nested dispatch depth and ops per tick. Exceeding any of them is a
guest assertion, not a silent drop, because a dropped event is the failure mode
that would be hardest to notice.

An action authored `everyFrame` is not a one-shot. PlayMaker runs a state's
actions once on entry, then re-runs the OnUpdate of those that have one, every
frame, for as long as the state is current. The IR marks those ops with a
`REPEAT` flag bit, and once a state's one-shot pass is done the executor runs
only the flagged ops, once each per tick. Without it a state authored to watch a
condition would test it once and sit there for ever, which is a silent wrong
answer rather than a refusal, so the compiler must set the flag or refuse the
action.

A state's actions are a priority list, not a set of independent tests.
`FsmState::ActivateActions` in the shipped PlayMaker.dll returns as soon as
`Fsm.IsSwitchingState`, so once one action has transitioned the remaining
actions of that state never run. Every conditional dialogue branch depends on
it: Elderbug's 21 branches are ordered, and each is a said-once flag, so
evaluating them as independent tests would pick the wrong line. The executor
leaves a state the moment an op transitions, and a test names the reason.

Events are synchronous and nested, matching PlayMaker: `SendEventByName` runs
the target's transition inside the sending op, up to the depth bound, and only
then returns. Delayed sends go on a pending queue drained at the start of the
next tick. The Runner's same-callback restart, called out in step 5, works
because of the synchronous half; turning it into an extra frame would change
its behaviour.

## Objects

Nearly every action left on the blocker list needs a script to address an
object, so this is what one is. A reference is a plain `i32`, and there are
exactly three kinds:

- `HERO`, the reserved id `0x7fffffff`. Source ids are path ids masked to 31
  bits, so the top of that range cannot collide with one.
- A cooked object's stable source id, the same one the world metadata bank
  carries, which the host resolves to whatever it actually holds: a region
  object, an actor instance, or nothing.
- `NO_OBJECT`, zero. A native call handed this does nothing, which is what
  PlayMaker does with a null target. It is not an error: a `FindChild` whose
  parent genuinely has no such child stores exactly this, and the original
  stores the same null. What a native call must never do is fall back to some
  default object, because "the wrong object" is worse than "no object".

The executor never resolves a reference. It passes the id to the host, which
owns the mapping, and that is deliberate: the guest's object model is regions,
region objects and actors, not a Unity scene graph, and pretending otherwise in
the IR would invite actions the port cannot honour.

References come from three places and all three are cook time.
`GetOwner` is the FSM's own object, which never changes. `GetHero` is the
constant. `FindChild`, `GetParent` and `FindGameObject` resolve where the
target is static and are refused where it is not.

"Static" is a rule, not a feeling, so here it is. The compiler is handed the
source scene and walks its transform tree. `FindChild` and `GetParent` answer
from the owner, from a literal pointer, or from a variable this cook fixed;
`FindChild` over a parent with no such child stores `NO_OBJECT`, which is the
null the original stores, and the source scene is the authority for that rather
than the cooked world, because an object the port has not built yet is not an
object the game did not have. `FindGameObject` answers for the `Player` tag,
which is the Knight, and for a name exactly one active object of the scene
carries; a second object with the same name is refused, because Unity does not
define which one `GameObject.Find` returns, and a name this scene does not hold
is refused too, because Unity searches the persistent objects as well and the
port has no model of those.

Fixing a variable is the part that earns the rest. `FindChild` under `Self` is
the common shape, and `Self` is whatever the state's opening `GetOwner` put
there, so without that step most of the reference actions in the game resolve
to nothing. A variable is fixed when no action outside the reference set
mentions it under anything but a known read field, since an action the slice
does not model might be writing it; a read sees the write when the two are in
one state, because PlayMaker runs a state's actions in order and abandons the
rest as soon as one transitions, and across states when the writing state
*dominates* the reading one, which is to say every path from the FSM's start
state to the read has already taken the write. Everything else is refused. A
reference resolved from a write that did not happen would be a wrong answer
rather than a refusal, which is the one outcome this format is built to avoid.

Dominance is computed over the authored transitions, which are the whole of a
PlayMaker FSM's control flow once a definition carrying a global transition is
refused. A write counts only where it sits in the run of actions at the top of
its state with nothing before it that could transition or block, because that is
what makes "the state ran" mean "the write ran". A state does not read its own
entry writes this way: an action ahead of the store still has to see nothing,
and the in-order pass inside the state is what gets that right.

The narrow version of this rule was the start state's opening run alone, and
widening it to dominance is worth 505 more resolved object fields, 1,579 to
2,084 over the admitted scenes. It is also a correctness fix: the old rule read
that opening run out of `states[0]` rather than out of the authored start state,
so an FSM that begins elsewhere got the wrong state's writes.

Two of the things that used to disturb a variable were not writes at all, and
both are now settled by evidence rather than by a name on a list.

The first is a rule read off the field's own type. An `FsmOwnerDefault` says
*which* object an action operates on: it is an owner option plus a GameObject,
and an action with something to store uses a plain `FsmGameObject`, because
there is nothing to store an owner option into. Measured over the admitted
scenes before it was trusted: 23,047 occurrences across 133 (action, field)
pairs, carried by six field names, and not one of them is the store field of a
reference action or is even named like an output. Three of the six were being
counted as writes for want of a name on the list, `gameObject1` on
`BoxColliderOffset` and on `BoundsBoxCollider` among them. That is worth 46 more
resolved `GetParent` instances, 144 to 190 of 350.

The second is `SetGameObject`, the largest single disturber, and it is a
reference action like the rest: it copies one object field into an object
variable, so wherever the source resolves the destination is fixed too. It now
compiles as one, and **it resolves nothing at all**: 0 of its 193 enabled
instances, 66 refused because the pointer is into another file and 127 because
the source is itself a variable this cook cannot fix. That is the answer to the
question the earlier note left open. The chain does not bottom out in a rule the
compiler was missing; it bottoms out in cross-file prefab pointers and in other
unfixable variables. What it buys is a named reason in place of 193 silent
refusals, which is what the inventory is for.

## Triggers

`Trigger2dEvent` is the interaction primitive: 1,026 enabled instances, present
in all 45 admitted scenes, and the action that refused first for 274 FSM
instances, more than any other. It fires an event when a collider enters, stays
inside or leaves the FSM object's own trigger volume, filtered by a tag and a
layer.

The port can answer one question about a volume, and it is the question
`dialogue::npc_tick` and the bench already put to a cooked object's bounds: is
the hero body inside it. So the op is `HeroTrigger`, not a general
`Trigger2dEvent`, and it names what it does. Its operands are the owner's cooked
id in the constant pool, the phase in the flags byte beside `REPEAT`, and the
event, in PlayMaker's own `Trigger2DType` order: enter 0, stay 1, exit 2.

The compiler compiles an instance only where the hero is provably the one body
that can be in the volume, and there are two ways to know that. The first is the
action's own tag filter: `Player` is the Knight. The second is the project's
Physics2D layer matrix, which decides what Unity delivers to the collider in the
first place. An instance with no tag filter fires for whatever physics handed
it, and for an object on the `Hero Detector` layer the matrix admits the
`Player` layer and no other named layer, so "any collider" and "the Knight" are
the same set. That is a rule read out of `globalgamemanagers`, not a guess about
what a room contains. Layers the project never named are dropped from the
reading, because their matrix rows keep Unity's all-on default: the inspector
shows no row to edit for an unnamed layer and nothing can be assigned to one,
and no object in the 45 scenes carries a collider on one.

Everything else refuses, and the measured reasons are worth keeping because they
are not one missing feature. 309 instances sit on a layer the matrix delivers no
Player-layer collider to, which is an `Enemy Detector` volume watching for
enemies, or an `Attack` volume that sees the Knight only through the `Hero Box`
layer the port has no collider for. 94 name a tag that is a child
collider of the Knight rather than the Knight: `HeroBox` 36, `Dream Attack` 35,
`Nail Attack` 13, `Hero Spell` 6, `Wall Breaker` 4. Each is a different shape
from the body, so answering them with the body would move where the trigger
fires rather than decline to place it. 51 filter on the `Enemies` layer. 52
store the colliding object into a variable, which has to happen inside the op,
before the send and only when the filter matched, and the op has no fourth
operand for the slot. 22 sit on an object with no trigger collider of its own,
so Unity never raises the callback for them: six `Area Title Controller`
instances are authored exactly that way, with no collider and no `Rigidbody2D`
to forward one from a child. That leaves 470 compiling and 556 refused, and the
compiling ones are 56 proven by the tag and 414 by the layer matrix, over 226
distinct objects that would need cooked trigger bounds if every compiling FSM
shipped. Nine of them do, which is the next section.

Whole FSM instances move much less than that, because an FSM compiles only when
every action in it does: 246 to 255. The ranking is what actually moved.
`Trigger2dEvent` refuses first for 196 instances now rather than 274, and the
instances one action short of compiling regrouped behind `SetFsmBool`, which is
scoping work rather than a missing opcode.

## On the disc

`host/cook_scripts.py` is the cooker and `game/src/script.rs` is the runtime.
Between them they close the loop the measurement was for: the programs reach
`data/scripts.rs`, the guest links them, and the resident scene's instances tick
at 60 Hz against the hero body.

The shipped bank is much smaller than the compiled set, and the gap is the
number worth keeping. 255 instances compile; **245 of them compile to nothing**.
`Spawn Offset`, `PlayMaker Unity 2D`, `RespawnTriggerFSM` and the Hollow Shade
markers are single empty states, so they are not cooked: putting them in the
pool would tick zero ops for ever and would report as coverage. Ten instances
have ops. Three of those ten can act; the other seven are `Area Title
Controller`, one state with no transitions at all, whose `Trigger2dEvent` sends
an event the definition has nowhere to take. That is what the source does too,
so they are cooked and reproduced, and they are never counted as behaviour.

The three that act are `Set` on King's Pass's Set Seen Focus Tablet, which
writes `seenFocusTablet` when the Knight enters its box, `Area Resetter` in
Dirtmouth, which writes `currentArea`, and `Set NPC Leave` in Crossroads_06. The
first two are in the only two scenes the disc holds, and only the second is
reached by a validation route: the tablet trigger spans y 27.98 to 37.9 and both
King's Pass routes pass below it, `kings-death` under y 25 and `kings-return` at
y 11.4. `Area Resetter` at the well mouth is the one `well-drop` and
`crossroads-gate` walk into, and it is what the route assertions pin.

What a script writes survives a quit. The PlayerData store rides in the bench
save record, `HKS3`, in a fixed reserve of sixteen slots rather than in the
bank's own field count, because a record whose length followed a generated table
would invalidate every card fixture on every cook. The cooker refuses a bank that
needs more. Slot order is the cook's, so the record also carries an identity word
over the ordered field list and the loader drops the values when it is running a
different bank: reading `seenFocusTablet` as `currentArea` is the one corruption
a checksum cannot see, because the bytes are intact.

Bounds for the nine watched volumes ride in the generated table rather than in
the world metadata bank. The bank is the right home and needs a `KIND_` of its
own; what it also needs is `host/regions.py` to emit the rows, which is where
the region records are built. Until then an instance is bound to its source
scene rather than to a region, which is sound while no scene has more than two.

Two things about the runtime. The op always carries `REPEAT`, because the source
action listens for as long as its state is current; sampled once on state entry
it would be a trigger that silently never fires again, which is the failure this
format exists to avoid. And the host owns the previous tick's overlap, not the
executor. Unity raises `OnTriggerEnter2D` from the physics step rather than from
the state that listens, so an FSM entering a state while the Knight already
stands inside sees Stay and never Enter; an edge remembered per op would fire
Enter on every re-entry to that state instead.

## Native calls

`CallMethodProper`, `SendMessage` and the rest of the tail compile to a native
call, whose id sits in the op's `flags` so all three operands stay free for
arguments. An object plus a two-component value is the common shape:
`SetVelocity2d(object, x, y)` fits exactly.

There are two forms. `Native` reads its three arguments from variable slots.
`NativeConst` reads them from the constant pool, which is what a cook-time
resolved reference needs: `ActivateAllChildren` over a static parent becomes one
call per child, each carrying the child's cooked id, with no temporaries.

The cooker refuses an id it cannot bind. That is the rule step 7 names: an
unsupported required action must block the scene, never compile to a no-op that
lets the scene claim to be implemented.

## Provenance

Every definition, state and op keeps its source identity: the assets file and
path id of the FSM, the authored state name and the action index within it.
That costs a parallel table in the host report and nothing in the guest bank,
and it is what makes step 8's trace comparison possible: the guest emits
`(definition, state, op)` triples and the host replays the same FSM to compare
event and state order.

## Not in this format

Arbitrary Unity semantics. There is no component graph, no transform hierarchy
walk, no reflection. `FindChild` (995 instances) and `GetOwner` (921) resolve at
cook time into fixed object references wherever the target is static, and are
refused where it is not. A universal PlayMaker VM is explicitly not the goal;
the mapping from each supported source action to an implemented IR form is.

Three actions are refused although they look cheap, and it is worth writing down
why so the next pass does not re-derive it.

`ActivateGameObject` and `ActivateAllChildren` have no guest to call. A cooked
object carries a `state_id`, but that is a persistence key and nothing in the
renderer consults an active flag, so these would compile to a call that changes
nothing. Step 7 asks for a refusal rather than a scene that claims to be
implemented, and a no-op at runtime is the worse of the two because it is
invisible.

They look like the largest unlock on the list and they are not, which is worth
writing down so the next pass does not re-derive it. 872 enabled instances
across the admitted scenes; 695 name an object through a variable this cook
cannot fix; and of the targets that do resolve, **20 are `activate=true` on an
object the cook never built**. That last group is not a resolver problem and
will never come back. It is the disagreement `host/activation.py` already
documents: a fresh save deletes objects that are on screen in the source, and
the cook resolves it by leaving them out. A runtime flag can hide something
built and can never reveal something absent, so the paired `closed`/`open` door
objects, seven of each, are half unbuildable. If those matter later, the fix is
in what the cook admits, not in the script runtime.

What a runtime active flag would actually unlock, measured at full depth, is
**eight FSM instances**: seven `ActivateGameObject` (four in Town, three in
Crossroads_33) and one `ActivateAllChildren`. Widening the reference resolver to
dominance did not move that number at all, which is the measurement that
matters: the binding constraint on these two is the cooked world, not the cook's
ability to name an object.

`SendEventByName` to another FSM needs a cross-FSM instance table, which is the
scoping work and not a missing opcode: the executor already sends synchronously,
it just has nowhere to look the target up.

A send to self compiles today, and "to self" is a larger set than the `Self`
option, because PlayMaker spells the sending FSM more than one way. A send to
`GameObject` delivers to every `PlayMakerFSM` on that object, so where the
object resolves to the sender's own and carries no other FSM, it names the same
single receiver; a send to `GameObjectFSM` is the sender when both halves match.
Recognising those is cook-time resolution with no new opcode and no runtime
lookup, the same shape as `GetOwner`, and it is worth 144 action instances: 58
to 202 of 2,381.

The rest now refuse by what is actually in the way rather than under one
blanket reason, which is the difference between a target the cook cannot name
and a target it named and found to be somebody else. 1,315 send at an object
this cook cannot fix, 513 at a broadcast or a host FSM, 251 at the sender's own
object alongside other FSMs, 92 at another object.

Two fields of this action were being dropped rather than refused, which is the
failure mode the format exists to avoid, and both are now closed. A non-zero
`delay` goes on PlayMaker's own queue and arrives later; the executor has a
pending queue but no op that schedules into it, so six instances authored 0.5,
1.0 and 4.0 seconds were compiling to a send on the wrong frame, and a delay is
refused until an op can carry it. An `everyFrame` send re-sends for as long as
the state is current, which is exactly `REPEAT`, so the one instance authored
that way now carries the flag instead of compiling to a single send.

## The ceiling

This is a measured conclusion rather than a limitation, and it closes the
question of how far the script runtime should go. The answer is: no further.

The temptation is to read the blocker list as a ranking and work down it. That
is the wrong shape. An FSM compiles only when *every* action in it compiles, so
the real question is a set cover: which set of actions completes the most
instances that would then do something. "Do something" here means the instance
can change state or write PlayerData, which is at least one enabled action
naming an event the definition has a transition for, or one PlayerData write.
That is the same test `host/cook_scripts.py` applies to the cooked bank. A
looser definition, any external effect at all, counts nearly every instance and
says nothing.

Of 1,621 blocked instances, **1,267 would act**. That is the ceiling of the
whole enterprise. Greedy set cover over those 1,267:

| actions implemented | acting instances | marginal, per action |
| ---: | ---: | ---: |
| 1 | 66 | 66 |
| 2 | 94 | 28 |
| 5 | 127 | 11 |
| 10 | 177 | 10 |
| 20 | 247 | 7.0 |
| 30 | 342 | 9.5 |
| 50 | 421 | 4.0 |
| 75 | 511 | 3.6 |
| 100 | 677 | 6.6 |
| 150 | 1,020 | 6.9 |
| 200 | 1,135 | 2.3 |
| 302 | 1,267 | 1.7 |

The marginal column is the argument. After the first two entries it sits between
three and ten and stays there for two hundred actions. There is no knee because
there is no shared core to collapse onto: **302 distinct actions** appear across
the missing sets, against 280 action types in the whole measured inventory; the
median blocked-but-acting instance is missing **seven** of them; and 63% are
missing five or more. Only 150 of 1,267 are one action short.

### The head of the curve promises 114 and measures 8

The curve assumes that implementing an action makes it compile. For anything
that names an object, that is false, and the gap is not small:

| rank | action | curve | measured at full depth |
| ---: | --- | ---: | ---: |
| 1 | `ActivateAllChildren` | 66 | **1** |
| 2 | `ActivateGameObject` | 28 | **7** |
| 3 | `SetFsmBool` | 20 | **0** |
| 3 | `SendEventByName` to a sibling FSM | 23 | **0** |
| 5 | `Trigger2dEvent`, owner with no trigger collider | 6 | **0, permanently** |

The top three are worth 114 on the curve and eight in fact. `SetFsmBool` is
zero because not one of its twenty instances names a target that resolves to
exactly one FSM of that name on a fixed object with that variable declared.

The cross-FSM row is the newest and the cleanest, because the third question
has a countable answer here. Once the sends that are really sends to self are
recognised, 23 instances are left one unit short of compiling, each blocked
only by a send reaching the other FSMs on its own GameObject. Building the
instance table would unblock all 23. Those 23 send to 46 sibling FSMs between
them, and **all 46 are themselves blocked**, so every one of those events would
land on a receiver that is not in the bank. That is a send compiled into a
silent no-op, which is the one outcome this format exists to refuse. The
scoping work is not gated on the instance table; it is gated on the receivers.

The fifth row is a different category and the curve cannot see the difference.
Those six instances are not waiting for an implementation. Their FSM's own
GameObject carries no `Collider2D` and no `Rigidbody2D` to forward one from a
child, so Unity never raises `OnTriggerEnter2D` for them and the action cannot
fire however much is built. A blocker list mixes "not implemented" with "cannot
exist", and only the first responds to work.

### The unit price, concretely

`Trigger2dEvent` is what the rate looks like when it is paid. One slice, one
action, 1,026 action instances compiled, and **three acting instances on the
disc**. Three slices of this runtime have produced three acting instances
between them, while the charm board, the shop transaction, the conversation
chain and the False Knight phase machine all landed natively in the same period
against extracted source numbers.

Reaching even a tenth of the 1,267 means twenty to thirty actions and most of
the guest systems they sit on: animation, audio, physics, particles, a cross-FSM
instance table. At that point the systems underneath are carrying the behaviour
and can be used directly, which is what the native work already does.

### A caution about the numbers, which is not about scripts

Three forward projections were made while measuring this, and all three
overshot, in the same direction, for the same reason: each counted cases where
the data exists *somewhere* as cases where the data resolves into a usable
answer.

| projected | measured |
| ---: | ---: |
| 94 activate instances a runtime flag would unlock | 7 |
| 333 activate targets a wider resolver would fix | 94 |
| 1,131 object fields the resolver would resolve | 505 |

The discipline that catches it is asking a third question. Not "does it
compile", not "would it then act", but "can the port answer the thing the action
names". Every one of these collapsed on the third question, and none of them
collapsed on the first two.

### Why this format stays rather than goes

The runtime is frozen, not deleted, for three reasons.

It serves the instances it serves. `Area Resetter` takes its trigger at the
Dirtmouth well mouth and writes `currentArea` on two validation routes.

It is the apparatus that produced this answer. Every number above came out of
`host/script_ir.py` compiling the real FSMs and refusing the ones it cannot, and
that remains the cheapest way to ask what the source actually does before
implementing it natively.

And **the refusals are an asset, not debris**. `.hkpsx/script-ir-coverage.json`
is a live, source-derived inventory of what the port cannot answer, named by
action and by reason, regenerated whenever the cook changes. That is the same
list a native implementer needs in order to know what a scene expects of them.
Deleting the compiler would throw away the list along with the runtime.

That piece of sharpening has since been done, and it is worth recording what it
cost and what it bought, because the prediction attached to it was exactly
right. Four cook-time widenings landed: the `FsmOwnerDefault` rule,
`SetGameObject` as a reference action, the sends to self that PlayMaker spells
another way, and constant folding in `Wait`, `SetBoolValue`, `SetIntValue` and
`IntCompare`, which were reading their scalar without the folding every other
field already used. Together they are **375 more compiled action instances,
8,882 to 9,257 of 41,603, 21.3% to 22.3%**.

They moved the FSM instance count by **nothing**: 256 of 1,821 before and 256
after, with the cooked bank identical at 10 instances, 27 ops and 2,565 linked
bytes. Four independent widenings, each landing the action-instance gain it
predicted to the unit, and not one of them completed a single further FSM. That
is the ceiling measured a second time, from a different direction, by work that
was chosen precisely because it was the cheapest remaining. An FSM needs every
one of its actions before any of it runs, the median blocked instance is missing
seven, and widening the cook does not change that arithmetic.
