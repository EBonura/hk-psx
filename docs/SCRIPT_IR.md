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
rest as soon as one transitions, and across states only when the write is in
the run of actions that opens the start state and nothing before it can
transition or block. Everything else is refused. A reference resolved from a
write that did not happen would be a wrong answer rather than a refusal, which
is the one outcome this format is built to avoid.

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
invisible. They are the largest single unlock on the list, which is precisely
why they need the runtime flag first rather than an opcode.

`SendEventByName` to another FSM needs a cross-FSM instance table, which is the
scoping work and not a missing opcode: the executor already sends synchronously,
it just has nowhere to look the target up. A send to self compiles today.
