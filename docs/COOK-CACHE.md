# The cook cache: what a cook reuses, and how to force a full cook

A change to a host script used to recook the whole game: a full region recook is about 3.8 hours of CPU
(about 75 minutes of wall time on a loaded machine), and the asset scripts and the prepass ran again on
top. The cook now records what it actually used and recooks only what a change touches.

The one rule: **a cached build is byte for byte what a full recook would make.** If you ever suspect it is
not, force a full cook (below) and tell the coordinator.

## Forcing a full cook

```text
cargo hk-build build --recook ...         ignore every receipt and cache: regions, menus, assets
HK_FORCE_COOK=1 cargo hk-build cook       the same, through the environment
cargo hk-build cook-check [--keep]        recook everything in a clone of this checkout with every cache
                                          ignored, then compare data/ and .hkpsx/ with this checkout's; fails
                                          on any difference. A full recook: take a cook slot first.
```

A full cook needs a cook slot (`lane-infra/cookslot.sh acquire <lane>`) because it writes several GB.
`HK_FORCE_COOK` is also part of the unit identity only through its effect: forced units are cooked again and
get fresh receipts.

## Units and what they record

```text
region scenery      room.hk, tilemap-fill.json, unsupported.json, scene-base.json, the base report
region annotations  the actor, hazard and shroom records (cook.cook_annotations)
cook stage          the main process of host/regions.py: the passes over the finished regions
```

A **receipt** (`data/regions/region-NNN/scenery-cache.json`, `annotations-cache.json`) holds the unit's
identity (region spec, the budget constants, the region's scene row, the retail root), the digest of every
file it wrote, the report it returned and the name of its **dependency record**
(`.hkpsx/cookdeps/<sha>.json`). It is written after the outputs are complete and removed before a unit
rewrites them, so a cook that dies leaves nothing that vouches for a half-written file.

A **dependency record** (host/depcache.py) is made while the unit runs and lists:

- every Python function in `host/` and `tools/` that started, by bytecode digest (comments, blank lines and
  line moves are not edits; a function the unit never ran is not a dependency);
- every loaded host module's **skeleton**: its module level code (constants, tables, imports, decorators and
  class bodies) with function bodies removed. Editing a table at module level invalidates every unit that
  loaded the module; editing a function body invalidates only units that ran it. The scene catalog
  `host/quality.py` is the one exception: growing it must keep earlier regions, so the units key the budget
  constants and their own scene row by value instead;
- every repository file opened for reading (hash at open time) and every directory listed;
- every retail source file opened, with the hash the build driver's checked manifest holds for it;
- every program a unit ran (the executable's hash), and a flag that makes the record never fresh if the unit
  used `os.system`, an exec, a fork or the network;
- the interpreter, `host/requirements.lock`, the depcache `VERSION` and every `HK_` environment variable that
  is not a path.

A worker process cooks one part of one scene, so all of its units share one record, and the record is
cumulative over the process: a result one unit computed and a later one read from an in-process cache still
shows the code that made it. Scenery and annotations are separate processes for the same reason, and
`host/cook.py` imports the actor recognisers only inside `cook_annotations`, so an edit to an enemy never
invalidates scenery.

The **stage record** (`.hkpsx/stagecache/regions.json`, written by `host/stagecache.py run`) names the main
process's own record and the records of every unit it used. `cargo hk-build cook` asks
`stagecache.py status regions`; fresh and the Rust steps' sources and the output hashes unchanged is
"Cook cache hit".

## What invalidates what

| Change | Recooks |
| --- | --- |
| a function an enemy recogniser runs (`host/actors.py`, `gruzzer.py`, ...) | annotations of the regions whose worker ran it, then the passes after them |
| a function the scenery cook runs (`cook.py`, `quantize.py`, ...) | scenery of the regions whose worker ran it |
| a module level table in a module a unit loaded | that unit |
| a function never run, a comment, a blank line | nothing |
| a file read through `open()`, or a directory listed | the units that read it |
| a deleted or renamed `.py` a unit imported | those units |
| the retail game files | the units that read the changed files |
| `host/requirements.lock`, Python version, an `HK_` switch | everything |
| `host/cook_inputs.txt` | only the cook store key and the Rust steps' fingerprint |

## Known limits

- The Rust steps that run after `host/regions.py` (the Mossman, Shaman runner and Hornet scene cookers) are
  keyed by the non-Python entries of `host/cook_inputs.txt` plus their own sources. Other `hk-cook` code they
  link is not tracked there (it is hashed by the asset key and the cook store key).
- A unit must not update one of its own inputs in place. A file a unit both read and wrote counts as an
  output, not as an input.
- `.hkpsx/provenance.json` is written by every region cook (last one wins) and only its `inputs` list is read.
  It is not compared by `cook-check`.
- Code outside `host/` and `tools/` that a unit imports is recorded by whole-file hash; native libraries are
  covered by `requirements.lock`.

## Adding a stage

Run its script as `python host/stagecache.py run <stage> host/<script>.py`, add `depcache.STAGE_UNITS` ids if
it has units, and ask `stagecache.py status <stage>` before running it again. Enter `depcache.Tracker`
before importing the modules the unit uses, or functions that run while they import are missed.
