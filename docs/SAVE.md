# Saving on a console

The bench Save writes the port-1 memory card (`game/src/save.rs`, two files per profile).
This page is what the player sees, what each failure means, how the card layout is checked,
and what to look at when a real console disagrees with the emulator.

## What the player sees

| Moment | Screen |
| --- | --- |
| Sitting at a bench | `Save game?` Yes / No, in the conversation box (the same fleurs and backboard as Elderbug's) |
| Yes | `Saving...` / `Do not remove the memory card.` The box is drawn for several frames before the write starts, because the write holds the CPU for longer than a video period and the frame on screen is the last one drawn until it ends |
| Done | `Game saved` for 1.5 s |
| Card not formatted | `Memory card is not formatted.` / `Format it and save?` Yes / No. The card is never formatted without an answer; a blank card used to be formatted silently by the first save |
| Anything else | `Save failed`, the reason, what to do, a `Code` number and the box's stop mark (X or O closes it). It stays until closed |

Reasons: no memory card in slot 1; card not responding (it answered and broke protocol); could not write
to the card; card full (free a block); card data damaged; unformatted (the question above).

`Code` is `fault * 10000 + hardware * 1000 + exchange`: `fault` is `save::Fault` (0 no card, 1 full, 2 damaged,
3 unformatted, 4 other, 5 not responding, 6 write error), `hardware` is `HK_CARD_FAULT` (1 TX never ready, 2 RX
never filled, 3 `/ACK` never came, 4 `/ACK` never released) and `exchange` is the byte in the frame
transaction where it happened (the response bytes of a read are numbered from 0 at the `0x81` select).
`SELECT SAVE` on the title shows the same reasons for a missing or unformatted card.

Card behaviour that changed with this page: three retries per frame (two video periods apart) for a protocol
error, and for an absent card on reads (the first frame after a chain-load is the one that finds SIO0 as the
launcher left it); when a replacement does not fit, the older copy of that profile is deleted first and the
newer one stays; the SC title frame is written in double-width Shift-JIS and the icon is the Knight's mask.

## What the card keeps, and where it lives in RAM

Before HKS6 the port kept PlayerData in six stores, each with its own bytes in the record, two of them
indexed by cook order. HKS6 (`game/src/save.rs`, `game/src/player_data.rs`) puts every PlayerData value in
one store of named fields. What persists, and who owns it in a session:

| What | Source counterpart | Owner in a session | On the card (HKS6) |
| --- | --- | --- | --- |
| Respawn scene, seat, facing, region | `respawnScene`, `respawnMarkerName` | `frame::Game` | record header |
| Geo wallet | `geo` | `geo::World` | record header |
| Great Door hits | (scene state) | `great_door` | record header |
| The Shade owed | `shadeScene`, `geoPool`, ... | `shade` | record header |
| Abilities, map and mapper, boss flags, `shaman` | PlayerData bools and ints | `player_data` (via `persist::player`) | PlayerData ids 0 to 31 |
| `fireballLevel`, `quakeLevel`, `screamLevel` | PlayerData ints | `player_data` (via `persist::level`) | ids 32 to 34 |
| The cooked FSMs' fields (`seenFocusTablet`, `currentArea`, `openedMapperShop`, ...) | PlayerData, by name | `player_data`; `script.rs` fills the kernel's view before a tick and writes it back after; the title card writes `currentArea` and Cornifer's panel `openedMapperShop` too | ids 36 to 42 |
| Sly's bools and counters, `maxHealthBase`, `MPReserveMax`, keys, eggs | PlayerData | `shop::State` | ids 43 to 60 |
| Charm board: `gotCharm_N`, `equippedCharm_N`, `charmSlots`, `canOvercharm` | PlayerData | `charms::State` | ids 61 to 66 |
| NPC conversation cursors (two bits per NPC) | the NPCs' own PlayerData bools | `dialogue` | id 67 (`port.npcCursors`) |
| Broken walls, mined rocks, cocoon, secrets, arenas, enemy deaths, grubs, totems, area titles seen, pickups, map scenes | SceneData persistent items | `persist::Store` (walls and rocks from `world::State` and `geo::World`) | item list after PlayerData |
| Title style unlocks | roaming shared data | `save::UNLOCKED` | its own file (below) |

Not on the card: the title menu's settings and the Cheats page (chosen at each boot), the charm cheat's grant,
and every `HK_*` telemetry word. `grubsCollected` is the count of `Grub` items rather than a stored number.

The owners marked `shop::State`, `charms::State` and `dialogue` keep their own session state as before; their
fields are copied into the store before a save (`player_data::snapshot`) and handed back at boot. A cooked FSM
that reads one of those fields mid-session therefore sees the value as of the last save or load until that
owner moves onto the store. Everything else reads and writes the store directly.

### The PlayerData store

A field is an id, the id is its index in `player_data::NAMES`, and the name is the source's PlayerData field
name, so a cooked `GetPlayerDataBool`/`SetPlayerDataInt` binds to it at compile time (`player_data::find`; a
cooked name with no row fails the guest build). Names starting `port.` are the port's own. Ids are append only:
a field that retires keeps its id. Every value is an `i32`, a bool 0 or 1. The first 32 ids are HKS5's
`persist` bits at the same numbers (the cooked pickups name an ability by its bit, and `HK_WORLD_PLAYER`, which
routes pin, is still published as that bitmask); 9 to 15 and 27 to 31 are reserved for the branches that use
them, 24 is `shaman` (HKS5's bits 24 and 25); 22, 25 and 35 are retired (below).

Adding a field: append a row to `NAMES` (and a default in `player_data::default` if it is not 0). Nothing else
changes. A record written before the field existed has no entry for it and reads it as its default; every
other field reads as written. No version bump, no fixture rewrite.

Until the merge the port kept two source fields twice. Cornifer's hand-coded panel wrote its own
`openedMapperShop` (id 22, `HK_WORLD_PLAYER` bit 22) beside the one the Crossroads_06 FSM writes (id 39), and
the title card kept its area as a title index plus one (id 35) beside `currentArea` (id 37), which the Town's
`Area Resetter` FSM writes. Both are one field now, as in the source:

- `openedMapperShop` is id 39 for the FSM and for Cornifer (`persist::OPENED_MAPPER_SHOP`). `HK_WORLD_PLAYER`
  still publishes it as bit 22, so the bit is now also set by the Crossroads_06 FSM, and `HK_SCRIPT_FLAGS`
  bit 3 by Cornifer's panel.
- `currentArea` holds the AreaTitleController `areaID` (`title_card::AREA_IDS`, read by host/title_cards.py
  from the constructor's area list: Dirtmouth 1, Crossroads 2, Greenpath 3), which is what
  `AreaTitleController.Finish` compares it with. The title card reads and writes it, so the Area Resetter's
  -1 now re-arms the next area's card, and `HK_SCRIPT_FLAGS` bit 1 is set once any card has announced an area.
- `PlayerData::fold_retired` carries an older record across on load: Cornifer's bit is ORed into
  `openedMapperShop`, and a title-card area overrides `currentArea` with that title's `areaID` (it is what the
  card last announced). The retired ids read zero afterwards.

### HKS6 layout

| Bytes | Field |
| --- | --- |
| 0 to 3 | `HKS6` |
| 4 to 51 | header: scene, seat x and y, facing, region, Geo, Great Door hits (byte 28), the Shade (29 to 47), sequence (48), at HKS5's offsets |
| 52 to 53 | PlayerData entry count `n` |
| 54 | `n` entries of a `u16` id and an `i32` value, ids strictly increasing, fields at their default left out |
| then | SceneData item count `m` (`u16`) and `m` item words, sorted |
| last 4 | FNV-1a of everything before it |

A new game is 60 bytes; every field set and the item list full is 1,236. A section with an id past the table
(a newer build's field), a repeated or out-of-order id, or a count that runs past the record is refused like a
bad checksum: a save at the next bench would otherwise drop the field for good.

### Loading HKS4 and HKS5

The survey decodes only the header of each profile; the chosen one is read again at boot and `save::migrate`
puts PlayerData and the items into their stores. HKS4 and HKS5 records are migrated on that read, never
rewritten on the card until the next bench save writes HKS6:

- the charm board, the NPC cursors and Sly's half are copied to their fields (`maxHealthBase` as five plus the
  masks recorded, `MPReserveMax` as 33 SOUL per vessel); shop bits past the cooked flags, which
  `shop::State::restore` refused, migrate as a new shelf;
- the script reserve goes to the fields the running bank names in those slots, only when its field-list
  identity is this bank's, as before; otherwise it is dropped, as before;
- HKS5's `persist` bits and levels go to ids 0 to 35; HKS4's False Knight in the reserve's top two slots becomes
  the two bits and the arena's item, as HKS5 did;
- then the retired copies are folded into the merged fields (above).

`save::load` and `save::migrate` run once, at boot, so they ride the title's code module (host/code_modules.py)
rather than resident RAM: `main` reads the chosen profile under the loading screen, before `title::close`.

`tests/save_runtime.rs` holds the HKS5 guest's reader unchanged as the oracle and loads every record in every
`tools/cards/*.mcd` fixture both ways, requiring the same state for every owner apart from the merge, then
the HKS6 resave of it. The merge changes the two records of `fungus1-31-bench.mcd` and nothing else (pinned
as `MERGE_CHANGES`): `HK_WORLD_PLAYER` bit 22 becomes 1 from the script's `openedMapperShop`, and
`currentArea` goes from -1 to 3 (copy 1, Greenpath) and to 2 (copy 0, Crossroads) from the title card's area.
At this commit that is 58 cards and 73 records (8 HKS4, 65 HKS5); 25 of the HKS5 records carry an older bank's
script identity (`4201821659`) whose values the HKS5 guest already dropped, and still do.
`tools/migrate_cards.py` now stops at HKS5, the last append-only format, and `tools/seed_card.py` edits HKS5
records (the guest migrates them like any other).

## The profile record (menu style unlocks)

Besides the profiles, the card holds one small file that no slot owns, `BASLUS-00000HKPSXMS`, the port's
counterpart of the original's roaming shared data. It carries the title style unlocks: one bit each for
`infectedMenu` (1), `shadeMenu` (2), `steelSoulMenu` (4) and `eternalOrdealMenu` (8), which the endings
and the Mighty Zote fight set in the original (`MenuStyleUnlock`, `MenuStyleUnlockAction`;
docs/MENU_STYLES.md). Twelve bytes: `HKU1`, the mask, three zero bytes, and the FNV-1a of those eight. It uses one
card block (the profiles' eight, so nine of fifteen), with the same title and icon as the saves. A missing, damaged or
unreadable record reads as nothing unlocked, and a card that is missing, unformatted (never formatted
unasked) or full leaves the keys held for the session only.

- `save::survey` reads it with the profiles, before the title; the title's options row skips the styles whose key is
  clear, exactly as `MenuStyle.IsAvailable` does.
- `save::unlock_styles(mask)` sets keys and writes the record: the Cheats page's last action (`Unlock all menu
  styles`, the port's `unlockAllMenuStyles`) calls it with all four, and the endings and the fight will call it
  with theirs once the port has them. The selector sees the styles at once, and so does the next boot.
- A card that holds only an older save set (no record) is untouched until something unlocks; the layout check
  (`card-check`) lists the record as one more block of the same title.

## Layout check

PSoXide cannot run a BIOS (its HLE BIOS has no card manager), so the console's listing is checked against
the format rules instead: `cargo hk-build card-check CARD.mcd...` prints each card as a manager would
list it and exits 1 on anything the manager would refuse (`host/hk-build/card_check.rs`, with tests over every
committed fixture). It needs a real console to prove the manager actually draws the title and icon.

## Telemetry

`HK_SAVE_UI` (0 idle, 1 ask, 2 format question, 3 saving, 4 saved, 5 failed), `HK_SAVE_WRITES`, `HK_SAVE_ERRORS`,
`HK_SAVE_FAULT` (fault + 1), `HK_CARD_FAULT`, `HK_CARD_FAULT_EXCHANGE`, `HK_CARD_RETRIES`, `HK_CARD_FRAMES`,
`HK_CARD_LAST` (bit 16 set for a write, frame in the low bits) and `HK_CARD_WRITE_VBLANKS` (video periods the last
save held the CPU for: the number that says what a real card costs).

## Diagnostic disc

`HK_GUEST_FEATURES=card-diag cargo hk-build build --library DIR` builds a disc with `game/src/card_diag.rs`:
a CARD DIAG page after the title and two lines at the bottom of every frame. Never ship it, and never install it
over the library copy. The page shows the boot-time first card transaction, a fresh read of frame 0, the
`MC` header, and a write-back of frame 63 (the BIOS's own write-test frame, written with the bytes it already
holds) with a timing sweep if it fails. One photograph of that page says whether a console card answers, whether
a write is accepted, how long it takes in video periods and which timing works.
