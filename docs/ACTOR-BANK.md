# Actor art banks: any actor family, scoped by data

Status 2026-10-30. The mechanism of docs/BOSS-ART.md (sprites cut into parts of at most 64x64 texels, shared palettes, packbits,
decoded into the animation cache when a frame first needs them), generalised from one boss to the ordinary actors of any scene.
The first user is the three pre-battle husks of the False Knight arena (Crossroads_10); the first test scene is Crossroads_01's
two Zombie Runners.

## Why

The room path cooks every frame of every clip an actor plays as tiles of the view's texture table, and a view has 640 records,
256 KiB of animation tail and 416 CLUT slots. An actor family with many clips (three husks with all their clips: 99 sprites)
does not fit the arena views next to the False Knight. The bank pays in RAM only what the family's coded art costs, outside the
view's tables.

## What it is

- A **slice** is one `hk_format::boss::Bank` blob: header, clip/sequence/sprite/part tables and the coded texels. A scene's bank
  is one or two slices (`MAX_SLICES`). Sprites and parts are numbered globally, the first slice's first, so a frame names a
  sprite without knowing which slice holds it (`boss::locate_sprite`, `locate_part`).
- Slice 0 is a **data package** in the module pool (`art_*` in `DATA_MODULES`), carried by the scene like `art_goam`: read at
  the gate, resident until the next scene admission, read-only (`modules::bytes`). Slice 1 (art that does not fit the pool, such
  as Death and Fall clips) is read-only data linked into the image (`data/actor_banks.rs`). Both cost RAM from the same
  pot, the free RAM below the pool.
- In each **bank view** the actors' frames are ordinary frame records (clips, lengths, completion and every validator work
  unchanged) whose texture is the first **carrier** texture, a 4x4 streamed texture whose stream is `HKBP`, the slice's bank id
  and a palette index, and whose palette is that bank palette. The carriers are how the bank's CLUTs reach VRAM with the scene's.
  The sprite index rides in the low bytes of the frame box's first two words (`actor_bank::frame_sprite`).
- **Scoping by data.** A bank actor is listed (`row['actors']`) only in the views that carry the bank. The guest already steps
  and draws only the actors its bound view lists, so an actor in a bank-less view neither asserts nor costs anything.

## Turning it on for a scene

Add an entry to `host/actor_banks.json` and run a normal build. Nothing else changes: an empty `scenes` object changes no
cooked byte (the work is a postpass over the finished view packs, `regions.postpack_art_bank`).

```json
{"scenes": {"Crossroads_01": {
  "id": 4,                              // bank id of slice 0; slice k is id + k
  "package": "art_runners",             // the module pool data package that carries slice 0 (name it art_*)
  "actors": ["Zombie Runner", "Zombie Runner 1"],   // actor names or `source` ids of the scene
  "views": [235, 236],                  // optional: chunk ids of the bank views; default: the views whose region cook lists the actors
  "banks": [
    {"name": "core", "palettes": 4, "exclude_clips": ["Death Air", "Death Land", "Fall"]},
    {"name": "late", "palettes": 4, "include_clips": ["Death Air", "Death Land", "Fall"]}]}}}
```

- `palettes` per slice: every palette is a carrier texture in every bank view, so the slices' palettes together must fit the
  tightest bank view's free texture records (Crossroads_10 view 236 has 9).
- `include_clips` / `exclude_clips` split the clips (live and corpse) between the slices by name.
- Only the Zombie family (`ZombieSwipeWalker`) has its clip bindings in `host/actor_bank.py` (`bindings`, a copy of the chain in
  `cook.append_actor_art`). Another family adds its chain there; the cooker itself (`hk-cook actor-bank`) takes any library.
- The additive-scene rule of `actors.py` (an actor merged from an additive scene is refused until its controller opts in) is
  lifted for the entry's actors by `actor_bank.admit`: being in the entry is the opt-in.
- Hornet: her bank is the chunked form (`boss_bank.rs`, carried as scene textures), which a scene with one big boss uses. An
  ordinary actor family in her room (or any other) takes an entry like the one above; both can coexist only if their cache keys
  do not collide, so a scene carries one or the other for now (`KEY_BASE` is shared).

## Checks

- Cook: the bank views are re-validated by `row_over_budget`; the postpass refuses a view that exceeds its tables.
- Pool: the package counts in the room's pool need; `module_publish.rs` fails the build if it does not fit.
- Guest counters (`HK_ACTOR_BANK_*`): `UPLOADS` > 0 where actors are on screen; `MISSES`, `UNBOUND` must stay 0.
- A/B: a scene moved from the room path to the bank must match frame for frame within the shared palettes' quantiser error.
