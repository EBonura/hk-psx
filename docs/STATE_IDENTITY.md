# Stable identity and state ownership

P04 fixes the identity and in-memory ownership contract before whole-world
metadata and persistence formats depend on build indices. Generate the ignored
registry with:

```sh
.venv/bin/python tools/world_identity.py
```

`world-identities.sqlite` contains collision-checked 64-bit BLAKE2b IDs for all
501 scenes, 1,997,203 serialized components, 1,119 gates, 815 PlayerData keys
and 7,004 referenced assets. `world-identities.json` records counts, fingerprints,
the registry digest and unresolved references. The current verified scan has no
ID collision or unresolved asset reference.

Scene IDs use the normalized Unity source scene path. Scene component IDs use
that path, the Unity PathID and exact component type, so changing BuildSettings
order or `levelN` filenames does not remap them. Gate IDs use the scene path and
TransitionPoint PathID. PlayerData IDs use the exact case-sensitive field name.
Authored spawned instances use the stable prefab ID, stable owning object ID and
an authored stable slot; runtime list position is never an identity.

External asset IDs use source-file SHA256 plus PathID. Those IDs deliberately
belong to one installed source version and are never save keys. Unity built-in
pseudo-assets use the normalized pseudo-file name plus PathID. A changed scene
path or PathID requires an explicit old-ID/new-ID migration alias. An unknown
identity/save schema must be rejected rather than interpreted under new IDs.

The SQLite registry retains aliases for the current guest's sparse IDs: 37
Breakable owners keep `scene×128 + state_index`, and 39 grass owners keep
`scene×1024 + state_index`. Duplicate regional copies must resolve to the same
stable owner or generation fails. This lets P05 relocate metadata without
changing current Tutorial/Town behavior.

[`persistent.rs`](../shared/hk-sim/src/persistent.rs) defines the allocation-free
state API. Global persistent values, challenge/mode values and active-scene bits
and timers have separate owners. Sorted stable-ID entries make mutation order
irrelevant to serialization. Checked `Set`, `Add`, `Maximum` and `Minimum`
operations reject invalid IDs, capacity exhaustion and overflow. `grant_once`
preflights capacity/arithmetic and makes duplicate reward occurrences
idempotent.

For this installed source, the conservative global upper bound is 815 PlayerData
keys plus 4,468 Breakable owners. A full `Store<5283>` is 84,536 bytes. The
reserved `Store<512>` mode owner is 8,200 bytes, and the bounded active-scene
owner with 1,024 flags and 64 timers is 1,176 bytes. This is 93,912 bytes total;
it does not allocate the old per-scene arrays 501 times. Later save work can
encode only occupied sorted entries while preserving these runtime bounds.

Scene leave, bench, death, quit/load and dream return clear active-scene state.
Challenge reset also clears mode state. Mode reset/new game clears every scope.
Loading a save replaces the global snapshot and invalidates active-scene state.
Breakable source flags select active-scene (`dontSave`), mode-session
(`semiPersistent`) or global ownership; grass and ordinary actors are active
scene, while their durable rewards/defeat flags use separate PlayerData owners.

P04 tests cover source-order independence, legacy alias collisions, stable
spawned slots, sorted mutations, overflow/capacity, duplicate rewards, every
reset class, snapshot reload and the exact structure sizes above. This defines
state behavior; P05 moves metadata to scene banks and P13 implements the
memory-card encoding and retail persistence flow.

## One state, every view that draws or holds it

An object's state is scene-wide (the `broken` and `grass` tables of
`world::State`, keyed by the stable scene state id), but its art and its
colliders are cooked per view, and a view lists the objects inside its
activation window (grass and breakables: +2 units; colliders are kept +3). A
view draws every sprite the camera can reach, about 11 units beyond that. So a
patch or stone that only the neighbouring view lists was drawn by this view
without anything to hide it: cut grass and broken stones showed their authored
art again as soon as the camera's view changed (King's Pass, the tablet room:
the grass right of the tablet comes back while the Knight stands left of it),
and 17 broken breakables kept a solid collider in the 1-unit band where a view
holds the collider but not the owner (Tute Doors 2, 6 and 7, the Shaman Temple
planks).

`host/world.py postpack_remote_art` finds, for every view, each such object
that has its art or its collider there and is not listed there. The bank cooks
them as one `KIND_REMOTE_ART` carrier per view: a u16 stream in the index
section with one entry per object (header with the grass flag and the three
list lengths, the scene-local state index, then off draws, on draws and edge
indices), 4 bytes plus 2 per index, where one 48-byte object record each cost
12.8 KB of scene arena (4,294 bindings over the game). `State::apply` hides or
shows those draws from the scene table, and `fill_edges` / `broken_edge` blank
the edges of a broken one. The bound draws join the mutable-draw set the opaque
tile cook excludes (`hk-cook opaque_tiles::mutable_draws`) and the textures
similarity dedup protects. A Geo rock already followed this rule (`bind_regions`
lists a rock wherever a view has its draws or edges).

The cut-stub art of a grass patch, and a broken pole's remnant, are only drawn
by the views that list the object (the cook forces them there): a remote view
hides the authored art and shows no stub.
