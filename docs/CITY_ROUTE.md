# Route to the City of Tears (first-release bar)

Status 2026-11-06 (hk-world). Plan only: nothing here is admitted yet. Sources: the whole-world import's `room-graph.json` (hk-psx checkout `.hkpsx/world-import/`; literal TransitionPoint
adjacency, no ability or PlayMaker gating), `docs/catalog/scenes.tsv` (sizes of the isolated scene packs), `host/quality.py` (what is
admitted today), `docs/BUDGET.md` (free RAM 8,900 B on main 1954aa7; the bar is 8,192 B).

The bar: Soul Master (Ruins1_24 and its additive `Ruins1_24_boss`) and the Desolate Dive pickup (Ruins1_32), with the City reached from the
top of the Fungal Wastes. The Stag is later.

## The road, by the graph

The Fungal Wastes are already adjacent to two admitted rooms: Crossroads_18 (right exit) and Crossroads_49b (exit to Fungus2_07). Shortest
literal path from Crossroads_18, restricted to Fungal Wastes and City rooms:

    Crossroads_18 -> Fungus2_06 -> Fungus2_07 -> Fungus2_08 -> Fungus2_09 -> Fungus2_10 -> Fungus2_21 -> Ruins1_01

Fungus2_21 is the top of the Wastes; its second exit is Ruins1_01, the City's first room. Inside the City the shortest path to the Sanctum is

    Ruins1_01 -> Ruins1_17 -> Ruins1_06 -> Ruins1_31 -> Ruins1_23 -> Ruins1_30 -> Ruins1_24 (Soul Master) -> Ruins1_32 (Desolate Dive)

Queen's Station (Fungus2_01/02) and the Fog Canyon rooms are not on this road: they are reached from Fog Canyon (Fungus3_02), which Greenpath
reaches through Fungus1_13 and Fungus1_23. Admitting the Wastes through Crossroads_18 needs none of them. The Leg Eater (Fungus2_26, a shop off
Fungus2_06) is optional. Fungus2_06 and 2_08 have side exits (2_05, 2_33, 2_32) that stay dangling until later; a gate whose target is
off disc is dropped by the census (`world.resolve_gates`), as Fungus1_31's was.

## Costs per scene (isolated pack figures from the catalog; none is the in-world figure)

| Scene | views | pages | palettes | scene RAM | VRAM | disc | enemies (families) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| Fungus2_06 | 22 | 8 | 608 | 179,652 | 281,600 | 119,145 | Fungus Flyer 4, Fung Crawler 2 |
| Fungus2_07 | 12 | 3 | 265 | 82,936 | 106,784 | 51,706 | Mushroom Baby 3, Mushroom Roller 2 |
| Fungus2_08 | 10 | 4 | 334 | 93,712 | 141,760 | 60,175 | Mushroom Turret 3, Fungus Flyer 1 |
| Fungus2_09 | 8 | 3 | 243 | 73,032 | 106,080 | 44,811 | none |
| Fungus2_10 | 24 | 6 | 459 | 142,204 | 211,296 | 91,157 | Zombie Fungus A 1, B 1 |
| Fungus2_21 | 28 | 6 | 433 | 130,704 | 210,464 | 81,173 | Fung Crawler 1 |
| Ruins1_01 | 18 | 5 | 306 | 97,536 | 173,632 | 54,436 | Ceiling Dropper 2, Ruins Sentry 1, Zombie Barger/Hornhead/Leaper 1 each |
| Ruins1_17 | 24 | 7 | 397 | 123,872 | 242,080 | 69,371 | Ruins Flying Sentry 2, Javelin 2, Buzzer 1, Ruins Sentry 1, Zombie Hornhead 1, Leaper 1 |
| Ruins1_06 | 15 | 4 | 255 | 88,928 | 139,232 | 48,411 | none |
| Ruins1_31 | 18 | 5 | 335 | 88,960 | 174,560 | 49,225 | none (bench, lift, toll gate, breakable wall) |
| Ruins1_23 | 21 | 7 | 390 | 117,448 | 241,856 | 66,379 | Mage Blob 4, Mage 2, Mage Knight 1 |
| Ruins1_30 | 20 | 7 | 405 | 136,428 | 242,336 | 79,129 | Mage Blob 9, Mage Balloon 4, Mage 3 |
| Ruins1_24 | 20 | 7 | 449 | 119,352 | 243,744 | 70,823 | none; Soul Master arrives from the additive `_boss` scene |
| Ruins1_32 | 24 | 7 | 422 | 136,580 | 242,880 | 80,916 | Mage Blob 16, Mage Balloon 12 |

Fourteen scenes, 1.17 MB of disc. Every row is inside the scene arena (415,052 B) and the page budget (`STATIC_PAGE_BUDGET` 19); the largest
palette count, 608 (Fungus2_06), is below the 1,152-entry texture capacity and below Fungus1_22's 681 that is already on the disc. None of
these needs a new budget class. They need the same gate census, the cook (about 2 minutes per scene on a cook-store hit, far longer cold) and
the packet and CLUT headroom check that Fungus1_31/32 got (`PACKETS region` lines in the build log).

