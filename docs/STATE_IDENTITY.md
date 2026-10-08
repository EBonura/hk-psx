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
