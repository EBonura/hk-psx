# Scene spec chunks

A scene's enemy types (`hk_sim::ActorSpec`, and the `CorpseSpec` each points at) are not linked into
the guest. They travel in a chunk of the scene's disc group and are decoded at the gate into a pool
sized for one scene. The linked image used to hold all 65 scenes' specs (15.7 KB of `.data`, about
150 B per distinct type per scene); it now holds the pool (`MAX_SCENE_SPECS` x 108 B +
`MAX_SCENE_CORPSES` x 48 B) and a few bytes per scene.

## Pieces

- `shared/hk-sim/src/wire.rs`: the wire form. Little-endian, field by field; no struct, enum or
  pointer is cast from bytes. Every `match` on `ActorController` and `runner::Attack` is exhaustive,
  so a new variant does not compile until it has a tag (a family added later cannot be forgotten).
  The one reference a spec holds, `ActorSpec::corpse`, travels as an index into the chunk's corpse
  list (`NO_CORPSE` for none) and is resolved against the pool's decoded corpses. Equal corpses
  share a record (61 linked records became 4 per scene at most). No other field of the old structs
  was a pointer: art, audio and clip references are clip ids (`u16`) into banks the scene loads
  anyway.
- `host/hk-specpack` (Rust): rustc is the parser. `host/hk-build/scene_specs.rs` cuts the
  `static SCENE_ACTORS_n` block out of `data/regions.rs` into `.hkpsx/specpack/scene_actors.rs`,
  hk-specpack compiles it in against hk-sim and writes `data/scene-specs/scene_<id>.hkspec`, one
  chunk per catalogue scene padded to `SPEC_SECTORS` sectors, and `data/scene_specs.rs`
  (`SPEC_SECTORS`, `MAX_SCENE_SPECS`, `MAX_SCENE_CORPSES`, `SPEC_SOURCE_FNV`, `FALSE_KNIGHT_SCENE`,
  `SCENE_RUNNER_FAMILY`). `data/regions_guest.rs` is regions.rs without the spec block; the guest
  includes it (native harnesses keep the full file).
- When: on every `cargo hk-build build`, after the asset stage (the three `bind_generated_specs`
  post-passes have run by then) and before the guest compiles. It is not an asset-cache output
  (like `boot_art.rs`): it is rederived from regions.rs each build, in seconds.
- No late bind: `SPEC_SOURCE_FNV` is the FNV-1a of the spec text the chunks were packed from.
  `scene_specs::verify` (called by `package_disc` and `cargo hk-build disc`) refuses a disc whose
  regions.rs spec block no longer hashes to it, and one whose `regions_guest.rs` is stale.
- Pool bounds: hk-specpack fails the cook for a scene past 24 specs or 24 corpses; the pool is the
  widest scene's counts, and `wire::decode_chunk` refuses a chunk that does not fit it.
- Disc: chunk ids follow the stream tails (`SPEC_INDEX`); the chunk sits in its scene's group after
  the atlases and before the scene chunk, so a prefetch holds it with the rest of the group and the
  split windows (everything but scene + metadata, then those two) keep their shape. Its length is
  the cook's constant; its checksum is in its frame (`wire::read_chunk_head`), so it needs no
  per-scene manifest row (`disc::self_checked`).
- Guest: `game/src/spec_pool.rs`. `disc::Cache::prepare_specs` decodes the chunk at the gate;
  `world::region_actors*` read `spec_pool::specs(region.scene)`. Two things are needed before a scene
  is admitted and come from the cook as constants: the runner audio family (the next scene's bank is
  read ahead) and the False Knight's scene (the save carries a won arena).

## Lifetime rules

The pool is overwritten only by an admission, which is exclusive (drained, blacked-out gate, the
outgoing scene's metadata bank revoked first). An `Actor` holds no spec; `Actors::placed_spec`
holds references into the pool and `sync_region` rebuilds it whenever `world::actors_key` changes.
Actors of other scenes stay in the slot array but are never given a spec until their scene is
admitted again. Corpses copy their fields out of the `CorpseSpec` when they start.

## Proof

`cargo test` in `host/hk-specpack` (run by `cargo hk-build build` after the pack) decodes every
chunk and compares it with the generated spec, field by field (Debug text, which covers the
controller payloads and the corpse), for every scene, and checks the cooked constants.
`shared/hk-sim/tests/wire.rs` round-trips every controller variant and refuses malformed bytes.

## Adding an enemy type or a field

A new `ActorController` variant or field: give it a tag in `wire.rs` (the compiler insists) and run
the tests. Nothing per scene or per enemy is edited anywhere else.

## The atlas rows (host/hk-build/guest_manifest.rs)

`data/scene_manifest.rs` lists every atlas chunk as an `AtlasDesc` of eight words (22,656 bytes for
708 rows). The owning scene, the raw length and the unpacked kind and count follow from a row's place
in `SCENE_ATLAS_RANGES`, its kind and its count. On every build, after the spec pack, a Rust step
checks that for all rows and writes `data/scene_manifest_guest.rs` with `ATLAS_ROWS` of 16-byte
`AtlasRow`s; `disc::atlas(i)` expands a row to the old `AtlasDesc`, so every reader sees the same
values. Like the spec pack it is derived from the final cook output each build and touches no cook
key; `guest_manifest::verify` (disc packaging) refuses a stale guest manifest.

## Not done: a header chunk per group

Moving the remaining per-scene descriptors (SceneDesc, CoverageDesc, WorldMetaDesc, EffectArtDesc)
and the 16-byte rows out of the image needs a header at the head of each group plus cook-computed
planning aggregates (group sectors, prefix and first-window sectors, split base, atlas range), because
`staged_prefix`, `tail_fits`, `split_fits`, the prefetch plan and `init` all decide from every chunk's
lengths before anything is read. About 12 to 16 resident bytes a scene would remain. See hk-resident
NOTES for the cost estimate; it is only worth doing if the resident table cost of the remaining
scenes (about 0.25 KB a scene after this) threatens the 8,192-byte bar.