## Free RAM, the real constraint

Each admitted scene costs resident bytes (scene table rows, atlas descriptors, the metadata-bank index). The last admission, Fungus1_20_v02,
moved free RAM from 10,900 to 9,668 (1,232 B, with a Crawler pin and census code in the same change). At roughly that rate the fourteen
scenes above cost about 17 KB. Main has 708 B above the bar. So the order is:

1. hk-resident lands (about +20 KB). Until then nothing here is admitted on main; a probe can be built on a branch.
2. Admit one scene (Fungus2_06) on a branch and read the real per-scene delta from BUDGET.md before committing to the count.
3. If the per-scene delta is over about 1.2 KB, the next hk-resident step (what is still linked per scene) comes before more rooms.

## What has to exist first

Admission is mostly a catalogue row, but a room is only playable when its enemies and objects are. The census has not been run on these
scenes; this list is from the catalog columns, not from the cook report, and the first admission turns each line into a fact.

Fungal Wastes (all new to the port): Fungus Flyer, Fung Crawler, Mushroom Baby, Mushroom Roller, Mushroom Turret, Zombie Fungus A and B,
(the Cap Hit and Mushroom Brawler of 2_05/2_29 are off the road). Each is a PlayMaker tree, so each is a row in `host/enemy_families.json`
(docs/ENEMY-FAMILIES.md) plus whatever FSM actions the HKFSM03 kernel lacks; only Plant Trap exists as a family row today. The Mushroom
Roller may reuse the Baldur recognizer (both are `Roller`-named line-of-sight rollers; to be proved, not assumed). Other objects: bouncy
mushrooms, breakable walls and the soul totem in 2_10 and 2_21, two levers in 2_21 (the existing lever/gate census), Geo rocks.

City of Tears: Ruins Sentry, Ruins Flying Sentry and the Javelin variant, Ceiling Dropper, Zombie Barger/Hornhead/Leaper (the Crossroads
Runner variants may cover them; the Hornhead and Leaper already run on the parameterized Runner), Buzzer (Vengefly family, in the port), Mage,
Mage Blob, Mage Balloon, Mage Knight. Objects: lifts (Ruins1_23, 25, 31), toll gates (Ruins1_17, 23, 30, 31), levers and switches (the
existing census covers levers and toll gates; lifts are not in it), the City bench, the shiny pickup in Ruins1_32 (Desolate Dive: needs the
spell grant path that Vengeful Spirit used, `spells.py`).

Bosses and scripted scenes:
* Soul Master (Mage Lord, Mage Lord Phase) in `Ruins1_24_boss`, 2 enemies, an additive scene like Hornet's. This is a Hornet-sized item
  (docs/HORNET.md: a fight module of roughly 50 KB, a 15 KB art module and a 140 KB boss bank in the scene stream). The room's pool budget
  is the risk: Ruins1_24 is not yet measured, and Hornet's room left 220 B of 98,304. The Mantis Lords are in scope per Manny's decision
  but they are in Fungus2_15, off this road (the road does not pass them), so they follow, not precede, the City.
* The Desolate Dive pickup and its cutscene in Ruins1_32.

Cross-cutting: `INERT_ON_DISC` and the PlayerData bits for rooms of the Wastes (for instance the Fungus2_21 levers); the music and ambience for
two new areas (`docs/MUSIC.md`); the area name cards; the map (`game_map.py`) for Fungal Wastes and City sheets.

## Ability gates (not decided from data)

The room graph has no ability logic. What the original asks of the Knight on this road is **not verified here**. The walk planner
(`hk-walkplan`) can check each hop with the abilities the port grants at that point (Mothwing Cloak from Hornet, Vengeful Spirit, no Mantis
Claw because the Claw is in Mantis Village, Fungus2_14, off this road). If a hop needs the Claw, the road is Wastes-to-Mantis-Village first
and the City moves behind it. That is the first thing the Fungus2_06 probe should answer, by walking 2_06 to 2_21 with the planner before any
City room is cooked.

## Order

1. Hornet sightings and Geo streaming (this lane's items 2 and 3) are independent of the road; the Hornet counter is only read by the
   sightings, so the road does not need them.
2. After hk-resident: Fungus2_06 probe (RAM delta, enemy census, planner walk to 2_07).
3. Fungus2_07, 08, 09 (the three light rooms), then 2_10 and 2_21, one landing each, `walk-` file per room pair, library disc each time.
4. Ruins1_01, 17, 06, 31 (no enemy beyond Sentries and Zombies already half covered), then 23, 30, 25 if it lies on the Soul Master gate.
5. Ruins1_24 with Soul Master (own lane, Hornet-sized), then Ruins1_32 and the pickup.
