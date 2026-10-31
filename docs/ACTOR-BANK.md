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

- One **bank** per scene: a `hk_format::boss::Bank` blob (header, clip/sequence/sprite/part tables, then the parts' coded
  texels, live clips first and the corpse clips last) with one set of up to 16 shared palettes. It is cut by byte into two
  memory pieces that the guest reads as one string (`actor_bank::Pieces`): the **head** is a data package in the module
  pool (`art_*` in `DATA_MODULES`, carried by the scene like `art_goam`: read at the gate, resident until the next scene
  admission, read-only, `modules::bytes`), and **the rest** is read-only data linked into the image
  (`data/actor_banks.rs`). Both cost RAM from the same pot, the free RAM below the pool; how much goes in the pool is
  `package_bytes`, a number tuned against the room's pool slack (the pool gate prints it).
- In each **bank view** the actors' frames are ordinary frame records (clips, lengths, completion and every validator work
  unchanged) whose texture is the first **carrier** texture, a 4x4 streamed texture whose stream is `HKBP`, the bank id and
  a palette index, and whose palette is that bank palette. The carriers are how the bank's CLUTs reach VRAM with the scene's.
  The sprite index rides in the low bytes of the frame box's first two words (`actor_bank::frame_sprite`).
- **Scoping by data.** A bank actor is listed (`row['actors']`) only in the views that carry the bank. The guest already steps
  and draws only the actors its bound view lists, so an actor in a bank-less view neither asserts nor costs anything. The
  catch is pop-in: a view draws every actor it lists and no other, and the renderer draws the view whose camera range holds
  the camera, which can be a neighbour of the Knight's. An actor must therefore be listed in every view from which it can be
  on screen: the default is the views whose interaction bounds hold its placement, `reach` widens that for actors that wander
  or stand near a view edge, and `views` overrides it (`"all"` for every view of the scene).

## Turning it on for a scene

Add an entry to `host/actor_banks.json` and run a normal build. Nothing else changes: an empty `scenes` object changes no
cooked byte (the work is a postpass over the finished view packs, `regions.postpack_art_bank`).

```
{"scenes": {"Crossroads_10": {
  "id": 6,                      bank id (the carriers name it)
  "package": "art_husks",       the module pool data package that carries the head of the bank (name it art_*)
  "package_bytes": 35000,       bytes of the bank in the package (multiple of 4); the rest is linked data
  "palettes": 8,                shared palettes = carrier textures in every bank view
  "actors": ["Zombie Barger", "Zombie Runner", "Zombie Hornhead"],   names or `source` ids
  "reach": 8,                   optional: widen the default views by this many world units
  "views": [235, 236]}}}       optional: chunk ids, or "all"
```

- Budget what the bank costs: the package counts in the room's pool need (`module_publish.rs` fails the build if it does
  not fit); the linked rest costs image RAM; every bank view pays the carriers' texture records (`row['textures']` against 640),
  its clip and frame records, and the scene's resident raw grows by what the views do not share (a scene that sizes the
  arena pays it 1:1 in BSS).
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
