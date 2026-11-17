# Hot text: holding the frame path in place in the I-cache

The R3000's instruction cache is 4 KB and direct-mapped: two hot functions whose code lies a multiple of
4 KB apart evict each other on every call. Without a rule for where the hot code goes, its place is decided
by the size of everything linked ahead of it, and an SDK pin that grows or shrinks code the game never runs
moves it. That happened at SDK b0f834e1db84: the same game code measured kings-return 1.73% slower than the
installed build, because `render::scenery_range` moved from cache offset 0x360 to 0xfc8 and shared lines
with `tile_visibility` and `visible_runs_rows` (I-cache refill stalls +6.3%).

## What holds it

`game/hk.layout` is the SDK's layout profile (`psoxide-pgo layout`, tools/psoxide-pgo/README.md "order"):
for every function the frame-table routes ran, its portable name, a hash of its code, the count of every
word that ran and its direct calls. It is collected over the twelve frame-table routes (the measure the
game's speed is judged by), from the first frame after the save loads to the end of the tape.

Every ordinary build (`host/build_guest.py` `place_hot_text`) links once, publishes that link, and runs
`psoxide-pgo place` on its map and patched resident image: the profile binds by name and code hash, the
hot functions are placed at the offsets in the cache that cost least against each other (the gaps filled
with code the routes never ran) and an ordering file comes out, which `game/build.rs` hands to the linker
(`PSOXIDE_LINK_ORDER`) for the second link. The placement is computed from each link, so code ahead of the
hot text changing size does not move the hot text in the cache. `HK_HOT_LAYOUT=0` builds the plain link.

`place` refuses a profile that no longer describes the code (under 98% of its instructions bind, or one
function of more than 1% does not); `psoxide-pgo layout` refuses one in which a function that ran 0.5% or
more of the instructions lies outside the laid-out text (a `.psx_cold` section, or a code module), and names
it. Either means the profile is collected again:

```sh
HK_GUEST_FEATURES=route-input cargo hk-build build --no-validate --library DIR
tools/hot-layout.sh DIR          # writes game/hk.layout
```

Collect again after any change to frame-path code or a new SDK pin that changes the code of a function the
profile lists (an inlined SDK function that changed shape changes its caller's hash).

## One layout for the shipping disc and the route-input twin

The frame table is measured on the route-input twin (its tapes press buttons by poll count); the shipping
disc is built with the `intro` feature instead. The two builds have the same code on the frame path: a
build without the opening movies makes the same call from `main` to a stand-in `intro::play` that does
nothing, the loader's inlining attributes and the cache's fields do not depend on the feature, and
`route-input` changes only the title menu. The profile collected on the twin binds onto the shipping link
with the same hashes, and the placed functions land at the same cache offsets in both (the check: the
cache offset of every placed function in the two link maps).
