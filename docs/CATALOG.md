# Scene catalog

Generated from `docs/catalog/scenes.tsv` by `hk-catalog render`; do not edit by hand. The TSV is generated from the ignored whole-world import (`.hkpsx/world-import`) by `hk-catalog extract`. Both steps are deterministic and a test (`catalog::tests`) fails when either file drifts. It holds names, counts and byte sizes only, no game assets.

- Scenes: 501 (every BuildSettings scene; none excluded)
- Source world fingerprint: `cac49e9826463f629043d52586daa5ebfc31cd340cb178d097ce8f86c906d52e`
- Pack matrix inventory fingerprint: `763003846b5b2be2d8783767c371eb04e4165fa64f4b0e6fdd0910cd9e96f72d`
- Regenerate (from the repository root, with the import already done): `cargo run --release --manifest-path host/Cargo.toml -p hk-cook --bin hk-catalog -- all .hkpsx/world-import`

## Cook state

| State | Scenes | Meaning |
| --- | ---: | --- |
| playable | 60 | In `quality::SCENE_TABLE`: on the disc and playable. |
| cooked-pack | 397 | An isolated scene pack cooks (decoder and packet bound PASS) but the scene is not admitted to the disc. |
| no-envelope | 41 | The importer found no tilemap, gate or camera envelope to cook views from. |
| over-128-views | 3 | The scene needs more than the 128 views one HKSCNE scene header allows, so its pack fails. |

## Areas

Area comes from the scene name prefix (`area_of` in `host/hk-cook/src/catalog.rs`). Fungus3 holds both Fog Canyon and the Queen's Gardens and is not split. `Interior rooms` are the `Room_*` scenes that do not name their area. Enemies are HealthManager objects (bosses, minions and spawners included). Cooked KB is the summed stored pack size of the scenes that cook.

| Area | Scenes | Playable | Cooked pack | No envelope | Over 128 views | Enemies | Cooked KB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Menus, helpers and tests | 12 | 0 | 2 | 10 | 0 | 0 | 35 |
| Cinematics and openings | 10 | 0 | 2 | 8 | 0 | 0 | 41 |
| King's Pass | 1 | 1 | 0 | 0 | 0 | 7 | 177 |
| Dirtmouth | 1 | 1 | 0 | 0 | 0 | 0 | 58 |
| Interior rooms | 22 | 1 | 21 | 0 | 0 | 7 | 516 |
| Tram | 2 | 0 | 2 | 0 | 0 | 0 | 39 |
| Forgotten Crossroads | 46 | 43 | 0 | 3 | 0 | 274 | 2206 |
| Greenpath | 37 | 14 | 22 | 1 | 0 | 241 | 2757 |
| Fungal Wastes | 32 | 0 | 30 | 2 | 0 | 198 | 2186 |
| Fog Canyon and Queen's Gardens | 33 | 0 | 30 | 3 | 0 | 276 | 1835 |
| City of Tears | 45 | 0 | 41 | 4 | 0 | 238 | 2181 |
| Resting Grounds | 11 | 0 | 10 | 1 | 0 | 12 | 502 |
| Howling Cliffs | 7 | 0 | 6 | 1 | 0 | 45 | 594 |
| Crystal Peak | 29 | 0 | 28 | 1 | 0 | 110 | 1649 |
| Deepnest | 29 | 0 | 29 | 0 | 0 | 190 | 2328 |
| Kingdom's Edge | 20 | 0 | 19 | 1 | 0 | 150 | 1187 |
| Colosseum of Fools | 6 | 0 | 6 | 0 | 0 | 31 | 228 |
| Ancient Basin and Abyss | 21 | 0 | 20 | 0 | 1 | 121 | 1556 |
| Royal Waterways | 16 | 0 | 14 | 2 | 0 | 208 | 927 |
| The Hive | 6 | 0 | 6 | 0 | 0 | 115 | 379 |
| White Palace | 19 | 0 | 17 | 0 | 2 | 68 | 1469 |
| Grimm Troupe | 4 | 0 | 3 | 1 | 0 | 4 | 96 |
| Dreams | 13 | 0 | 13 | 0 | 0 | 18 | 589 |
| Hollow Knight finale | 2 | 0 | 2 | 0 | 0 | 1 | 68 |
| Godhome | 77 | 0 | 74 | 3 | 0 | 185 | 3003 |
| **Total** | **501** | **60** | **397** | **41** | **3** | **2499** | **26606** |

## Scenes without an envelope

These cannot cook as rooms today. The role is the importer's evidence-backed classification.

| # | Scene | Area | Role | Flags |
| ---: | --- | --- | --- | --- |
| 0 | Pre_Menu_Intro | Menus, helpers and tests | transition_or_bootstrap_helper | text |
| 1 | Menu_Title | Menus, helpers and tests | menu | text |
| 2 | Quit_To_Menu | Menus, helpers and tests | transition_or_bootstrap_helper |  |
| 3 | BetaEnd | Menus, helpers and tests | transition_or_bootstrap_helper |  |
| 4 | Knight_Pickup | Menus, helpers and tests | shared_player_prefab_scene |  |
| 5 | Opening_Sequence | Cinematics and openings | gameplay | text |
| 8 | Cinematic_Stag_travel | Cinematics and openings | cinematic |  |
| 47 | Crossroads_10_preload | Forgotten Crossroads | transition_or_bootstrap_helper |  |
| 48 | Crossroads_10_boss | Forgotten Crossroads | gameplay |  |
| 49 | Crossroads_10_boss_defeated | Forgotten Crossroads | additive_boss_defeated_variant |  |
| 102 | Ruins1_24_boss | City of Tears | gameplay |  |
| 103 | Ruins1_24_boss_defeated | City of Tears | additive_boss_defeated_variant |  |
| 115 | Ruins2_03_boss | City of Tears | gameplay |  |
| 126 | Ruins2_11_boss | City of Tears | gameplay |  |
| 133 | Fungus1_04_boss | Greenpath | gameplay |  |
| 180 | Fungus2_15_boss | Fungal Wastes | gameplay |  |
| 181 | Fungus2_15_boss_defeated | Fungal Wastes | additive_boss_defeated_variant |  |
| 209 | Fungus3_23_boss | Fog Canyon and Queen's Gardens | gameplay |  |
| 221 | Fungus3_40_boss | Fog Canyon and Queen's Gardens | gameplay |  |
| 229 | Fungus3_archive_02_boss | Fog Canyon and Queen's Gardens | gameplay |  |
| 232 | Cliffs_02_boss | Howling Cliffs | gameplay |  |
| 238 | RestingGrounds_02_boss | Resting Grounds | gameplay |  |
| 261 | Mines_18_boss | Crystal Peak | gameplay |  |
| 325 | Deepnest_East_Hornet_boss | Kingdom's Edge | gameplay |  |
| 355 | Waterways_05_boss | Royal Waterways | gameplay |  |
| 361 | Waterways_12_boss | Royal Waterways | gameplay |  |
| 392 | Grimm_Main_Tent_boss | Grimm Troupe | gameplay |  |
| 410 | Cinematic_Ending_A | Cinematics and openings | cinematic |  |
| 411 | Cinematic_Ending_B | Cinematics and openings | cinematic |  |
| 412 | Cinematic_Ending_C | Cinematics and openings | cinematic |  |
| 413 | Cinematic_Ending_D | Cinematics and openings | cinematic |  |
| 414 | Cinematic_Ending_E | Cinematics and openings | cinematic |  |
| 415 | End_Credits | Menus, helpers and tests | credits | text |
| 416 | Cinematic_MrMushroom | Cinematics and openings | cinematic | text |
| 417 | Menu_Credits | Menus, helpers and tests | credits | text |
| 418 | End_Game_Completion | Menus, helpers and tests | completion_screen | text |
| 419 | PermaDeath | Menus, helpers and tests | transition_or_bootstrap_helper | text |
| 421 | PermaDeath_Unlock | Menus, helpers and tests | transition_or_bootstrap_helper | text |
| 465 | GG_Unlock | Godhome | transition_or_bootstrap_helper | text |
| 480 | GG_Entrance_Cutscene | Godhome | gameplay |  |
| 483 | GG_Boss_Door_Entrance | Godhome | gameplay |  |

## Scenes over the 128-view limit

| # | Scene | Area | Views needed |
| ---: | --- | --- | ---: |
| 345 | Abyss_21 | Ancient Basin and Abyss | 136 |
| 376 | White_Palace_13 | White Palace | 135 |
| 383 | White_Palace_20 | White Palace | 156 |

## Enemy families

A family is a HealthManager object's GameObject name with trailing numbers removed. It is the source naming, not the 136 journal families, and one journal family can span several of these names.

197 families, 2499 instances.

| Family | Instances | Scenes | Areas |
| --- | ---: | ---: | --- |
| fluke_baby | 99 | 2 | Royal Waterways, Godhome |
| Jellyfish Baby | 81 | 8 | Fog Canyon and Queen's Gardens |
| Mage Balloon Spawner | 69 | 3 | City of Tears, Colosseum of Fools, Godhome |
| Hatcher Baby Spawner | 68 | 4 | Forgotten Crossroads |
| Bee Hatchling Ambient | 65 | 9 | Kingdom's Edge, Ancient Basin and Abyss, The Hive |
| Pigeon | 62 | 8 | Greenpath |
| Hopper | 56 | 8 | Kingdom's Edge |
| Buzzer | 54 | 19 | King's Pass, Forgotten Crossroads, Greenpath, City of Tears, Howling Cliffs |
| Ceiling Dropper | 52 | 16 | City of Tears, Resting Grounds, Kingdom's Edge, Ancient Basin and Abyss, Royal Waterways |
| Jellyfish | 52 | 9 | Fog Canyon and Queen's Gardens |
| Hiveling Spawner | 50 | 3 | The Hive |
| Spider Mini | 45 | 10 | Deepnest |
| Fungoon Baby | 44 | 8 | Fungal Wastes, Godhome |
| Moss Flyer | 43 | 12 | Fog Canyon and Queen's Gardens, Deepnest |
| Abyss Crawler | 42 | 5 | Ancient Basin and Abyss |
| Inflater | 38 | 7 | Royal Waterways |
| Shade Sibling | 38 | 2 | Ancient Basin and Abyss |
| Mage Blob | 37 | 5 | City of Tears, Colosseum of Fools |
| Enemy | 33 | 5 | White Palace |
| Moss Walker | 33 | 16 | Greenpath, Fog Canyon and Queen's Gardens, Deepnest |
| Mushroom Baby | 33 | 6 | Fungal Wastes |
| White Palace Fly | 31 | 5 | White Palace |
| Baby Centipede | 29 | 6 | Deepnest |
| Climber | 27 | 10 | Forgotten Crossroads, Greenpath, Howling Cliffs |
| Crawler | 26 | 10 | King's Pass, Forgotten Crossroads, Greenpath, Howling Cliffs |
| Plant Trap | 25 | 7 | Greenpath, Fog Canyon and Queen's Gardens |
| Ruins Flying Sentry | 25 | 12 | City of Tears, Resting Grounds, Ancient Basin and Abyss, Royal Waterways |
| Spider Flyer | 25 | 3 | Deepnest |
| Crystallised Lazer Bug | 24 | 4 | Crystal Peak |
| Jellyfish Baby Inert | 24 | 1 | Fog Canyon and Queen's Gardens |
| Spitter | 24 | 6 | Forgotten Crossroads, Ancient Basin and Abyss |
| Mosquito | 23 | 7 | Interior rooms, Greenpath |
| Ruins Sentry | 23 | 13 | City of Tears, Resting Grounds, Ancient Basin and Abyss, Royal Waterways |
| Super Spitter | 23 | 6 | Kingdom's Edge |
| Crystal Flyer | 22 | 8 | Crystal Peak |
| Mantis Heavy Flyer | 22 | 8 | Fog Canyon and Queen's Gardens, Deepnest |
| Angry Buzzer | 21 | 11 | Forgotten Crossroads |
| Zombie Hornhead | 21 | 17 | Forgotten Crossroads, Greenpath, Fungal Wastes, City of Tears, Howling Cliffs, Ancient Basin and Abyss |
| Fly | 20 | 4 | Forgotten Crossroads |
| Mushroom Turret | 20 | 8 | Fungal Wastes |
| Zombie Barger | 20 | 13 | Forgotten Crossroads, Greenpath, City of Tears, Howling Cliffs, Ancient Basin and Abyss |
| Fluke Fly | 19 | 5 | Royal Waterways, Godhome |
| Flukeman | 19 | 5 | Royal Waterways, Godhome |
| Tiny Spider | 19 | 4 | Deepnest |
| Zombie Miner | 19 | 8 | Crystal Peak |
| Zombie Runner | 19 | 14 | Forgotten Crossroads, Greenpath, City of Tears |
| Acid Flyer | 18 | 1 | Greenpath |
| Blow Fly | 18 | 4 | Kingdom's Edge |
| Grass Hopper | 17 | 2 | Greenpath, Fog Canyon and Queen's Gardens |
| Mantis Flyer Child | 17 | 4 | Fungal Wastes |
| Mines Crawler | 17 | 7 | Crystal Peak |
| Fung Crawler | 16 | 8 | Fungal Wastes |
| Fungus Flyer | 16 | 8 | Fungal Wastes |
| Mage Balloon | 16 | 2 | City of Tears |
| Mushroom Roller | 16 | 5 | Fungal Wastes |
| Royal Zombie Coward | 16 | 9 | City of Tears |
| Royal Zombie | 15 | 9 | City of Tears |
| Mage | 14 | 5 | City of Tears, Colosseum of Fools |
| Mantis | 14 | 5 | Fungal Wastes |
| Mantis Lord S | 14 | 3 | Fungal Wastes, Godhome |
| Ruins Flying Sentry Javelin | 14 | 9 | City of Tears, Ancient Basin and Abyss, Royal Waterways |
| Bee Stinger | 13 | 5 | The Hive |
| Fluke Fly Spawner | 13 | 3 | Royal Waterways, Godhome |
| Mossman_Shaker | 13 | 7 | Greenpath |
| Royal Zombie Fat | 13 | 8 | City of Tears |
| Zombie Leaper | 13 | 9 | Forgotten Crossroads, Greenpath, City of Tears, Howling Cliffs |
| Zombie Spider | 13 | 2 | Deepnest |
| Baby Centipede Spawner | 12 | 2 | Deepnest |
| Black Knight | 12 | 2 | City of Tears, Godhome |
| Bursting Zombie | 12 | 8 | Forgotten Crossroads |
| Crystal Crawler | 12 | 8 | Crystal Peak |
| Flip Hopper | 12 | 4 | Royal Waterways |
| Garden Zombie | 12 | 4 | Fog Canyon and Queen's Gardens, Deepnest |
| Roller | 12 | 2 | Forgotten Crossroads, Crystal Peak |
| Plant Turret | 11 | 3 | Greenpath |
| Ruins Sentry Fat | 11 | 6 | City of Tears, Royal Waterways |
| Slash Spider | 11 | 4 | Deepnest |
| Egg Sac | 10 | 10 | Forgotten Crossroads, Fog Canyon and Queen's Gardens, Crystal Peak, Deepnest, Kingdom's Edge, Royal Waterways |
| Zombie Beam Miner | 10 | 3 | Crystal Peak |
| Zombie Hornhead Sp | 10 | 4 | Deepnest |
| Acid Walker | 9 | 4 | Greenpath, Fog Canyon and Queen's Gardens |
| Fat Fly | 9 | 2 | Greenpath |
| Giant Hopper | 9 | 5 | Kingdom's Edge |
| Mantis Heavy | 9 | 3 | Fog Canyon and Queen's Gardens |
| Shell | 9 | 4 | Greenpath, Fog Canyon and Queen's Gardens |
| Spitting Zombie | 9 | 6 | Forgotten Crossroads |
| Zote Balloon | 9 | 3 | Dreams, Godhome |
| Great Shield Zombie | 8 | 5 | City of Tears, Resting Grounds |
| Lesser Mawlek | 8 | 1 | Ancient Basin and Abyss |
| Moss Charger | 8 | 3 | Greenpath |
| Zoteling | 8 | 2 | Dreams, Godhome |
| Bursting Bouncer | 7 | 2 | Forgotten Crossroads |
| Cap Hit | 7 | 3 | Fungal Wastes |
| Hatcher | 7 | 4 | Forgotten Crossroads |
| Mushroom Brawler | 7 | 3 | Fungal Wastes |
| Zombie Runner Sp | 7 | 4 | Deepnest |
| Centipede Hatcher | 6 | 2 | Deepnest |
| Mage Knight | 6 | 5 | City of Tears, Colosseum of Fools, Godhome |
| Mantis Lord | 6 | 3 | Fungal Wastes, Godhome |
| Mossman_Runner | 6 | 5 | Greenpath |
| Parasite Balloon | 6 | 2 | Ancient Basin and Abyss |
| Zombie Hive | 6 | 3 | The Hive |
| Big Bee | 5 | 3 | The Hive |
| Blocker | 4 | 3 | Forgotten Crossroads, Greenpath |
| Dream Mage Lord Phase | 4 | 2 | Dreams, Godhome |
| Electric Mage New | 4 | 1 | Colosseum of Fools |
| Fat Fluke | 4 | 1 | Godhome |
| Ghost Warrior Markoth | 4 | 4 | Kingdom's Edge, Godhome |
| Giant Buzzer Col | 4 | 3 | Colosseum of Fools, Godhome |
| Grave Zombie | 4 | 1 | Resting Grounds |
| Grub Mimic | 4 | 2 | Crystal Peak, Deepnest |
| Hatcher NP | 4 | 1 | Forgotten Crossroads |
| Head | 4 | 4 | Forgotten Crossroads, Dreams, Godhome |
| Hornet Boss | 4 | 4 | Greenpath, Kingdom's Edge, Godhome |
| Mawlek Turret | 4 | 1 | Ancient Basin and Abyss |
| Mega Fat Bee | 4 | 2 | Colosseum of Fools, Godhome |
| Moss Knight | 4 | 2 | Greenpath |
| Real Bat | 4 | 4 | Grimm Troupe, Godhome |
| Royal Gaurd | 4 | 3 | White Palace |
| Zote Crew Normal | 4 | 1 | Godhome |
| Ghost Warrior Marmu | 3 | 3 | Fog Canyon and Queen's Gardens, Godhome |
| Ghost Warrior No Eyes | 3 | 3 | Greenpath, Godhome |
| Ghost Warrior Slug | 3 | 3 | Howling Cliffs, Godhome |
| Ghost Warrior Xero | 3 | 3 | Resting Grounds, Godhome |
| Giant Fly | 3 | 3 | Forgotten Crossroads, Godhome |
| Jar Collector | 3 | 3 | City of Tears, Godhome |
| Mawlek Body | 3 | 3 | Forgotten Crossroads, Godhome |
| Mimic Spider | 3 | 3 | Deepnest, Godhome |
| Moss Knight Fat | 3 | 1 | Fog Canyon and Queen's Gardens |
| Zombie Shield | 3 | 2 | Forgotten Crossroads |
| Zote Crew Fat | 3 | 1 | Godhome |
| Colosseum_Miner | 2 | 2 | Colosseum of Fools |
| Dream Mage Lord | 2 | 2 | Dreams, Godhome |
| Dung Defender | 2 | 2 | Royal Waterways, Godhome |
| False Knight Dream | 2 | 2 | Dreams, Godhome |
| False Knight New | 2 | 2 | Forgotten Crossroads, Godhome |
| Fluke Mother | 2 | 2 | Royal Waterways, Godhome |
| Ghost Warrior Galien | 2 | 2 | Deepnest, Godhome |
| Ghost Warrior Hu | 2 | 2 | Fungal Wastes, Godhome |
| Grey Prince | 2 | 2 | Dreams, Godhome |
| Grimm Boss | 2 | 2 | Grimm Troupe, Godhome |
| Hive Knight | 2 | 2 | The Hive, Godhome |
| Infected Knight | 2 | 2 | Ancient Basin and Abyss, Godhome |
| Lancer | 2 | 2 | Colosseum of Fools, Godhome |
| Lazy Flyer Enemy | 2 | 2 | Greenpath, Fog Canyon and Queen's Gardens |
| Lobster | 2 | 2 | Colosseum of Fools, Godhome |
| Lost Kin | 2 | 2 | Dreams, Godhome |
| Mage Lord | 2 | 2 | City of Tears, Godhome |
| Mage Lord Phase | 2 | 2 | City of Tears, Godhome |
| Mantis Traitor Lord | 2 | 2 | Fog Canyon and Queen's Gardens, Godhome |
| Mawlek Turret Ceiling | 2 | 1 | Ancient Basin and Abyss |
| Mega Jellyfish GG | 2 | 2 | Godhome |
| Mega Moss Charger | 2 | 2 | Greenpath, Godhome |
| Mega Zombie Beam Miner | 2 | 2 | Crystal Peak, Godhome |
| Nightmare Grimm Boss | 2 | 2 | Grimm Troupe, Godhome |
| Ordeal Zoteling | 2 | 1 | Godhome |
| Prayer Slug | 2 | 1 | Forgotten Crossroads |
| Tinger | 2 | 2 | Dreams, Godhome |
| White Defender | 2 | 2 | Dreams, Godhome |
| Zombie Beam Miner Rematch | 2 | 2 | Crystal Peak, Godhome |
| Zombie Fungus A | 2 | 2 | Fungal Wastes |
| Zombie Fungus B | 2 | 2 | Fungal Wastes |
| Zombie Guard | 2 | 2 | Forgotten Crossroads |
| Zote Crew Tall | 2 | 1 | Godhome |
| Zote Turret | 2 | 1 | Godhome |
| Absolute Radiance | 1 | 1 | Godhome |
| Crawler Fixed | 1 | 1 | King's Pass |
| Dummy Mantis | 1 | 1 | Fog Canyon and Queen's Gardens |
| Gate Mantis | 1 | 1 | Fungal Wastes |
| Giant Buzzer | 1 | 1 | Greenpath |
| Gorgeous Husk | 1 | 1 | City of Tears |
| Great Shield Zombie bottom | 1 | 1 | City of Tears |
| HK Prime | 1 | 1 | Godhome |
| Hollow Knight Boss | 1 | 1 | Hollow Knight finale |
| Hornet Nosk | 1 | 1 | Godhome |
| Mantis Heavy Spawn | 1 | 1 | Fog Canyon and Queen's Gardens |
| Mato | 1 | 1 | Godhome |
| Mega Jellyfish | 1 | 1 | Fog Canyon and Queen's Gardens |
| Mender Bug | 1 | 1 | Forgotten Crossroads |
| Moss Knight B | 1 | 1 | Greenpath |
| Moss Knight C | 1 | 1 | Greenpath |
| Oro | 1 | 1 | Godhome |
| Pale Lurker | 1 | 1 | Godhome |
| Plant Turret Right | 1 | 1 | Greenpath |
| Radiance | 1 | 1 | Dreams |
| Ruins Flying SentryB | 1 | 1 | City of Tears |
| Ruins Sentry Fat B | 1 | 1 | City of Tears |
| Ruins Sentry FatB | 1 | 1 | City of Tears |
| Ruins SentryB | 1 | 1 | City of Tears |
| Sheo Boss | 1 | 1 | Godhome |
| Sly Boss | 1 | 1 | Godhome |
| Zombie Myla | 1 | 1 | Forgotten Crossroads |
| Zote Balloon Ordeal | 1 | 1 | Godhome |
| Zote Boss | 1 | 1 | Godhome |
| Zote Fluke | 1 | 1 | Godhome |
| Zote Salubra | 1 | 1 | Godhome |
| Zote Thwomp | 1 | 1 | Godhome |

## Interactables

Counts by serialized component type or PlayMaker FSM name (`class_of`). `persistent` is PersistentBoolItem and PersistentIntItem, the objects the save must remember. `lever` is `Control Lever` plus BridgeLever; `Call Lever` is not counted. The behaviour-class census of every interactable is separate work.

| Class | Instances | Scenes |
| --- | ---: | ---: |
| breakable | 4749 | 320 |
| breakable_wall | 207 | 106 |
| reveal_mask | 422 | 171 |
| persistent | 2404 | 328 |
| geo_rock | 207 | 105 |
| bench | 51 | 51 |
| lever | 19 | 15 |
| switch | 63 | 47 |
| toll_gate | 42 | 29 |
| gate | 21 | 12 |
| door | 89 | 66 |
| chest | 19 | 12 |
| shiny | 112 | 91 |
| soul_totem | 60 | 53 |
| grub | 50 | 44 |
| lift | 21 | 18 |
| tram | 5 | 5 |
| stag | 11 | 11 |
| shop | 13 | 7 |
| conversation | 222 | 135 |
| inspect | 144 | 42 |
| battle | 45 | 45 |
| boss_door | 6 | 2 |
| boss_statue | 37 | 1 |

## Playable scenes

The rows of `SCENE_TABLE`. RAM is the resident scene bytes the arena must hold (arena: 354,032 B), disc is the stored pack, VRAM the atlas bytes.

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 6 | Tutorial_01 | playable | 84 | 309 | 609 | 177 | 7 | Buzzer x3, Crawler x3, Crawler Fixed x1 | breakable 43, breakable_wall 1, reveal_mask 8, persistent 24, geo_rock 5, chest 1, shiny 1, conversation 1 | 3 | Cliffs_02, Town | text |
| 7 | Town | playable | 72 | 115 | 205 | 58 | 0 |  | breakable 4, persistent 8, bench 1, switch 1, door 7, shiny 2, lift 1, conversation 10, inspect 1 | 12 | Cliffs_02, Crossroads_01, Grimm_Divine, Grimm_Tent, Mines_10, Room_Ouiji, Room_mapper, Tutorial_01 |  |
| 17 | Room_shop | playable | 6 | 35 | 35 | 18 | 0 |  | door 1, shop 2, conversation 1 | 2 | Town | text |
| 37 | Crossroads_01 | playable | 20 | 102 | 240 | 62 | 8 | Climber x2, Zombie Runner x2, Angry Buzzer x1 | breakable 24, reveal_mask 1, persistent 6, geo_rock 1, bench 1, door 1, shiny 1 | 5 | Crossroads_02, Crossroads_07, Town, bot1 |  |
| 38 | Crossroads_02 | playable | 12 | 61 | 103 | 35 | 0 |  | breakable 26, door 1 | 3 | Crossroads_01, Crossroads_39 |  |
| 39 | Crossroads_03 | playable | 12 | 98 | 206 | 61 | 10 | Angry Buzzer x3, Buzzer x2, Climber x2 | breakable 18, reveal_mask 1, persistent 6, toll_gate 2, grub 1 | 6 | Crossroads_15, Crossroads_16, Crossroads_19, Crossroads_21, Crossroads_47, Mines_33 |  |
| 40 | Crossroads_04 | playable | 21 | 107 | 238 | 63 | 16 | Fly x7, Bursting Zombie x2, Spitting Zombie x2 | breakable 24, breakable_wall 1, reveal_mask 2, persistent 13, bench 1, door 3, conversation 1, battle 1 | 6 | Crossroads_19, Crossroads_27, Crossroads_50, Room_ruinhouse |  |
| 41 | Crossroads_05 | playable | 8 | 77 | 138 | 46 | 11 | Crawler x3, Angry Buzzer x2, Bursting Zombie x2 | breakable 19, persistent 6, geo_rock 1, grub 1 | 2 | Crossroads_07, Crossroads_40 |  |
| 42 | Crossroads_06 | playable | 15 | 81 | 169 | 46 | 0 |  | breakable 22, persistent 2, switch 1, door 1 | 3 | Crossroads_10, Crossroads_33 |  |
| 43 | Crossroads_07 | playable | 16 | 111 | 205 | 67 | 23 | Fly x9, Climber x7, Bursting Bouncer x6 | breakable 28, breakable_wall 1, reveal_mask 1, persistent 39, geo_rock 3 | 6 | Crossroads_01, Crossroads_05, Crossroads_11_alt, Crossroads_25, Crossroads_33, Crossroads_38 |  |
| 44 | Crossroads_08 | playable | 12 | 89 | 204 | 54 | 6 | Spitter x4, Bursting Zombie x1, Spitting Zombie x1 | breakable 15, reveal_mask 1, persistent 5, geo_rock 4, battle 1 | 4 | Crossroads_13, Crossroads_18, Crossroads_30, Crossroads_33 |  |
| 45 | Crossroads_09 | playable | 8 | 61 | 103 | 36 | 1 | Mawlek Body x1 | breakable_wall 1, persistent 4, battle 1 | 2 | Crossroads_33, Crossroads_36 |  |
| 46 | Crossroads_10 | playable | 20 | 141 | 271 | 84 | 0 |  | breakable 36, breakable_wall 1, reveal_mask 1, persistent 3, geo_rock 1, door 1, chest 1, shiny 1, conversation 1 | 3 | Crossroads_06, Crossroads_21 | text |
| 50 | Crossroads_11_alt | playable | 18 | 115 | 239 | 69 | 3 | Angry Buzzer x2, Blocker x1 | breakable 8, persistent 4, inspect 1 | 2 | Crossroads_07, Fungus1_01 |  |
| 51 | Crossroads_12 | playable | 8 | 80 | 138 | 48 | 1 | Angry Buzzer x1 | breakable 7, persistent 1, geo_rock 1 | 2 | Crossroads_33, Crossroads_35 |  |
| 52 | Crossroads_13 | playable | 16 | 125 | 207 | 78 | 5 | Angry Buzzer x2, Bursting Zombie x1, Buzzer x1 | breakable 21, breakable_wall 1, persistent 6, geo_rock 2 | 2 | Crossroads_08, Crossroads_42 |  |
| 53 | Crossroads_14 | playable | 8 | 67 | 137 | 40 | 1 | Angry Buzzer x1 | breakable 7, persistent 1 | 4 | Crossroads_16, Crossroads_39, Crossroads_45, Crossroads_48 |  |
| 54 | Crossroads_15 | playable | 6 | 53 | 70 | 31 | 4 | Spitting Zombie x2, Zombie Shield x2 | breakable 15, persistent 4 | 2 | Crossroads_03, Crossroads_27 |  |
| 55 | Crossroads_16 | playable | 12 | 74 | 138 | 44 | 7 | Angry Buzzer x3, Buzzer x3, Zombie Hornhead x1 | breakable 17, persistent 4, geo_rock 1 | 3 | Crossroads_03, Crossroads_14, Crossroads_40 |  |
| 56 | Crossroads_18 | playable | 8 | 99 | 206 | 62 | 0 |  | breakable 17, breakable_wall 1, reveal_mask 2, persistent 3, geo_rock 3, soul_totem 1 | 3 | Crossroads_08, Crossroads_52, Fungus2_06 |  |
| 57 | Crossroads_19 | playable | 12 | 91 | 172 | 55 | 19 | Hatcher Baby Spawner x15, Hatcher x1, Spitter x1 | breakable 14, persistent 4, geo_rock 1, soul_totem 1 | 4 | Crossroads_03, Crossroads_04, Crossroads_42, Crossroads_43 |  |
| 58 | Crossroads_21 | playable | 15 | 100 | 203 | 59 | 9 | Angry Buzzer x2, Zombie Barger x2, Bursting Zombie x1 | breakable 24, breakable_wall 2, reveal_mask 3, persistent 13, geo_rock 1 | 4 | Crossroads_03, Crossroads_10, Crossroads_22 |  |
| 59 | Crossroads_22 | playable | 15 | 114 | 172 | 71 | 40 | Hatcher Baby Spawner x23, Spitter x12, Hatcher NP x4 | breakable 14, persistent 3, shiny 1, battle 1 | 1 | Crossroads_21 |  |
| 60 | Crossroads_25 | playable | 12 | 69 | 137 | 41 | 4 | Fly x3, Bursting Bouncer x1 | breakable 3, persistent 2, soul_totem 1 | 2 | Crossroads_07, Crossroads_36 |  |
| 61 | Crossroads_27 | playable | 10 | 86 | 172 | 54 | 18 | Hatcher Baby Spawner x15, Hatcher x3 | breakable 17, persistent 4, geo_rock 1 | 4 | Crossroads_04, Crossroads_15, Crossroads_31, Crossroads_46 |  |
| 62 | Crossroads_30 | playable | 6 | 56 | 103 | 33 | 0 |  | bench 1 | 1 | Crossroads_08 |  |
| 63 | Crossroads_31 | playable | 6 | 62 | 103 | 36 | 0 |  | breakable 8, persistent 1, grub 1 | 1 | Crossroads_27 |  |
| 64 | Crossroads_33 | playable | 12 | 89 | 172 | 53 | 0 |  | breakable 11, persistent 1, conversation 2 | 5 | Crossroads_06, Crossroads_07, Crossroads_08, Crossroads_09, Crossroads_12 |  |
| 65 | Crossroads_35 | playable | 24 | 108 | 239 | 65 | 17 | Hatcher Baby Spawner x15, Hatcher x2 | breakable 10, reveal_mask 1, persistent 5, soul_totem 1, grub 1 | 2 | Crossroads_12, Fungus3_26 |  |
| 66 | Crossroads_36 | playable | 12 | 114 | 205 | 70 | 0 |  | breakable 2, breakable_wall 2, reveal_mask 2, persistent 7, geo_rock 1, soul_totem 1 | 2 | Crossroads_09, Crossroads_25 |  |
| 67 | Crossroads_37 | playable | 15 | 98 | 171 | 60 | 14 | Zombie Barger x6, Zombie Hornhead x4, Zombie Leaper x2 | breakable 9, breakable_wall 1, reveal_mask 3, persistent 19, geo_rock 1 | 1 | Crossroads_49 |  |
| 68 | Crossroads_38 | playable | 6 | 57 | 103 | 34 | 0 |  | persistent 13, shiny 6 | 1 | Crossroads_07 |  |
| 69 | Crossroads_39 | playable | 8 | 65 | 137 | 38 | 6 | Bursting Zombie x2, Zombie Runner x2, Spitting Zombie x1 | breakable 27, persistent 6 | 2 | Crossroads_02, Crossroads_14 |  |
| 70 | Crossroads_40 | playable | 8 | 71 | 104 | 41 | 9 | Bursting Zombie x2, Spitting Zombie x2, Zombie Leaper x2 | breakable 10, persistent 8 | 2 | Crossroads_05, Crossroads_16 |  |
| 71 | Crossroads_42 | playable | 10 | 104 | 239 | 65 | 7 | Buzzer x4, Angry Buzzer x3 | breakable 23, persistent 3, geo_rock 2 | 2 | Crossroads_13, Crossroads_19 |  |
| 72 | Crossroads_43 | playable | 8 | 74 | 104 | 43 | 3 | Spitter x2, Zombie Shield x1 | breakable 21, persistent 1 | 2 | Crossroads_19, Crossroads_49 |  |
| 73 | Crossroads_45 | playable | 16 | 83 | 136 | 49 | 1 | Zombie Myla x1 | breakable 15, persistent 2, soul_totem 1, lift 1, conversation 2 | 2 | Crossroads_14, Mines_01 |  |
| 74 | Crossroads_46 | playable | 6 | 56 | 103 | 33 | 0 |  | breakable 1, reveal_mask 2, geo_rock 1, door 1, tram 1, conversation 1 | 2 | Crossroads_27 |  |
| 75 | Crossroads_46b | playable | 6 | 55 | 102 | 31 | 0 |  | breakable 1, door 1, tram 1, conversation 1 | 2 | RestingGrounds_02 |  |
| 76 | Crossroads_ShamanTemple | playable | 42 | 216 | 309 | 134 | 17 | Roller x11, Buzzer x3, Blocker x1 | breakable 47, reveal_mask 2, persistent 55, geo_rock 4, bench 1, gate 1, shiny 1, soul_totem 2, conversation 3 | 1 | Crossroads_06 |  |
| 77 | Crossroads_47 | playable | 6 | 47 | 69 | 26 | 0 |  | breakable 3, bench 1, door 1, stag 1, conversation 2 | 2 | Crossroads_03 |  |
| 78 | Crossroads_48 | playable | 6 | 53 | 102 | 30 | 1 | Zombie Guard x1 | breakable 5, persistent 2, grub 1 | 1 | Crossroads_14 |  |
| 79 | Crossroads_49 | playable | 24 | 56 | 68 | 26 | 0 |  | breakable 2, door 2 | 3 | Crossroads_37, Crossroads_43 |  |
| 80 | Crossroads_49b | playable | 12 | 49 | 69 | 26 | 0 |  | door 2 | 3 | Fungus2_07, Ruins1_28 |  |
| 81 | Crossroads_50 | playable | 55 | 134 | 239 | 73 | 6 | Buzzer x3, Climber x1, Crawler x1 | persistent 1, conversation 2 | 2 | Crossroads_04, RestingGrounds_06 |  |
| 82 | Crossroads_52 | playable | 15 | 60 | 103 | 34 | 0 |  | breakable 1, breakable_wall 1, reveal_mask 2, persistent 3, geo_rock 2, conversation 1 | 1 | Crossroads_18 |  |
| 128 | Fungus1_01 | playable | 24 | 164 | 307 | 104 | 16 | Pigeon x8, Moss Walker x4, Mossman_Shaker x2 | breakable 10, reveal_mask 1, persistent 5, geo_rock 1 | 2 | Crossroads_11_alt, Fungus1_01b |  |
| 129 | Fungus1_01b | playable | 12 | 94 | 203 | 59 | 18 | Pigeon x18 | breakable 4, geo_rock 1, bench 1 | 2 | Fungus1_01, Fungus1_02 |  |
| 130 | Fungus1_02 | playable | 21 | 135 | 273 | 84 | 2 | Mosquito x1, Mossman_Shaker x1 | breakable 7, persistent 1, geo_rock 2 | 3 | Fungus1_01b, Fungus1_06, Fungus1_17 |  |
| 134 | Fungus1_05 | playable | 12 | 134 | 306 | 89 | 12 | Mosquito x5, Moss Walker x4, Mossman_Shaker x2 | breakable 6, persistent 3, geo_rock 1 | 3 | Fungus1_03, Fungus1_10, Fungus1_14 |  |
| 135 | Fungus1_06 | playable | 24 | 139 | 271 | 87 | 7 | Mosquito x4, Mossman_Shaker x2, Mossman_Runner x1 | breakable 15, reveal_mask 2, persistent 14, grub 1, conversation 2 | 2 | Fungus1_02, Fungus1_07 |  |
| 136 | Fungus1_07 | playable | 20 | 124 | 237 | 78 | 5 | Mosquito x2, Mossman_Runner x2, Moss Walker x1 | breakable 4, reveal_mask 1, persistent 9, geo_rock 1, soul_totem 1, grub 1 | 3 | Fungus1_06, Fungus1_08, Fungus1_19 |  |
| 137 | Fungus1_08 | playable | 16 | 109 | 237 | 68 | 0 |  | breakable_wall 2, reveal_mask 1, persistent 5, shiny 2, conversation 2, inspect 1 | 1 | Fungus1_07 |  |
| 138 | Fungus1_09 | playable | 33 | 161 | 205 | 96 | 18 | Acid Flyer x18 | breakable 4, persistent 1 | 2 | Fungus1_15, Fungus1_30 |  |
| 139 | Fungus1_10 | playable | 24 | 163 | 273 | 98 | 7 | Moss Charger x4, Moss Walker x3 | breakable 22, persistent 6, geo_rock 1, shiny 1 | 3 | Fungus1_05, Fungus1_19, Fungus1_30 |  |
| 143 | Fungus1_14 | playable | 10 | 83 | 136 | 51 | 0 |  | persistent 2, shiny 1 | 1 | Fungus1_05 |  |
| 144 | Fungus1_15 | playable | 12 | 86 | 138 | 53 | 0 |  | breakable 4, persistent 1, bench 1, door 1, shiny 1 | 2 | Fungus1_09, Room_nailmaster_02 |  |
| 146 | Fungus1_17 | playable | 12 | 105 | 172 | 65 | 3 | Mosquito x2, Moss Charger x1 | breakable 6, reveal_mask 2, persistent 3, inspect 1 | 2 | Fungus1_02, Fungus1_03 |  |
| 147 | Fungus1_19 | playable | 9 | 110 | 273 | 71 | 7 | Fat Fly x4, Moss Walker x1, Mossman_Shaker x1 | breakable 8, reveal_mask 2, persistent 5, geo_rock 1, inspect 1 | 3 | Fungus1_07, Fungus1_10, Fungus1_11 |  |
| 157 | Fungus1_30 | playable | 15 | 131 | 206 | 83 | 0 |  | breakable 11, persistent 1, soul_totem 1, inspect 1 | 4 | Fungus1_09, Fungus1_10, Fungus1_22, Fungus1_31 |  |

## Menus, helpers and tests

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 0 | Pre_Menu_Intro | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 1 | Menu_Title | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 2 | Quit_To_Menu | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 3 | BetaEnd | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 4 | Knight_Pickup | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 415 | End_Credits | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 417 | Menu_Credits | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 418 | End_Game_Completion | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 419 | PermaDeath | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 420 | _test_cocoon_2 | cooked-pack | 6 | 34 | 35 | 17 | 0 |  | persistent 1 | 1 | _test_cocoon_1 |  |
| 421 | PermaDeath_Unlock | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 422 | _test_cocoon_1 | cooked-pack | 6 | 34 | 35 | 17 | 0 |  | persistent 2 | 1 | _test_cocoon_2 |  |

## Cinematics and openings

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 5 | Opening_Sequence | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 8 | Cinematic_Stag_travel | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 403 | Cutscene_Boss_Door | cooked-pack | 6 | 34 | 35 | 18 | 0 |  |  | 1 |  |  |
| 410 | Cinematic_Ending_A | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 411 | Cinematic_Ending_B | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 412 | Cinematic_Ending_C | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 413 | Cinematic_Ending_D | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 414 | Cinematic_Ending_E | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 416 | Cinematic_MrMushroom | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 499 | GG_Door_5_Finale | cooked-pack | 21 | 48 | 35 | 23 | 0 |  |  | 0 |  |  |

## King's Pass

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 6 | Tutorial_01 | playable | 84 | 309 | 609 | 177 | 7 | Buzzer x3, Crawler x3, Crawler Fixed x1 | breakable 43, breakable_wall 1, reveal_mask 8, persistent 24, geo_rock 5, chest 1, shiny 1, conversation 1 | 3 | Cliffs_02, Town | text |

## Dirtmouth

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 7 | Town | playable | 72 | 115 | 205 | 58 | 0 |  | breakable 4, persistent 8, bench 1, switch 1, door 7, shiny 2, lift 1, conversation 10, inspect 1 | 12 | Cliffs_02, Crossroads_01, Grimm_Divine, Grimm_Tent, Mines_10, Room_Ouiji, Room_mapper, Tutorial_01 |  |

## Interior rooms

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 9 | Room_Town_Stag_Station | cooked-pack | 9 | 55 | 70 | 30 | 0 |  | breakable 1, persistent 1, switch 1, door 1, lift 1, stag 1, conversation 1 | 2 | Town |  |
| 10 | Room_Charm_Shop | cooked-pack | 4 | 34 | 35 | 18 | 0 |  | shop 2, conversation 1 | 1 | Crossroads_04 | text |
| 11 | Room_Mender_House | cooked-pack | 4 | 34 | 35 | 18 | 0 |  | conversation 1 | 1 | Crossroads_04 |  |
| 12 | Room_mapper | cooked-pack | 4 | 34 | 35 | 18 | 0 |  | shop 2, conversation 1 | 1 | Town | text |
| 13 | Room_nailmaster | cooked-pack | 12 | 48 | 68 | 26 | 0 |  | reveal_mask 3, persistent 3, bench 1, conversation 2 | 1 | Cliffs_02 |  |
| 14 | Room_nailmaster_02 | cooked-pack | 9 | 39 | 35 | 20 | 0 |  | conversation 4 | 1 | Fungus1_15 |  |
| 15 | Room_nailmaster_03 | cooked-pack | 9 | 38 | 35 | 20 | 0 |  | breakable 4, conversation 1 | 1 | Deepnest_East_06 |  |
| 16 | Room_nailsmith | cooked-pack | 4 | 34 | 35 | 18 | 0 |  | conversation 1 | 1 | Ruins1_04 |  |
| 17 | Room_shop | playable | 6 | 35 | 35 | 18 | 0 |  | door 1, shop 2, conversation 1 | 2 | Town | text |
| 18 | Room_Sly_Storeroom | cooked-pack | 15 | 51 | 69 | 27 | 0 |  | breakable 6, door 1, shop 2, conversation 1 | 2 | Room_shop | text |
| 19 | Room_temple | cooked-pack | 9 | 43 | 35 | 22 | 0 |  | breakable 7, persistent 1, door 1, conversation 2 | 2 | Crossroads_02 |  |
| 20 | Room_ruinhouse | cooked-pack | 4 | 33 | 35 | 17 | 0 |  | conversation 1 | 1 | Crossroads_04 |  |
| 21 | Room_Mask_Maker | cooked-pack | 6 | 42 | 36 | 22 | 0 |  | conversation 1 | 1 | Deepnest_42 |  |
| 22 | Room_Mansion | cooked-pack | 6 | 37 | 35 | 19 | 0 |  | breakable 1, persistent 1, conversation 1 | 1 | RestingGrounds_12 |  |
| 25 | Room_Bretta | cooked-pack | 4 | 35 | 35 | 18 | 0 |  | persistent 1, door 1, conversation 1 | 2 | Town |  |
| 26 | Room_Bretta_Basement | cooked-pack | 4 | 36 | 35 | 19 | 0 |  | door 1 | 3 | Room_Bretta |  |
| 27 | Room_Fungus_Shaman | cooked-pack | 21 | 93 | 169 | 54 | 7 | Mosquito x7 | breakable 7, breakable_wall 1, reveal_mask 1, persistent 3, geo_rock 1, battle 1 | 1 | Fungus3_44 |  |
| 28 | Room_Ouiji | cooked-pack | 9 | 46 | 36 | 25 | 0 |  | breakable 8, conversation 1 | 1 | Town |  |
| 29 | Room_Jinn | cooked-pack | 9 | 44 | 68 | 23 | 0 |  | conversation 2 | 1 | Town |  |
| 36 | Room_Slug_Shrine | cooked-pack | 4 | 38 | 36 | 21 | 0 |  | bench 1, conversation 1 | 1 | Fungus1_26 |  |
| 326 | Room_Wyrm | cooked-pack | 15 | 57 | 68 | 30 | 0 |  | persistent 1, shiny 1 | 1 | Deepnest_East_Hornet |  |
| 348 | Room_Queen | cooked-pack | 16 | 61 | 69 | 33 | 0 |  | reveal_mask 1, persistent 3, shiny 1, conversation 1 | 1 | Fungus3_48 | text |

## Tram

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 23 | Room_Tram | cooked-pack | 6 | 38 | 35 | 20 | 0 |  | breakable 5, persistent 2, bench 1, door 1 | 1 |  |  |
| 24 | Room_Tram_RG | cooked-pack | 6 | 38 | 35 | 20 | 0 |  | breakable 5, persistent 2, bench 1, door 1 | 1 |  |  |

## Forgotten Crossroads

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 37 | Crossroads_01 | playable | 20 | 102 | 240 | 62 | 8 | Climber x2, Zombie Runner x2, Angry Buzzer x1 | breakable 24, reveal_mask 1, persistent 6, geo_rock 1, bench 1, door 1, shiny 1 | 5 | Crossroads_02, Crossroads_07, Town, bot1 |  |
| 38 | Crossroads_02 | playable | 12 | 61 | 103 | 35 | 0 |  | breakable 26, door 1 | 3 | Crossroads_01, Crossroads_39 |  |
| 39 | Crossroads_03 | playable | 12 | 98 | 206 | 61 | 10 | Angry Buzzer x3, Buzzer x2, Climber x2 | breakable 18, reveal_mask 1, persistent 6, toll_gate 2, grub 1 | 6 | Crossroads_15, Crossroads_16, Crossroads_19, Crossroads_21, Crossroads_47, Mines_33 |  |
| 40 | Crossroads_04 | playable | 21 | 107 | 238 | 63 | 16 | Fly x7, Bursting Zombie x2, Spitting Zombie x2 | breakable 24, breakable_wall 1, reveal_mask 2, persistent 13, bench 1, door 3, conversation 1, battle 1 | 6 | Crossroads_19, Crossroads_27, Crossroads_50, Room_ruinhouse |  |
| 41 | Crossroads_05 | playable | 8 | 77 | 138 | 46 | 11 | Crawler x3, Angry Buzzer x2, Bursting Zombie x2 | breakable 19, persistent 6, geo_rock 1, grub 1 | 2 | Crossroads_07, Crossroads_40 |  |
| 42 | Crossroads_06 | playable | 15 | 81 | 169 | 46 | 0 |  | breakable 22, persistent 2, switch 1, door 1 | 3 | Crossroads_10, Crossroads_33 |  |
| 43 | Crossroads_07 | playable | 16 | 111 | 205 | 67 | 23 | Fly x9, Climber x7, Bursting Bouncer x6 | breakable 28, breakable_wall 1, reveal_mask 1, persistent 39, geo_rock 3 | 6 | Crossroads_01, Crossroads_05, Crossroads_11_alt, Crossroads_25, Crossroads_33, Crossroads_38 |  |
| 44 | Crossroads_08 | playable | 12 | 89 | 204 | 54 | 6 | Spitter x4, Bursting Zombie x1, Spitting Zombie x1 | breakable 15, reveal_mask 1, persistent 5, geo_rock 4, battle 1 | 4 | Crossroads_13, Crossroads_18, Crossroads_30, Crossroads_33 |  |
| 45 | Crossroads_09 | playable | 8 | 61 | 103 | 36 | 1 | Mawlek Body x1 | breakable_wall 1, persistent 4, battle 1 | 2 | Crossroads_33, Crossroads_36 |  |
| 46 | Crossroads_10 | playable | 20 | 141 | 271 | 84 | 0 |  | breakable 36, breakable_wall 1, reveal_mask 1, persistent 3, geo_rock 1, door 1, chest 1, shiny 1, conversation 1 | 3 | Crossroads_06, Crossroads_21 | text |
| 47 | Crossroads_10_preload | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 48 | Crossroads_10_boss | no-envelope |  |  |  |  | 5 | False Knight New x1, Head x1, Zombie Barger x1 | persistent 4, battle 1 | 0 |  |  |
| 49 | Crossroads_10_boss_defeated | no-envelope |  |  |  |  | 2 | Prayer Slug x2 | breakable 8, persistent 2 | 0 |  |  |
| 50 | Crossroads_11_alt | playable | 18 | 115 | 239 | 69 | 3 | Angry Buzzer x2, Blocker x1 | breakable 8, persistent 4, inspect 1 | 2 | Crossroads_07, Fungus1_01 |  |
| 51 | Crossroads_12 | playable | 8 | 80 | 138 | 48 | 1 | Angry Buzzer x1 | breakable 7, persistent 1, geo_rock 1 | 2 | Crossroads_33, Crossroads_35 |  |
| 52 | Crossroads_13 | playable | 16 | 125 | 207 | 78 | 5 | Angry Buzzer x2, Bursting Zombie x1, Buzzer x1 | breakable 21, breakable_wall 1, persistent 6, geo_rock 2 | 2 | Crossroads_08, Crossroads_42 |  |
| 53 | Crossroads_14 | playable | 8 | 67 | 137 | 40 | 1 | Angry Buzzer x1 | breakable 7, persistent 1 | 4 | Crossroads_16, Crossroads_39, Crossroads_45, Crossroads_48 |  |
| 54 | Crossroads_15 | playable | 6 | 53 | 70 | 31 | 4 | Spitting Zombie x2, Zombie Shield x2 | breakable 15, persistent 4 | 2 | Crossroads_03, Crossroads_27 |  |
| 55 | Crossroads_16 | playable | 12 | 74 | 138 | 44 | 7 | Angry Buzzer x3, Buzzer x3, Zombie Hornhead x1 | breakable 17, persistent 4, geo_rock 1 | 3 | Crossroads_03, Crossroads_14, Crossroads_40 |  |
| 56 | Crossroads_18 | playable | 8 | 99 | 206 | 62 | 0 |  | breakable 17, breakable_wall 1, reveal_mask 2, persistent 3, geo_rock 3, soul_totem 1 | 3 | Crossroads_08, Crossroads_52, Fungus2_06 |  |
| 57 | Crossroads_19 | playable | 12 | 91 | 172 | 55 | 19 | Hatcher Baby Spawner x15, Hatcher x1, Spitter x1 | breakable 14, persistent 4, geo_rock 1, soul_totem 1 | 4 | Crossroads_03, Crossroads_04, Crossroads_42, Crossroads_43 |  |
| 58 | Crossroads_21 | playable | 15 | 100 | 203 | 59 | 9 | Angry Buzzer x2, Zombie Barger x2, Bursting Zombie x1 | breakable 24, breakable_wall 2, reveal_mask 3, persistent 13, geo_rock 1 | 4 | Crossroads_03, Crossroads_10, Crossroads_22 |  |
| 59 | Crossroads_22 | playable | 15 | 114 | 172 | 71 | 40 | Hatcher Baby Spawner x23, Spitter x12, Hatcher NP x4 | breakable 14, persistent 3, shiny 1, battle 1 | 1 | Crossroads_21 |  |
| 60 | Crossroads_25 | playable | 12 | 69 | 137 | 41 | 4 | Fly x3, Bursting Bouncer x1 | breakable 3, persistent 2, soul_totem 1 | 2 | Crossroads_07, Crossroads_36 |  |
| 61 | Crossroads_27 | playable | 10 | 86 | 172 | 54 | 18 | Hatcher Baby Spawner x15, Hatcher x3 | breakable 17, persistent 4, geo_rock 1 | 4 | Crossroads_04, Crossroads_15, Crossroads_31, Crossroads_46 |  |
| 62 | Crossroads_30 | playable | 6 | 56 | 103 | 33 | 0 |  | bench 1 | 1 | Crossroads_08 |  |
| 63 | Crossroads_31 | playable | 6 | 62 | 103 | 36 | 0 |  | breakable 8, persistent 1, grub 1 | 1 | Crossroads_27 |  |
| 64 | Crossroads_33 | playable | 12 | 89 | 172 | 53 | 0 |  | breakable 11, persistent 1, conversation 2 | 5 | Crossroads_06, Crossroads_07, Crossroads_08, Crossroads_09, Crossroads_12 |  |
| 65 | Crossroads_35 | playable | 24 | 108 | 239 | 65 | 17 | Hatcher Baby Spawner x15, Hatcher x2 | breakable 10, reveal_mask 1, persistent 5, soul_totem 1, grub 1 | 2 | Crossroads_12, Fungus3_26 |  |
| 66 | Crossroads_36 | playable | 12 | 114 | 205 | 70 | 0 |  | breakable 2, breakable_wall 2, reveal_mask 2, persistent 7, geo_rock 1, soul_totem 1 | 2 | Crossroads_09, Crossroads_25 |  |
| 67 | Crossroads_37 | playable | 15 | 98 | 171 | 60 | 14 | Zombie Barger x6, Zombie Hornhead x4, Zombie Leaper x2 | breakable 9, breakable_wall 1, reveal_mask 3, persistent 19, geo_rock 1 | 1 | Crossroads_49 |  |
| 68 | Crossroads_38 | playable | 6 | 57 | 103 | 34 | 0 |  | persistent 13, shiny 6 | 1 | Crossroads_07 |  |
| 69 | Crossroads_39 | playable | 8 | 65 | 137 | 38 | 6 | Bursting Zombie x2, Zombie Runner x2, Spitting Zombie x1 | breakable 27, persistent 6 | 2 | Crossroads_02, Crossroads_14 |  |
| 70 | Crossroads_40 | playable | 8 | 71 | 104 | 41 | 9 | Bursting Zombie x2, Spitting Zombie x2, Zombie Leaper x2 | breakable 10, persistent 8 | 2 | Crossroads_05, Crossroads_16 |  |
| 71 | Crossroads_42 | playable | 10 | 104 | 239 | 65 | 7 | Buzzer x4, Angry Buzzer x3 | breakable 23, persistent 3, geo_rock 2 | 2 | Crossroads_13, Crossroads_19 |  |
| 72 | Crossroads_43 | playable | 8 | 74 | 104 | 43 | 3 | Spitter x2, Zombie Shield x1 | breakable 21, persistent 1 | 2 | Crossroads_19, Crossroads_49 |  |
| 73 | Crossroads_45 | playable | 16 | 83 | 136 | 49 | 1 | Zombie Myla x1 | breakable 15, persistent 2, soul_totem 1, lift 1, conversation 2 | 2 | Crossroads_14, Mines_01 |  |
| 74 | Crossroads_46 | playable | 6 | 56 | 103 | 33 | 0 |  | breakable 1, reveal_mask 2, geo_rock 1, door 1, tram 1, conversation 1 | 2 | Crossroads_27 |  |
| 75 | Crossroads_46b | playable | 6 | 55 | 102 | 31 | 0 |  | breakable 1, door 1, tram 1, conversation 1 | 2 | RestingGrounds_02 |  |
| 76 | Crossroads_ShamanTemple | playable | 42 | 216 | 309 | 134 | 17 | Roller x11, Buzzer x3, Blocker x1 | breakable 47, reveal_mask 2, persistent 55, geo_rock 4, bench 1, gate 1, shiny 1, soul_totem 2, conversation 3 | 1 | Crossroads_06 |  |
| 77 | Crossroads_47 | playable | 6 | 47 | 69 | 26 | 0 |  | breakable 3, bench 1, door 1, stag 1, conversation 2 | 2 | Crossroads_03 |  |
| 78 | Crossroads_48 | playable | 6 | 53 | 102 | 30 | 1 | Zombie Guard x1 | breakable 5, persistent 2, grub 1 | 1 | Crossroads_14 |  |
| 79 | Crossroads_49 | playable | 24 | 56 | 68 | 26 | 0 |  | breakable 2, door 2 | 3 | Crossroads_37, Crossroads_43 |  |
| 80 | Crossroads_49b | playable | 12 | 49 | 69 | 26 | 0 |  | door 2 | 3 | Fungus2_07, Ruins1_28 |  |
| 81 | Crossroads_50 | playable | 55 | 134 | 239 | 73 | 6 | Buzzer x3, Climber x1, Crawler x1 | persistent 1, conversation 2 | 2 | Crossroads_04, RestingGrounds_06 |  |
| 82 | Crossroads_52 | playable | 15 | 60 | 103 | 34 | 0 |  | breakable 1, breakable_wall 1, reveal_mask 2, persistent 3, geo_rock 2, conversation 1 | 1 | Crossroads_18 |  |

## Greenpath

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 128 | Fungus1_01 | playable | 24 | 164 | 307 | 104 | 16 | Pigeon x8, Moss Walker x4, Mossman_Shaker x2 | breakable 10, reveal_mask 1, persistent 5, geo_rock 1 | 2 | Crossroads_11_alt, Fungus1_01b |  |
| 129 | Fungus1_01b | playable | 12 | 94 | 203 | 59 | 18 | Pigeon x18 | breakable 4, geo_rock 1, bench 1 | 2 | Fungus1_01, Fungus1_02 |  |
| 130 | Fungus1_02 | playable | 21 | 135 | 273 | 84 | 2 | Mosquito x1, Mossman_Shaker x1 | breakable 7, persistent 1, geo_rock 2 | 3 | Fungus1_01b, Fungus1_06, Fungus1_17 |  |
| 131 | Fungus1_03 | cooked-pack | 13 | 123 | 238 | 76 | 14 | Pigeon x5, Moss Walker x4, Zombie Runner x2 | breakable 22, persistent 5, geo_rock 3 | 3 | Fungus1_05, Fungus1_17, Fungus1_31 |  |
| 132 | Fungus1_04 | cooked-pack | 20 | 120 | 238 | 76 | 0 |  | breakable 7, breakable_wall 1, reveal_mask 1, persistent 5, geo_rock 1, shiny 1 | 2 | Fungus1_21, Fungus1_25 | text |
| 133 | Fungus1_04_boss | no-envelope |  |  |  |  | 1 | Hornet Boss x1 |  | 0 |  |  |
| 134 | Fungus1_05 | playable | 12 | 134 | 306 | 89 | 12 | Mosquito x5, Moss Walker x4, Mossman_Shaker x2 | breakable 6, persistent 3, geo_rock 1 | 3 | Fungus1_03, Fungus1_10, Fungus1_14 |  |
| 135 | Fungus1_06 | playable | 24 | 139 | 271 | 87 | 7 | Mosquito x4, Mossman_Shaker x2, Mossman_Runner x1 | breakable 15, reveal_mask 2, persistent 14, grub 1, conversation 2 | 2 | Fungus1_02, Fungus1_07 |  |
| 136 | Fungus1_07 | playable | 20 | 124 | 237 | 78 | 5 | Mosquito x2, Mossman_Runner x2, Moss Walker x1 | breakable 4, reveal_mask 1, persistent 9, geo_rock 1, soul_totem 1, grub 1 | 3 | Fungus1_06, Fungus1_08, Fungus1_19 |  |
| 137 | Fungus1_08 | playable | 16 | 109 | 237 | 68 | 0 |  | breakable_wall 2, reveal_mask 1, persistent 5, shiny 2, conversation 2, inspect 1 | 1 | Fungus1_07 |  |
| 138 | Fungus1_09 | playable | 33 | 161 | 205 | 96 | 18 | Acid Flyer x18 | breakable 4, persistent 1 | 2 | Fungus1_15, Fungus1_30 |  |
| 139 | Fungus1_10 | playable | 24 | 163 | 273 | 98 | 7 | Moss Charger x4, Moss Walker x3 | breakable 22, persistent 6, geo_rock 1, shiny 1 | 3 | Fungus1_05, Fungus1_19, Fungus1_30 |  |
| 140 | Fungus1_11 | cooked-pack | 19 | 142 | 306 | 90 | 6 | Plant Turret x3, Acid Walker x1, Moss Walker x1 | breakable 15, reveal_mask 1, persistent 3, shiny 1 | 5 | Fungus1_19, Fungus1_29, Fungus1_34, Fungus1_37, Fungus3_01 |  |
| 141 | Fungus1_12 | cooked-pack | 10 | 98 | 172 | 62 | 15 | Pigeon x7, Plant Turret x5, Acid Walker x1 | breakable 6, breakable_wall 1, reveal_mask 3, persistent 4, geo_rock 3 | 2 | Fungus1_13, Fungus1_29 |  |
| 142 | Fungus1_13 | cooked-pack | 40 | 194 | 306 | 119 | 14 | Acid Walker x6, Shell x6, Moss Walker x2 | breakable 7, persistent 57, chest 1, grub 1, inspect 1 | 2 | Fungus1_12, Fungus3_22 |  |
| 143 | Fungus1_14 | playable | 10 | 83 | 136 | 51 | 0 |  | persistent 2, shiny 1 | 1 | Fungus1_05 |  |
| 144 | Fungus1_15 | playable | 12 | 86 | 138 | 53 | 0 |  | breakable 4, persistent 1, bench 1, door 1, shiny 1 | 2 | Fungus1_09, Room_nailmaster_02 |  |
| 145 | Fungus1_16_alt | cooked-pack | 6 | 60 | 136 | 36 | 0 |  | breakable 1, bench 1, door 1, stag 1, conversation 1 | 2 | Fungus1_22 |  |
| 146 | Fungus1_17 | playable | 12 | 105 | 172 | 65 | 3 | Mosquito x2, Moss Charger x1 | breakable 6, reveal_mask 2, persistent 3, inspect 1 | 2 | Fungus1_02, Fungus1_03 |  |
| 147 | Fungus1_19 | playable | 9 | 110 | 273 | 71 | 7 | Fat Fly x4, Moss Walker x1, Mossman_Shaker x1 | breakable 8, reveal_mask 2, persistent 5, geo_rock 1, inspect 1 | 3 | Fungus1_07, Fungus1_10, Fungus1_11 |  |
| 148 | Fungus1_20_v02 | cooked-pack | 16 | 133 | 272 | 84 | 8 | Buzzer x4, Crawler x2, Climber x1 | breakable 15 | 3 | Fungus1_21, Fungus1_28, Fungus1_32 |  |
| 149 | Fungus1_21 | cooked-pack | 17 | 161 | 339 | 105 | 7 | Moss Charger x3, Moss Knight x3, Moss Walker x1 | breakable 12, reveal_mask 1, persistent 11, geo_rock 3, grub 1, inspect 1, battle 1 | 4 | Fungus1_04, Fungus1_20_v02, Fungus1_22, Fungus1_32 |  |
| 150 | Fungus1_22 | cooked-pack | 32 | 212 | 373 | 133 | 13 | Plant Trap x6, Mossman_Shaker x3, Mosquito x2 | breakable 17, breakable_wall 1, reveal_mask 5, persistent 14, geo_rock 2, switch 1, gate 1, shiny 1 | 3 | Fungus1_16_alt, Fungus1_21, Fungus1_30 |  |
| 151 | Fungus1_23 | cooked-pack | 21 | 116 | 237 | 70 | 13 | Grass Hopper x8, Pigeon x5 | breakable 4 | 2 | Fungus3_13, Fungus3_48 |  |
| 152 | Fungus1_24 | cooked-pack | 21 | 143 | 272 | 89 | 7 | Pigeon x7 | breakable 7, breakable_wall 1, reveal_mask 1, persistent 4, bench 1, conversation 3 | 1 | Fungus3_05 |  |
| 153 | Fungus1_25 | cooked-pack | 8 | 80 | 169 | 50 | 0 |  | breakable 4 | 2 | Fungus1_04, Fungus1_26 |  |
| 154 | Fungus1_26 | cooked-pack | 25 | 101 | 203 | 61 | 9 | Pigeon x7, Lazy Flyer Enemy x1, Moss Knight x1 | breakable 5, reveal_mask 2, persistent 4, door 1 | 3 | Fungus1_25, Fungus1_Slug, Room_Slug_Shrine |  |
| 155 | Fungus1_28 | cooked-pack | 35 | 194 | 405 | 123 | 8 | Crawler x3, Blocker x2, Climber x2 | breakable 24, breakable_wall 2, reveal_mask 5, persistent 9, geo_rock 2, chest 1, shiny 1, grub 1 | 2 | Cliffs_01, Fungus1_20_v02 |  |
| 156 | Fungus1_29 | cooked-pack | 12 | 108 | 203 | 67 | 6 | Plant Turret x3, Moss Walker x2, Mega Moss Charger x1 | breakable 5, persistent 1, geo_rock 1, soul_totem 1 | 2 | Fungus1_11, Fungus1_12 |  |
| 157 | Fungus1_30 | playable | 15 | 131 | 206 | 83 | 0 |  | breakable 11, persistent 1, soul_totem 1, inspect 1 | 4 | Fungus1_09, Fungus1_10, Fungus1_22, Fungus1_31 |  |
| 158 | Fungus1_31 | cooked-pack | 33 | 216 | 376 | 133 | 11 | Fat Fly x5, Moss Walker x3, Mossman_Shaker x2 | breakable 9, breakable_wall 1, reveal_mask 1, persistent 6, geo_rock 3, bench 1, toll_gate 2 | 3 | Fungus1_03, Fungus1_30, Fungus1_32 |  |
| 159 | Fungus1_32 | cooked-pack | 18 | 128 | 273 | 83 | 3 | Moss Knight B x1, Moss Knight C x1, Moss Walker x1 | breakable 14, breakable_wall 1, reveal_mask 2, persistent 5, inspect 1, battle 1 | 3 | Fungus1_20_v02, Fungus1_21, Fungus1_31 |  |
| 160 | Fungus1_34 | cooked-pack | 18 | 71 | 70 | 39 | 5 | Moss Walker x1, Plant Trap x1, Zombie Hornhead x1 | breakable 3, persistent 4, door 1 | 2 | Fungus1_11, Fungus1_35 |  |
| 161 | Fungus1_35 | cooked-pack | 18 | 96 | 103 | 55 | 1 | Ghost Warrior No Eyes x1 | breakable 1, conversation 1, inspect 1 | 2 | Fungus1_34, Fungus1_36 |  |
| 162 | Fungus1_36 | cooked-pack | 4 | 46 | 69 | 26 | 5 | Pigeon x5 | breakable 1, reveal_mask 1, persistent 2 | 1 | Fungus1_35 |  |
| 163 | Fungus1_37 | cooked-pack | 4 | 45 | 69 | 26 | 0 |  | bench 1 | 1 | Fungus1_11 |  |
| 164 | Fungus1_Slug | cooked-pack | 32 | 114 | 135 | 66 | 0 |  | breakable_wall 2, reveal_mask 3, persistent 7, shiny 2 | 1 | Fungus1_26 |  |

## Fungal Wastes

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 165 | Fungus2_01 | cooked-pack | 15 | 145 | 206 | 85 | 0 |  | breakable 5, reveal_mask 1, persistent 3, switch 1, toll_gate 1, conversation 1 | 4 | Fungus2_02, Fungus2_03, Fungus2_34, Fungus3_02 |  |
| 166 | Fungus2_02 | cooked-pack | 6 | 53 | 102 | 30 | 0 |  | breakable 1, bench 1, door 1, stag 1, conversation 1 | 2 | Fungus2_01 |  |
| 167 | Fungus2_03 | cooked-pack | 36 | 131 | 172 | 79 | 13 | Mushroom Turret x5, Fungoon Baby x3, Fungus Flyer x3 | breakable 46, persistent 6, shiny 1 | 3 | Fungus2_01, Fungus2_04, Fungus2_18 |  |
| 168 | Fungus2_04 | cooked-pack | 12 | 108 | 172 | 70 | 4 | Mushroom Turret x3, Fung Crawler x1 | breakable 28, persistent 3, geo_rock 1, switch 2, gate 2, shiny 1, inspect 1 | 4 | Fungus2_03, Fungus2_05, Fungus2_28 |  |
| 169 | Fungus2_05 | cooked-pack | 12 | 74 | 138 | 44 | 12 | Mushroom Baby x6, Cap Hit x3, Mushroom Brawler x3 | breakable 12, persistent 4, shiny 2, battle 1 | 2 | Fungus2_04, Fungus2_06 |  |
| 170 | Fungus2_06 | cooked-pack | 22 | 175 | 275 | 116 | 6 | Fungus Flyer x4, Fung Crawler x2 | breakable 42, persistent 4 | 5 | Crossroads_18, Fungus2_05, Fungus2_07, Fungus2_26, Fungus2_33 |  |
| 171 | Fungus2_07 | cooked-pack | 12 | 81 | 104 | 50 | 5 | Mushroom Baby x3, Mushroom Roller x2 | breakable 17, reveal_mask 2, persistent 4, inspect 1 | 2 | Fungus2_06, Fungus2_08 |  |
| 172 | Fungus2_08 | cooked-pack | 10 | 92 | 138 | 59 | 4 | Mushroom Turret x3, Fungus Flyer x1 | breakable 19, persistent 1, geo_rock 1 | 3 | Fungus2_07, Fungus2_09, Fungus2_32 |  |
| 173 | Fungus2_09 | cooked-pack | 8 | 71 | 104 | 44 | 0 |  | breakable 17, conversation 1 | 2 | Fungus2_08, Fungus2_10 |  |
| 174 | Fungus2_10 | cooked-pack | 24 | 139 | 206 | 89 | 2 | Zombie Fungus A x1, Zombie Fungus B x1 | breakable 45, breakable_wall 1, reveal_mask 1, persistent 5, geo_rock 1, soul_totem 1 | 3 | Fungus2_09, Fungus2_11, Fungus2_21 |  |
| 175 | Fungus2_11 | cooked-pack | 10 | 97 | 139 | 63 | 5 | Fung Crawler x2, Mushroom Turret x2, Fungus Flyer x1 | breakable 24, persistent 1, geo_rock 2 | 4 | Fungus2_10, Fungus2_12, Fungus2_17, Fungus2_18 |  |
| 176 | Fungus2_12 | cooked-pack | 24 | 128 | 205 | 81 | 7 | Mantis Flyer Child x4, Mantis x3 | breakable 31, persistent 3, inspect 1 | 2 | Fungus2_11, Fungus2_13 |  |
| 177 | Fungus2_13 | cooked-pack | 16 | 146 | 240 | 97 | 9 | Mantis x5, Mantis Flyer Child x4 | breakable 49, persistent 5, geo_rock 3, bench 1 | 3 | Fungus2_12, Fungus2_14, Fungus2_23 |  |
| 178 | Fungus2_14 | cooked-pack | 34 | 186 | 344 | 119 | 8 | Mantis Flyer Child x5, Mantis x2, Gate Mantis x1 | breakable 23, persistent 6, geo_rock 5, switch 2, gate 3, shiny 1, conversation 1, inspect 1 | 5 | Fungus2_13, Fungus2_15, Fungus2_17 |  |
| 179 | Fungus2_15 | cooked-pack | 28 | 206 | 380 | 138 | 7 | Mantis Flyer Child x4, Mantis x3 | breakable 31, breakable_wall 1, reveal_mask 1, persistent 10, geo_rock 2, switch 5, gate 6 | 5 | Fungus2_14, Fungus2_25, Fungus2_31 |  |
| 180 | Fungus2_15_boss | no-envelope |  |  |  |  | 6 | Mantis Lord S x4, Mantis Lord x2 | battle 1 | 0 |  |  |
| 181 | Fungus2_15_boss_defeated | no-envelope |  |  |  |  | 0 |  |  | 0 |  |  |
| 182 | Fungus2_17 | cooked-pack | 12 | 95 | 138 | 62 | 7 | Fungoon Baby x5, Fung Crawler x2 | breakable 20, persistent 20, shiny 1 | 3 | Fungus2_11, Fungus2_14, Fungus2_29 |  |
| 183 | Fungus2_18 | cooked-pack | 35 | 196 | 241 | 126 | 15 | Fungoon Baby x11, Fung Crawler x2, Fungus Flyer x1 | breakable 85, persistent 5, geo_rock 4, switch 1, gate 1, grub 1, conversation 3 | 3 | Fungus2_03, Fungus2_11, Fungus2_19 |  |
| 184 | Fungus2_19 | cooked-pack | 9 | 71 | 104 | 44 | 2 | Fungoon Baby x2 |  | 2 | Fungus2_18, Fungus2_20 |  |
| 185 | Fungus2_20 | cooked-pack | 12 | 98 | 139 | 64 | 1 | Mushroom Turret x1 | breakable 19, breakable_wall 2, reveal_mask 3, persistent 6, shiny 1, grub 1, inspect 1 | 2 | Deepnest_01, Fungus2_19 |  |
| 186 | Fungus2_21 | cooked-pack | 28 | 128 | 206 | 79 | 1 | Fung Crawler x1 | breakable 3, breakable_wall 1, reveal_mask 3, persistent 7, geo_rock 1, lever 2, soul_totem 1, conversation 1, inspect 1 | 2 | Fungus2_10, Ruins1_01 |  |
| 187 | Fungus2_23 | cooked-pack | 20 | 133 | 206 | 87 | 8 | Mushroom Baby x5, Mushroom Roller x2, Mushroom Turret x1 | breakable 20, breakable_wall 1, reveal_mask 1, persistent 5, gate 1, shiny 1, conversation 1 | 2 | Fungus2_13, Waterways_09 |  |
| 188 | Fungus2_25 | cooked-pack | 24 | 114 | 170 | 68 | 0 |  | breakable 14, breakable_wall 2, persistent 3, geo_rock 3, conversation 1 | 4 | Deepnest_16, Fungus2_15, Fungus2_30 |  |
| 189 | Fungus2_26 | cooked-pack | 6 | 63 | 104 | 39 | 0 |  | breakable 10, bench 1, shop 2, conversation 1 | 1 | Fungus2_06 | text |
| 190 | Fungus2_28 | cooked-pack | 15 | 98 | 137 | 63 | 17 | Mushroom Baby x11, Mushroom Roller x4, Fung Crawler x2 | breakable 35, persistent 4 | 2 | Fungus2_04 |  |
| 191 | Fungus2_29 | cooked-pack | 25 | 175 | 239 | 115 | 21 | Fungoon Baby x6, Mushroom Roller x4, Mushroom Turret x4 | breakable 71, breakable_wall 1, reveal_mask 3, persistent 14, geo_rock 1, shiny 1, soul_totem 1 | 2 | Fungus2_17, Fungus2_30 |  |
| 192 | Fungus2_30 | cooked-pack | 40 | 179 | 207 | 114 | 31 | Fungoon Baby x9, Mushroom Baby x7, Fung Crawler x4 | breakable 58, reveal_mask 1, persistent 10, geo_rock 2, inspect 1 | 2 | Fungus2_25, Fungus2_29 |  |
| 193 | Fungus2_31 | cooked-pack | 12 | 82 | 138 | 51 | 1 | Mantis x1 | breakable 9, reveal_mask 2, persistent 9, bench 1, chest 3, shiny 2 | 1 | Fungus2_15 |  |
| 194 | Fungus2_32 | cooked-pack | 8 | 57 | 69 | 35 | 1 | Ghost Warrior Hu x1 | breakable 21, conversation 1, inspect 1 | 1 | Fungus2_08 |  |
| 195 | Fungus2_33 | cooked-pack | 18 | 73 | 69 | 42 | 5 | Fungoon Baby x3, Fungus Flyer x2 | breakable 5, persistent 23 | 2 | Fungus2_06, Fungus3_26 |  |
| 196 | Fungus2_34 | cooked-pack | 6 | 56 | 71 | 32 | 0 |  | reveal_mask 3, persistent 4, shiny 1, conversation 2 | 1 | Fungus2_01 |  |

## Fog Canyon and Queen's Gardens

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 197 | Fungus3_01 | cooked-pack | 12 | 110 | 172 | 69 | 13 | Jellyfish Baby x7, Jellyfish x6 | breakable 9, persistent 6 | 4 | Fungus1_11, Fungus3_02, Fungus3_24, Fungus3_25 |  |
| 198 | Fungus3_02 | cooked-pack | 14 | 133 | 272 | 86 | 15 | Jellyfish Baby x10, Jellyfish x5 | breakable 11, persistent 5 | 5 | Fungus2_01, Fungus3_01, Fungus3_03, Fungus3_35, Fungus3_47 |  |
| 199 | Fungus3_03 | cooked-pack | 10 | 85 | 203 | 53 | 0 |  | geo_rock 1 | 2 | Fungus3_02, Fungus3_34 |  |
| 200 | Fungus3_04 | cooked-pack | 14 | 108 | 203 | 66 | 4 | Mantis Heavy Flyer x2, Moss Flyer x2 | breakable 15, persistent 4, switch 1 | 4 | Fungus3_05, Fungus3_13, Fungus3_21, Fungus3_34 |  |
| 201 | Fungus3_05 | cooked-pack | 20 | 105 | 170 | 63 | 8 | Mantis Heavy Flyer x5, Plant Trap x3 | breakable 9, breakable_wall 1, reveal_mask 1, persistent 9, switch 1, gate 1, battle 1 | 3 | Fungus1_24, Fungus3_04, Fungus3_11 |  |
| 202 | Fungus3_08 | cooked-pack | 21 | 92 | 137 | 51 | 8 | Mantis Heavy Flyer x4, Moss Flyer x4 | breakable 6, persistent 4 | 3 | Deepnest_43, Fungus3_10, Fungus3_11 |  |
| 203 | Fungus3_10 | cooked-pack | 20 | 103 | 169 | 60 | 16 | Garden Zombie x4, Mantis Heavy x4, Mantis Heavy Flyer x4 | breakable 12, reveal_mask 1, persistent 8, geo_rock 1, grub 1, battle 1 | 2 | Fungus3_08, Fungus3_13 |  |
| 204 | Fungus3_11 | cooked-pack | 18 | 97 | 136 | 57 | 5 | Moss Flyer x5 | breakable 9, persistent 30 | 3 | Fungus3_05, Fungus3_08, Fungus3_39 |  |
| 205 | Fungus3_13 | cooked-pack | 10 | 78 | 170 | 47 | 6 | Moss Flyer x6 | breakable 5 | 5 | Fungus1_23, Fungus3_04, Fungus3_10, Fungus3_40, Fungus3_49 |  |
| 206 | Fungus3_21 | cooked-pack | 12 | 74 | 102 | 43 | 0 |  | breakable 3, persistent 1, soul_totem 1 | 2 | Fungus3_04, Fungus3_22 |  |
| 207 | Fungus3_22 | cooked-pack | 20 | 142 | 271 | 89 | 9 | Moss Flyer x4, Garden Zombie x3, Mantis Heavy Flyer x2 | breakable 9, persistent 7, grub 1 | 3 | Fungus1_13, Fungus3_21, Fungus3_23 |  |
| 208 | Fungus3_23 | cooked-pack | 20 | 108 | 202 | 65 | 0 |  | breakable 8, reveal_mask 1, persistent 2, conversation 1 | 2 | Fungus3_22, Fungus3_48 |  |
| 209 | Fungus3_23_boss | no-envelope |  |  |  |  | 5 | Mantis Heavy x3, Dummy Mantis x1, Mantis Traitor Lord x1 | persistent 1, battle 1 | 0 |  |  |
| 210 | Fungus3_24 | cooked-pack | 15 | 90 | 170 | 55 | 0 |  | breakable 3 | 3 | Fungus3_01, Fungus3_30, Fungus3_44 |  |
| 211 | Fungus3_25 | cooked-pack | 20 | 91 | 136 | 54 | 19 | Jellyfish Baby x13, Jellyfish x6 | breakable 1, persistent 7, conversation 2 | 2 | Fungus3_01, Fungus3_25b |  |
| 212 | Fungus3_25b | cooked-pack | 15 | 100 | 172 | 61 | 15 | Jellyfish Baby x12, Jellyfish x3 | breakable 5, persistent 3 | 2 | Fungus3_25, Fungus3_26 |  |
| 213 | Fungus3_26 | cooked-pack | 16 | 97 | 137 | 60 | 15 | Jellyfish Baby x10, Jellyfish x5 | breakable 5, reveal_mask 1, persistent 7, geo_rock 1, shiny 1 | 5 | Crossroads_35, Fungus2_33, Fungus3_25b, Fungus3_27, Fungus3_28 |  |
| 214 | Fungus3_27 | cooked-pack | 8 | 75 | 137 | 46 | 13 | Jellyfish Baby x9, Jellyfish x4 | breakable 3, persistent 4 | 2 | Fungus3_26, Fungus3_47 |  |
| 215 | Fungus3_28 | cooked-pack | 13 | 103 | 137 | 66 | 15 | Jellyfish Baby x10, Jellyfish x5 | breakable 2, breakable_wall 1, persistent 7, shiny 1 | 1 | Fungus3_26 |  |
| 216 | Fungus3_30 | cooked-pack | 6 | 65 | 136 | 40 | 0 |  | breakable 9, persistent 2, shiny 1 | 1 | Fungus3_24 |  |
| 217 | Fungus3_34 | cooked-pack | 21 | 129 | 336 | 81 | 7 | Garden Zombie x3, Moss Flyer x3, Egg Sac x1 | breakable 11, persistent 4, conversation 1 | 3 | Fungus3_03, Fungus3_04, Fungus3_44 |  |
| 218 | Fungus3_35 | cooked-pack | 6 | 67 | 136 | 41 | 0 |  | breakable 6, shop 1, conversation 1 | 1 | Fungus3_02 | text |
| 219 | Fungus3_39 | cooked-pack | 20 | 125 | 237 | 78 | 12 | Moss Flyer x4, Moss Knight Fat x3, Mantis Heavy x2 | breakable_wall 1, reveal_mask 1, persistent 11, geo_rock 2, shiny 1, conversation 4, battle 1 | 2 | Deepnest_01, Fungus3_11 |  |
| 220 | Fungus3_40 | cooked-pack | 24 | 131 | 238 | 80 | 6 | Mantis Heavy Flyer x2, Moss Flyer x2, Moss Walker x2 | breakable 1, reveal_mask 3, persistent 8, bench 1, switch 1, door 1, soul_totem 1, stag 1, conversation 1, inspect 1 | 3 | Fungus3_13, Fungus3_48 |  |
| 221 | Fungus3_40_boss | no-envelope |  |  |  |  | 1 | Ghost Warrior Marmu x1 | conversation 1 | 0 |  |  |
| 222 | Fungus3_44 | cooked-pack | 25 | 122 | 236 | 76 | 5 | Moss Flyer x5 | breakable 2, breakable_wall 1, reveal_mask 1, persistent 3, switch 1, gate 1, door 1, conversation 1 | 3 | Fungus3_24, Fungus3_34, Room_Fung_Shaman |  |
| 223 | Fungus3_47 | cooked-pack | 16 | 78 | 135 | 45 | 11 | Jellyfish x11 | breakable 4, breakable_wall 1, reveal_mask 1, persistent 3, door 1, grub 1, conversation 1 | 3 | Fungus3_02, Fungus3_27, Fungus3_archive |  |
| 224 | Fungus3_48 | cooked-pack | 40 | 142 | 169 | 84 | 26 | Plant Trap x12, Grass Hopper x9, Mantis Heavy Flyer x2 | reveal_mask 2, persistent 18, geo_rock 1, door 1, shiny 1, grub 1 | 4 | Fungus1_23, Fungus3_23, Fungus3_40 |  |
| 225 | Fungus3_49 | cooked-pack | 12 | 63 | 69 | 35 | 0 |  | conversation 2 | 1 | Fungus3_13 |  |
| 226 | Fungus3_50 | cooked-pack | 18 | 53 | 69 | 27 | 0 |  | persistent 2, bench 1 | 1 | Deepnest_43 |  |
| 227 | Fungus3_archive | cooked-pack | 15 | 80 | 103 | 44 | 0 |  | breakable 9, bench 1 | 2 | Fungus3_47, Fungus3_archive_02 |  |
| 228 | Fungus3_archive_02 | cooked-pack | 65 | 219 | 204 | 113 | 41 | Jellyfish Baby Inert x24, Jellyfish Baby x10, Jellyfish x7 | breakable 34, persistent 7, conversation 4, inspect 3 | 2 | Fungus3_archive |  |
| 229 | Fungus3_archive_02_boss | no-envelope |  |  |  |  | 1 | Mega Jellyfish x1 |  | 0 |  |  |

## City of Tears

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 83 | Ruins_House_01 | cooked-pack | 6 | 39 | 35 | 20 | 1 | Great Shield Zombie x1 | breakable 3, persistent 2, grub 1 | 1 | Ruins2_04 |  |
| 84 | Ruins_House_02 | cooked-pack | 6 | 41 | 36 | 22 | 5 | Royal Zombie Coward x2, Gorgeous Husk x1, Royal Zombie x1 | breakable 6, reveal_mask 1, persistent 9 | 1 | Ruins2_04 |  |
| 85 | Ruins_House_03 | cooked-pack | 15 | 56 | 69 | 30 | 0 |  | breakable 2, persistent 3, switch 1, conversation 1 | 2 | Ruins2_04, Waterways_07 |  |
| 86 | Ruins_Elevator | cooked-pack | 55 | 97 | 135 | 46 | 0 |  | breakable 19, breakable_wall 1, reveal_mask 6, persistent 11, geo_rock 1, lever 1, shiny 2, lift 1, conversation 1, inspect 1 | 2 | Ruins2_04, Ruins_Bathhouse |  |
| 87 | Ruins_Bathhouse | cooked-pack | 16 | 79 | 170 | 45 | 0 |  | breakable_wall 1, reveal_mask 4, persistent 3, bench 1, door 1, conversation 2 | 2 | Ruins2_10b, Ruins_Elevator |  |
| 88 | Ruins1_01 | cooked-pack | 18 | 95 | 170 | 53 | 6 | Ceiling Dropper x2, Ruins Sentry x1, Zombie Barger x1 | breakable 31, persistent 6 | 3 | Fungus2_21, Ruins1_02, Ruins1_17 |  |
| 89 | Ruins1_02 | cooked-pack | 18 | 105 | 204 | 59 | 2 | Ruins Sentry x2 | breakable 32, reveal_mask 1, persistent 4, bench 1, lever 1, lift 1, conversation 1, inspect 1 | 2 | Ruins1_01, Ruins1_03 |  |
| 90 | Ruins1_03 | cooked-pack | 35 | 163 | 274 | 93 | 12 | Ruins Sentry x4, Ruins Flying Sentry x3, Buzzer x2 | breakable 40, persistent 12, geo_rock 1, lever 1, shiny 1, lift 1, inspect 1 | 4 | Ruins1_02, Ruins1_04, Ruins1_05b, Ruins1_05c |  |
| 91 | Ruins1_04 | cooked-pack | 35 | 127 | 173 | 76 | 6 | Buzzer x3, Ruins Sentry x1, Zombie Barger x1 | breakable 23, breakable_wall 1, reveal_mask 3, persistent 4, door 1, conversation 1 | 3 | Room_GG_Shortcut, Room_nailmsith, Ruins1_03 |  |
| 92 | Ruins1_05 | cooked-pack | 37 | 137 | 239 | 75 | 9 | Ruins Flying Sentry x2, Ceiling Dropper x1, Ruins Flying Sentry Javelin x1 | breakable 29, persistent 11, lever 1, switch 1, toll_gate 1, grub 1, lift 1, battle 1 | 6 | Ruins1_05c, Ruins1_09, Ruins1_18, Ruins1_31 |  |
| 93 | Ruins1_05b | cooked-pack | 9 | 73 | 170 | 42 | 0 |  | breakable 13, reveal_mask 3, persistent 2, geo_rock 1, lever 1, switch 1, toll_gate 1, door 1, lift 1, shop 2, conversation 3 | 4 | Ruins1_03, Ruins1_05c, Ruins1_27, Waterways_01 | text |
| 94 | Ruins1_05c | cooked-pack | 34 | 153 | 305 | 86 | 11 | Ceiling Dropper x3, Buzzer x2, Ruins Sentry x2 | breakable 33, reveal_mask 2, persistent 15, geo_rock 1, lever 1, switch 2, toll_gate 4, shiny 1, lift 1 | 5 | Ruins1_03, Ruins1_05, Ruins1_05b |  |
| 95 | Ruins1_06 | cooked-pack | 15 | 87 | 136 | 47 | 0 |  | breakable 22, conversation 1 | 2 | Ruins1_17, Ruins1_31 |  |
| 96 | Ruins1_09 | cooked-pack | 12 | 54 | 70 | 29 | 1 | Mage x1 | breakable 8, persistent 2, battle 1 | 2 | Ruins1_05, Ruins1_23 |  |
| 97 | Ruins1_17 | cooked-pack | 24 | 121 | 236 | 68 | 8 | Ruins Flying Sentry x2, Ruins Flying Sentry Javelin x2, Buzzer x1 | breakable 33, reveal_mask 6, persistent 44, switch 1, toll_gate 1, shiny 1 | 3 | Ruins1_01, Ruins1_06, Ruins1_28 |  |
| 98 | Ruins1_18 | cooked-pack | 12 | 78 | 104 | 43 | 0 |  | breakable 16, persistent 1, bench 1, switch 1, toll_gate 1 | 3 | Ruins1_05, Ruins2_01, Ruins2_03b |  |
| 99 | Ruins1_23 | cooked-pack | 21 | 115 | 236 | 65 | 7 | Mage Blob x4, Mage x2, Mage Knight x1 | breakable 29, persistent 10, lever 2, switch 2, toll_gate 2, lift 2, inspect 1, battle 1 | 5 | Ruins1_09, Ruins1_25, Ruins1_30, Ruins1_31 |  |
| 100 | Ruins1_30 | cooked-pack | 20 | 133 | 237 | 77 | 16 | Mage Blob x9, Mage Balloon x4, Mage x3 | breakable 51, breakable_wall 3, reveal_mask 7, persistent 19, switch 1, toll_gate 1, shiny 1 | 4 | Ruins1_23, Ruins1_24, Ruins1_25 |  |
| 101 | Ruins1_24 | cooked-pack | 20 | 117 | 238 | 69 | 0 |  | breakable 3, reveal_mask 1, persistent 5, door 1, soul_totem 1 | 5 | Ruins1_30, Ruins1_32 |  |
| 102 | Ruins1_24_boss | no-envelope |  |  |  |  | 2 | Mage Lord x1, Mage Lord Phase x1 |  | 0 |  |  |
| 103 | Ruins1_24_boss_defeated | no-envelope |  |  |  |  | 0 |  | conversation 1 | 0 |  |  |
| 104 | Ruins1_25 | cooked-pack | 16 | 109 | 203 | 62 | 7 | Mage Blob x5, Mage x2 | breakable 25, persistent 6, lever 1, switch 1, toll_gate 1, lift 1 | 3 | Ruins1_23, Ruins1_30 |  |
| 105 | Ruins1_27 | cooked-pack | 16 | 80 | 170 | 45 | 4 | Royal Zombie x2, Royal Zombie Coward x2 | breakable 9, reveal_mask 1, persistent 6, switch 1, toll_gate 1, conversation 2 | 2 | Ruins1_05b, Ruins2_01_b | text |
| 106 | Ruins1_28 | cooked-pack | 18 | 116 | 206 | 66 | 4 | Zombie Runner x2, Ruins Flying Sentry x1, Zombie Barger x1 | breakable 67, persistent 7, shiny 1 | 3 | Crossroads_49b, Ruins1_17, Ruins1_29 |  |
| 107 | Ruins1_29 | cooked-pack | 6 | 53 | 70 | 29 | 0 |  | breakable 9, bench 1, door 1, stag 1, conversation 1 | 2 | Ruins1_28 | text |
| 108 | Ruins1_31 | cooked-pack | 18 | 87 | 170 | 48 | 0 |  | breakable 8, breakable_wall 1, reveal_mask 1, persistent 6, bench 1, lever 1, toll_gate 1, door 1, lift 1, conversation 3 | 5 | Ruins1_05, Ruins1_06, Ruins1_23, Ruins1_31b |  |
| 109 | Ruins1_31b | cooked-pack | 18 | 75 | 137 | 42 | 25 | Mage Balloon Spawner x24, Mage Knight x1 | breakable 7, persistent 2, switch 1, toll_gate 1, battle 1 | 2 | Ruins1_31 |  |
| 110 | Ruins1_32 | cooked-pack | 24 | 133 | 237 | 79 | 28 | Mage Blob x16, Mage Balloon x12 | breakable 24, breakable_wall 7, reveal_mask 3, persistent 18, geo_rock 1, switch 2, toll_gate 2, chest 1, shiny 1, soul_totem 2, grub 1, inspect 1 | 2 | Ruins1_24 |  |
| 111 | Ruins2_01 | cooked-pack | 28 | 104 | 170 | 57 | 13 | Royal Zombie Coward x3, Royal Zombie x2, Royal Zombie Fat x2 | breakable 32, reveal_mask 2, persistent 18, geo_rock 1, switch 2, toll_gate 2, battle 1 | 3 | Ruins1_18, Ruins2_01_b, Ruins2_03b |  |
| 112 | Ruins2_01_b | cooked-pack | 16 | 80 | 136 | 44 | 4 | Royal Zombie Fat x2, Royal Zombie x1, Royal Zombie Coward x1 | breakable 15, breakable_wall 1, persistent 8, lever 2, lift 2, battle 1 | 3 | Ruins1_27, Ruins2_01, Ruins2_04 |  |
| 113 | Ruins2_03 | cooked-pack | 36 | 119 | 171 | 64 | 2 | Great Shield Zombie x1, Ruins Flying Sentry Javelin x1 | breakable 22, breakable_wall 1, reveal_mask 8, persistent 9, lever 1, chest 1, shiny 1, grub 1, lift 1, battle 1 | 3 | Ruins2_03b, Ruins2_Watcher_Room |  |
| 114 | Ruins2_03b | cooked-pack | 36 | 94 | 136 | 47 | 7 | Royal Zombie x2, Ruins Flying Sentry x2, Great Shield Zombie bottom x1 | breakable 19, persistent 9, lever 1, lift 1, battle 1 | 4 | Ruins1_18, Ruins2_01, Ruins2_03 |  |
| 115 | Ruins2_03_boss | no-envelope |  |  |  |  | 6 | Black Knight x6 | persistent 1, battle 1 | 0 |  |  |
| 116 | Ruins2_04 | cooked-pack | 35 | 137 | 172 | 74 | 10 | Great Shield Zombie x4, Royal Zombie x2, Royal Zombie Coward x2 | breakable 21, persistent 10, door 4, conversation 1 | 8 | Abyss_01, Ruins2_01_b, Ruins2_06, Ruins_Elevator, Ruins_House_01, Ruins_House_02, Ruins_House_03 |  |
| 117 | Ruins2_05 | cooked-pack | 18 | 107 | 203 | 60 | 16 | Ruins Sentry x4, Royal Zombie x3, Royal Zombie Coward x3 | breakable 11, persistent 19, geo_rock 1, lever 2, shiny 1, lift 2 | 3 | Ruins2_06, Ruins2_09, Ruins2_10b |  |
| 118 | Ruins2_06 | cooked-pack | 13 | 121 | 139 | 67 | 4 | Royal Zombie x1, Royal Zombie Fat x1, Ruins Flying Sentry Javelin x1 | breakable 34, reveal_mask 2, persistent 6, geo_rock 1 | 5 | Ruins2_04, Ruins2_05, Ruins2_07, Ruins2_08 |  |
| 119 | Ruins2_07 | cooked-pack | 12 | 98 | 205 | 60 | 5 | Ceiling Dropper x3, Buzzer x2 | breakable 3, persistent 6, grub 1 | 3 | Deepnest_East_03, Ruins2_06, Ruins2_11_b |  |
| 120 | Ruins2_08 | cooked-pack | 9 | 62 | 103 | 35 | 0 |  | breakable 3, persistent 1, bench 1, door 1, shiny 1, stag 1, conversation 1 | 2 | Ruins2_06 |  |
| 121 | Ruins2_09 | cooked-pack | 8 | 70 | 137 | 42 | 13 | Ruins Flying Sentry x3, Ruins Sentry Fat x3, Ruins Flying Sentry Javelin x2 | breakable 9, persistent 5, battle 1 | 1 | Ruins2_05 |  |
| 122 | Ruins2_10 | cooked-pack | 24 | 54 | 69 | 26 | 0 |  | breakable 1, door 2 | 3 | RestingGrounds_06, RestingGrounds_10 |  |
| 123 | Ruins2_10b | cooked-pack | 24 | 80 | 136 | 43 | 0 |  | breakable 4, door 2 | 4 | Deepnest_East_09, Ruins2_05, Ruins_Bathhouse |  |
| 124 | Ruins2_11 | cooked-pack | 40 | 101 | 169 | 53 | 2 | Buzzer x2 | reveal_mask 3, persistent 19, shiny 1, grub 3 | 1 | Ruins2_11_b |  |
| 125 | Ruins2_11_b | cooked-pack | 40 | 81 | 101 | 36 | 0 |  | reveal_mask 2, persistent 6, switch 1, toll_gate 2, conversation 1 | 3 | Deepnest_East_07, Ruins2_07, Ruins2_11 |  |
| 126 | Ruins2_11_boss | no-envelope |  |  |  |  | 1 | Jar Collector x1 | persistent 1 | 0 |  |  |
| 127 | Ruins2_Watcher_Room | cooked-pack | 44 | 113 | 170 | 57 | 1 | Royal Zombie Coward x1 | breakable 22, reveal_mask 2, persistent 5, lever 1, lift 1, conversation 1, inspect 2 | 2 | Ruins2_03 |  |

## Resting Grounds

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 237 | RestingGrounds_02 | cooked-pack | 18 | 85 | 170 | 48 | 0 |  | breakable 17, inspect 1 | 4 | Crossroads_46b, Mines_28, RestingGrounds_04, RestingGrounds_06 |  |
| 238 | RestingGrounds_02_boss | no-envelope |  |  |  |  | 1 | Ghost Warrior Xero x1 | conversation 1 | 0 |  |  |
| 239 | RestingGrounds_04 | cooked-pack | 15 | 70 | 136 | 40 | 0 |  | breakable 14, conversation 1 | 2 | RestingGrounds_02, RestingGrounds_05 | text |
| 240 | RestingGrounds_05 | cooked-pack | 21 | 108 | 203 | 61 | 0 |  | breakable 7, breakable_wall 1, persistent 23, soul_totem 1 | 6 | RestingGrounds_04, RestingGrounds_07, RestingGrounds_08, RestingGrounds_09, RestingGrounds_10, RestingGrounds_17 |  |
| 241 | RestingGrounds_06 | cooked-pack | 15 | 79 | 170 | 46 | 2 | Great Shield Zombie x1, Ruins Flying Sentry x1 | breakable 8, breakable_wall 1, reveal_mask 2, persistent 7, switch 1, soul_totem 1 | 3 | Crossroads_50, RestingGrounds_02, Ruins2_10 |  |
| 242 | RestingGrounds_07 | cooked-pack | 9 | 44 | 69 | 24 | 0 |  | reveal_mask 1, persistent 1, conversation 1 | 2 | RestingGrounds_05 |  |
| 243 | RestingGrounds_08 | cooked-pack | 60 | 186 | 336 | 105 | 1 | Ruins Sentry x1 | breakable 22, reveal_mask 1, persistent 57, door 1, shiny 1, conversation 19 | 2 | RestingGrounds_05 |  |
| 244 | RestingGrounds_09 | cooked-pack | 6 | 54 | 103 | 31 | 0 |  | breakable 1, persistent 2, bench 1, switch 1, door 1, stag 1, conversation 2 | 2 | RestingGrounds_05 | text |
| 245 | RestingGrounds_10 | cooked-pack | 30 | 135 | 267 | 79 | 8 | Ceiling Dropper x4, Grave Zombie x4 | breakable 5, breakable_wall 17, reveal_mask 23, persistent 35, geo_rock 3, chest 1, shiny 3, grub 1 | 3 | RestingGrounds_05, RestingGrounds_12, Ruins2_10 |  |
| 246 | RestingGrounds_12 | cooked-pack | 15 | 62 | 135 | 37 | 0 |  | persistent 1, bench 1, door 1 | 2 | RestingGrounds_10, Room_Mansion |  |
| 247 | RestingGrounds_17 | cooked-pack | 8 | 57 | 103 | 33 | 0 |  | breakable 2, persistent 1, shiny 1 | 1 | RestingGrounds_05 |  |

## Howling Cliffs

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 230 | Cliffs_01 | cooked-pack | 73 | 308 | 578 | 202 | 21 | Crawler x8, Climber x7, Buzzer x6 | breakable 5, breakable_wall 3, reveal_mask 7, persistent 54, geo_rock 4, shiny 2, soul_totem 1, conversation 2 | 4 | Cliffs_02, Cliffs_04, Cliffs_06, Fungus1_28 |  |
| 231 | Cliffs_02 | cooked-pack | 100 | 238 | 476 | 142 | 10 | Buzzer x3, Crawler x3, Climber x2 | breakable 5, breakable_wall 1, reveal_mask 4, persistent 9, geo_rock 2, door 1, soul_totem 1, inspect 1 | 6 | Cliffs_01, Cliffs_03, Room_nailmaster, Town, Tutorial_01 |  |
| 232 | Cliffs_02_boss | no-envelope |  |  |  |  | 1 | Ghost Warrior Slug x1 | conversation 1 | 0 |  |  |
| 233 | Cliffs_03 | cooked-pack | 40 | 142 | 239 | 80 | 0 |  | breakable 4, lift 1, stag 1, conversation 2 | 2 | Cliffs_02 |  |
| 234 | Cliffs_04 | cooked-pack | 20 | 129 | 272 | 83 | 13 | Buzzer x8, Zombie Leaper x3, Zombie Barger x1 | breakable 15, breakable_wall 1, reveal_mask 1, persistent 8, soul_totem 1 | 2 | Cliffs_01, Cliffs_05 |  |
| 235 | Cliffs_05 | cooked-pack | 20 | 86 | 138 | 53 | 0 |  | breakable 21, reveal_mask 1, persistent 4, shiny 1, conversation 1 | 1 | Cliffs_04 |  |
| 236 | Cliffs_06 | cooked-pack | 10 | 59 | 103 | 34 | 0 |  | reveal_mask 2, persistent 3, conversation 1, inspect 1 | 1 | Cliffs_01 |  |

## Crystal Peak

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 248 | Mines_01 | cooked-pack | 10 | 79 | 137 | 47 | 1 | Egg Sac x1 | breakable 18, persistent 2 | 2 | Crossroads_45, Mines_02 |  |
| 249 | Mines_02 | cooked-pack | 21 | 172 | 309 | 110 | 4 | Crystal Crawler x2, Zombie Miner x2 | breakable 30, persistent 2, geo_rock 3 | 4 | Mines_01, Mines_03, Mines_29, Mines_33 |  |
| 250 | Mines_03 | cooked-pack | 12 | 121 | 206 | 77 | 5 | Zombie Miner x3, Crystal Crawler x2 | breakable 11, persistent 5, switch 1, toll_gate 1, grub 1 | 3 | Mines_02, Mines_05, Mines_17 |  |
| 251 | Mines_04 | cooked-pack | 12 | 110 | 239 | 70 | 6 | Crystal Flyer x4, Zombie Miner x2 | breakable 12, persistent 8, geo_rock 2, switch 1, toll_gate 1, grub 1 | 5 | Mines_07, Mines_17, Mines_29, Mines_37 |  |
| 252 | Mines_05 | cooked-pack | 12 | 105 | 206 | 65 | 11 | Crystallised Lazer Bug x10, Zombie Miner x1 | breakable 11, breakable_wall 1, persistent 12, geo_rock 1 | 5 | Mines_03, Mines_06, Mines_11, Mines_19, Mines_30 |  |
| 253 | Mines_06 | cooked-pack | 21 | 100 | 138 | 59 | 0 |  | breakable 12, breakable_wall 1, reveal_mask 1, persistent 2 | 2 | Mines_05, Mines_36 |  |
| 254 | Mines_07 | cooked-pack | 12 | 77 | 170 | 47 | 3 | Crystal Flyer x2, Crystal Crawler x1 | breakable 2, persistent 2 | 2 | Mines_04, Mines_28 |  |
| 255 | Mines_10 | cooked-pack | 16 | 110 | 205 | 68 | 0 |  | breakable 6, persistent 1 | 3 | Mines_16, Mines_30, Town |  |
| 256 | Mines_11 | cooked-pack | 10 | 95 | 139 | 60 | 9 | Crystallised Lazer Bug x5, Mines Crawler x2, Crystal Crawler x1 | breakable 34, persistent 7, shiny 1 | 3 | Mines_05, Mines_13, Mines_18 |  |
| 257 | Mines_13 | cooked-pack | 20 | 83 | 169 | 47 | 0 |  | breakable 8, conversation 1 | 3 | Mines_11, Mines_20, Mines_34 |  |
| 258 | Mines_16 | cooked-pack | 9 | 89 | 204 | 57 | 3 | Crystal Flyer x2, Grub Mimic x1 | breakable 6, breakable_wall 1, reveal_mask 4, persistent 6, geo_rock 1, grub 2 | 1 | Mines_10 |  |
| 259 | Mines_17 | cooked-pack | 12 | 69 | 137 | 41 | 2 | Zombie Miner x2 | breakable 1, persistent 2 | 2 | Mines_03, Mines_04 |  |
| 260 | Mines_18 | cooked-pack | 9 | 66 | 136 | 40 | 0 |  | breakable 5, bench 1 | 3 | Mines_11, Mines_20, Mines_32 |  |
| 261 | Mines_18_boss | no-envelope |  |  |  |  | 1 | Mega Zombie Beam Miner x1 | persistent 2 | 0 |  |  |
| 262 | Mines_19 | cooked-pack | 9 | 70 | 136 | 41 | 0 |  | persistent 3, switch 2, gate 1, grub 1 | 2 | Mines_05, Mines_20 |  |
| 263 | Mines_20 | cooked-pack | 52 | 203 | 339 | 121 | 18 | Crystallised Lazer Bug x7, Crystal Flyer x4, Zombie Miner x3 | breakable 27, breakable_wall 1, persistent 21, geo_rock 3, switch 3, toll_gate 1, gate 2, shiny 1, soul_totem 1 | 6 | Mines_13, Mines_18, Mines_19, Mines_23, Mines_31, Mines_37 |  |
| 264 | Mines_23 | cooked-pack | 24 | 121 | 236 | 75 | 8 | Zombie Beam Miner x5, Crystal Flyer x3 | breakable 13, persistent 30 | 4 | Mines_20, Mines_24, Mines_25, Mines_34 |  |
| 265 | Mines_24 | cooked-pack | 6 | 46 | 68 | 25 | 3 | Mines Crawler x2, Zombie Beam Miner x1 | persistent 2, grub 1 | 1 | Mines_23 |  |
| 266 | Mines_25 | cooked-pack | 36 | 124 | 237 | 74 | 13 | Mines Crawler x6, Zombie Beam Miner x4, Crystal Flyer x3 | breakable 6, breakable_wall 3, reveal_mask 4, persistent 12, geo_rock 5, soul_totem 1 | 2 | Mines_23, Mines_34 |  |
| 267 | Mines_28 | cooked-pack | 25 | 74 | 102 | 40 | 0 |  | breakable 4, persistent 1, door 1, soul_totem 1 | 3 | Mines_07, Mines_35, RestingGrounds_02 |  |
| 268 | Mines_29 | cooked-pack | 15 | 85 | 170 | 51 | 0 |  | breakable 15, persistent 1, bench 1, shiny 1 | 3 | Mines_02, Mines_04 |  |
| 269 | Mines_30 | cooked-pack | 16 | 70 | 102 | 39 | 1 | Crystal Crawler x1 | breakable 12, persistent 2, shiny 1, conversation 2 | 2 | Mines_05, Mines_10 |  |
| 270 | Mines_31 | cooked-pack | 55 | 158 | 237 | 91 | 0 |  | breakable 20, reveal_mask 1, persistent 5, geo_rock 1, soul_totem 2, grub 1 | 1 | Mines_20 |  |
| 271 | Mines_32 | cooked-pack | 9 | 57 | 103 | 33 | 1 | Zombie Beam Miner Rematch x1 | breakable 7, persistent 3 | 1 | Mines_18 |  |
| 272 | Mines_33 | cooked-pack | 10 | 84 | 171 | 49 | 2 | Crystal Crawler x2 | breakable 16, breakable_wall 1, reveal_mask 1, persistent 4, geo_rock 3, toll_gate 2 | 2 | Crossroads_03, Mines_02 |  |
| 273 | Mines_34 | cooked-pack | 63 | 107 | 169 | 54 | 6 | Mines Crawler x4, Crystal Crawler x1, Crystal Flyer x1 | breakable 3, reveal_mask 1, persistent 3, shiny 1 | 4 | Mines_13, Mines_23, Mines_25 |  |
| 274 | Mines_35 | cooked-pack | 30 | 123 | 170 | 74 | 5 | Crystal Flyer x3, Mines Crawler x1, Roller x1 | breakable 22, breakable_wall 1, reveal_mask 1, persistent 8, soul_totem 1, grub 1 | 1 | Mines_28 |  |
| 275 | Mines_36 | cooked-pack | 6 | 41 | 35 | 22 | 0 |  | breakable 3, persistent 1, shiny 1 | 1 | Mines_06 |  |
| 276 | Mines_37 | cooked-pack | 20 | 120 | 238 | 73 | 8 | Zombie Miner x5, Crystallised Lazer Bug x2, Mines Crawler x1 | breakable 10, persistent 11, geo_rock 2, switch 3, toll_gate 2, chest 1 | 2 | Mines_04, Mines_20 |  |

## Deepnest

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 277 | Deepnest_01 | cooked-pack | 9 | 75 | 137 | 47 | 0 |  | breakable 5, breakable_wall 2, reveal_mask 2, persistent 4, geo_rock 2 | 4 | Deepnest_01b, Fungus2_20, Fungus3_39 |  |
| 278 | Deepnest_01b | cooked-pack | 29 | 147 | 305 | 93 | 11 | Baby Centipede x11 | breakable 19, breakable_wall 1, reveal_mask 1, persistent 4, conversation 1 | 5 | Deepnest_01, Deepnest_02, Deepnest_17 |  |
| 279 | Deepnest_02 | cooked-pack | 12 | 108 | 206 | 70 | 7 | Baby Centipede x5, Spider Mini x2 | breakable 28, breakable_wall 2, reveal_mask 2, persistent 3, geo_rock 2 | 3 | Deepnest_01b, Deepnest_36 |  |
| 280 | Deepnest_03 | cooked-pack | 24 | 177 | 306 | 115 | 14 | Spider Mini x7, Zombie Hornhead Sp x4, Zombie Runner Sp x3 | breakable 42, breakable_wall 2, reveal_mask 2, persistent 11, geo_rock 3, grub 1 | 4 | Deepnest_30, Deepnest_31, Deepnest_33, Deepnest_34 |  |
| 281 | Deepnest_09 | cooked-pack | 6 | 53 | 103 | 31 | 0 |  | breakable 4, door 1, stag 1, conversation 1 | 2 | Deepnest_10 |  |
| 282 | Deepnest_10 | cooked-pack | 40 | 99 | 171 | 54 | 0 |  | breakable 14, persistent 1, door 2, soul_totem 1 | 5 | Deepnest_09, Deepnest_41, Room_spider_small |  |
| 283 | Deepnest_14 | cooked-pack | 20 | 122 | 206 | 75 | 0 |  | breakable 22, breakable_wall 1, persistent 3, bench 1, conversation 1 | 7 | Deepnest_17, Deepnest_26, Deepnest_33 |  |
| 284 | Deepnest_16 | cooked-pack | 30 | 197 | 272 | 124 | 13 | Spider Mini x7, Baby Centipede x6 | breakable 49, breakable_wall 2, reveal_mask 4, persistent 8, geo_rock 5, shiny 1 | 2 | Deepnest_17, Fungus2_25 |  |
| 285 | Deepnest_17 | cooked-pack | 10 | 102 | 172 | 64 | 3 | Baby Centipede x3 | breakable 21 | 4 | Deepnest_01b, Deepnest_14, Deepnest_16, Deepnest_30 |  |
| 286 | Deepnest_26 | cooked-pack | 44 | 134 | 205 | 76 | 8 | Baby Centipede Spawner x6, Centipede Hatcher x2 | breakable 14, reveal_mask 3, persistent 8, switch 1, toll_gate 1 | 4 | Deepnest_14, Deepnest_26b, Deepnest_35 |  |
| 287 | Deepnest_26b | cooked-pack | 24 | 90 | 170 | 53 | 10 | Baby Centipede Spawner x6, Centipede Hatcher x4 | breakable 7, reveal_mask 2, persistent 8, switch 1, toll_gate 1, shiny 1 | 2 | Deepnest_26 |  |
| 288 | Deepnest_30 | cooked-pack | 66 | 213 | 274 | 127 | 3 | Baby Centipede x3 | breakable 33, breakable_wall 5, reveal_mask 3, persistent 9, bench 1, conversation 1 | 3 | Deepnest_03, Deepnest_17, Deepnest_37 |  |
| 289 | Deepnest_31 | cooked-pack | 30 | 152 | 204 | 92 | 0 |  | breakable 21, breakable_wall 3, reveal_mask 6, persistent 10, geo_rock 3, grub 1 | 2 | Deepnest_03, Deepnest_32 |  |
| 290 | Deepnest_32 | cooked-pack | 16 | 88 | 136 | 53 | 1 | Mimic Spider x1 | breakable 14, persistent 2, shiny 1 | 1 | Deepnest_31 |  |
| 291 | Deepnest_33 | cooked-pack | 18 | 132 | 205 | 83 | 19 | Zombie Spider x10, Spider Mini x5, Zombie Hornhead Sp x3 | breakable 37, breakable_wall 1, reveal_mask 1, persistent 8, shiny 1, conversation 1, battle 1 | 3 | Deepnest_03, Deepnest_14 |  |
| 292 | Deepnest_34 | cooked-pack | 28 | 118 | 204 | 72 | 5 | Zombie Runner Sp x2, Slash Spider x1, Spider Mini x1 | breakable 49, reveal_mask 1, persistent 5 | 3 | Deepnest_03, Deepnest_35, Deepnest_39 |  |
| 293 | Deepnest_35 | cooked-pack | 24 | 122 | 237 | 77 | 9 | Spider Mini x6, Zombie Hornhead Sp x2, Zombie Runner Sp x1 | breakable 41, persistent 3, geo_rock 2 | 3 | Deepnest_26, Deepnest_34, Deepnest_40 |  |
| 294 | Deepnest_36 | cooked-pack | 12 | 70 | 103 | 42 | 3 | Grub Mimic x3 | breakable 11, persistent 7, grub 4 | 1 | Deepnest_02 |  |
| 295 | Deepnest_37 | cooked-pack | 12 | 107 | 137 | 65 | 0 |  | breakable 24, geo_rock 2 | 4 | Abyss_03_b, Deepnest_30, Deepnest_38, Deepnest_44 |  |
| 296 | Deepnest_38 | cooked-pack | 36 | 164 | 205 | 99 | 1 | Baby Centipede x1 | breakable 16, breakable_wall 2, reveal_mask 2, persistent 5, soul_totem 1 | 1 | Deepnest_37 |  |
| 297 | Deepnest_39 | cooked-pack | 45 | 221 | 304 | 135 | 22 | Spider Flyer x11, Tiny Spider x6, Slash Spider x4 | breakable 58, breakable_wall 11, reveal_mask 3, persistent 66, geo_rock 3, door 1, grub 1 | 4 | Deepnest_34, Deepnest_41, Deepnest_42, Deepnest_45_v02 |  |
| 298 | Deepnest_40 | cooked-pack | 35 | 121 | 170 | 71 | 1 | Ghost Warrior Galien x1 | breakable 21, persistent 1, conversation 2, inspect 1 | 1 | Deepnest_35 |  |
| 299 | Deepnest_41 | cooked-pack | 48 | 167 | 238 | 99 | 12 | Spider Flyer x6, Spider Mini x3, Tiny Spider x2 | breakable 32, breakable_wall 8, reveal_mask 4, persistent 12, toll_gate 1, conversation 1 | 3 | Deepnest_10, Deepnest_39 |  |
| 300 | Deepnest_42 | cooked-pack | 20 | 133 | 205 | 84 | 14 | Spider Flyer x8, Spider Mini x4, Tiny Spider x2 | breakable 33, reveal_mask 1, persistent 5, soul_totem 1 | 3 | Deepnest_39, Deepnest_43, Room_Mask_Maker |  |
| 301 | Deepnest_43 | cooked-pack | 18 | 116 | 171 | 73 | 6 | Garden Zombie x2, Moss Flyer x2, Mantis Heavy Flyer x1 | breakable 2, reveal_mask 1, persistent 4, geo_rock 2 | 3 | Deepnest_42, Fungus3_08, Fungus3_50 |  |
| 302 | Deepnest_44 | cooked-pack | 35 | 145 | 138 | 87 | 0 |  | breakable 5, breakable_wall 1, reveal_mask 3, persistent 6, shiny 1, conversation 1 | 1 | Deepnest_37 |  |
| 303 | Deepnest_45_v02 | cooked-pack | 35 | 131 | 202 | 77 | 5 | Spider Mini x5 | breakable 23, breakable_wall 3, reveal_mask 7, persistent 10, chest 1, shiny 2 | 1 | Deepnest_39 |  |
| 304 | Deepnest_Spider_Town | cooked-pack | 66 | 270 | 237 | 166 | 23 | Tiny Spider x9, Slash Spider x5, Spider Mini x5 | breakable 95, breakable_wall 11, reveal_mask 10, persistent 34, geo_rock 8, bench 1, shiny 1, soul_totem 1, grub 1, conversation 6, inspect 1 | 2 | Deepnest_10 |  |
| 305 | Room_spider_small | cooked-pack | 9 | 43 | 36 | 22 | 0 |  | breakable 3, conversation 1 | 1 | Deepnest_10 |  |

## Kingdom's Edge

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 306 | Deepnest_East_01 | cooked-pack | 18 | 87 | 137 | 52 | 11 | Blow Fly x6, Bee Hatchling Ambient x5 | breakable 1, breakable_wall 1, reveal_mask 1, persistent 9, geo_rock 2, soul_totem 1, conversation 1 | 3 | Abyss_03_c, Deepnest_East_02, Hive_03_c |  |
| 307 | Deepnest_East_02 | cooked-pack | 15 | 109 | 169 | 66 | 12 | Bee Hatchling Ambient x12 | breakable_wall 2, persistent 3, geo_rock 2, soul_totem 1 | 4 | Deepnest_East_01, Deepnest_East_03, Hive_03, Waterways_14 |  |
| 308 | Deepnest_East_03 | cooked-pack | 40 | 110 | 170 | 60 | 6 | Blow Fly x6 | breakable 8, reveal_mask 1, persistent 13, conversation 2 | 6 | Deepnest_East_02, Deepnest_East_04, Deepnest_East_06, Deepnest_East_07, Deepnest_east_07, Ruins2_07 |  |
| 309 | Deepnest_East_04 | cooked-pack | 39 | 167 | 170 | 100 | 8 | Super Spitter x5, Blow Fly x3 | breakable 34, breakable_wall 1, reveal_mask 7, persistent 12, geo_rock 1, conversation 1 | 4 | Deepnest_East_03, Deepnest_East_07, Deepnest_East_11, Deepnest_East_15 |  |
| 310 | Deepnest_East_06 | cooked-pack | 60 | 200 | 205 | 114 | 18 | Hopper x11, Giant Hopper x4, Super Spitter x3 | breakable 21, breakable_wall 1, reveal_mask 1, persistent 9, geo_rock 2, bench 1, door 1 | 5 | Deepnest_East_03, Deepnest_East_14b, Deepnest_East_16, Deepnest_East_18, Room_nailmaster_03 |  |
| 311 | Deepnest_East_07 | cooked-pack | 52 | 189 | 237 | 111 | 26 | Hopper x11, Ceiling Dropper x7, Super Spitter x5 | breakable 15, reveal_mask 3, persistent 73, geo_rock 2, shiny 2, soul_totem 1 | 5 | Deepnest_East_03, Deepnest_East_04, Deepnest_East_08, Ruins2_11_b |  |
| 312 | Deepnest_East_08 | cooked-pack | 24 | 104 | 136 | 61 | 14 | Hopper x9, Ceiling Dropper x4, Giant Hopper x1 | breakable 13, breakable_wall 1, reveal_mask 3, persistent 7, geo_rock 1, shiny 1 | 2 | Deepnest_East_07, Deepnest_East_09 |  |
| 313 | Deepnest_East_09 | cooked-pack | 36 | 94 | 136 | 50 | 7 | Ceiling Dropper x7 | breakable_wall 1, reveal_mask 1, persistent 9 | 3 | Deepnest_East_08, Room_Colosseum_01, Ruins2_10b |  |
| 314 | Deepnest_East_10 | cooked-pack | 9 | 51 | 69 | 29 | 1 | Ghost Warrior Markoth x1 | conversation 1, inspect 1 | 1 | Deepnest_East_18 |  |
| 315 | Deepnest_East_11 | cooked-pack | 45 | 147 | 203 | 84 | 8 | Hopper x4, Super Spitter x4 | breakable 20, breakable_wall 2, reveal_mask 10, persistent 9, soul_totem 1, grub 1 | 4 | Deepnest_East_04, Deepnest_East_12, Deepnest_East_13, Deepnest_East_18 |  |
| 316 | Deepnest_East_12 | cooked-pack | 18 | 73 | 135 | 41 | 0 |  | breakable 12, persistent 1 | 3 | Deepnest_East_11, Deepnest_East_Hornet |  |
| 317 | Deepnest_East_13 | cooked-pack | 9 | 43 | 69 | 24 | 0 |  | persistent 1, bench 1, shiny 1 | 1 | Deepnest_East_11 |  |
| 318 | Deepnest_East_14 | cooked-pack | 35 | 141 | 203 | 84 | 12 | Hopper x8, Super Spitter x2, Egg Sac x1 | breakable 21, breakable_wall 3, reveal_mask 2, persistent 12, door 1, soul_totem 2, grub 1 | 3 | Deepnest_East_14b, Deepnest_East_16, Deepnest_East_17 |  |
| 319 | Deepnest_East_14b | cooked-pack | 20 | 86 | 170 | 50 | 11 | Bee Hatchling Ambient x5, Super Spitter x4, Giant Hopper x1 | breakable 7, reveal_mask 4, persistent 11, shiny 1 | 2 | Deepnest_East_06, Deepnest_East_14 |  |
| 320 | Deepnest_East_15 | cooked-pack | 6 | 46 | 68 | 26 | 0 |  | breakable 3, persistent 1 | 1 | Deepnest_East_04 |  |
| 321 | Deepnest_East_16 | cooked-pack | 9 | 54 | 102 | 31 | 3 | Giant Hopper x2, Ghost Warrior Markoth x1 | breakable 2, breakable_wall 1, reveal_mask 2, persistent 4, soul_totem 1 | 2 | Deepnest_East_06, Deepnest_East_14 |  |
| 322 | Deepnest_East_17 | cooked-pack | 42 | 163 | 204 | 97 | 3 | Hopper x3 | breakable 10, breakable_wall 11, reveal_mask 3, persistent 15, geo_rock 2, soul_totem 1 | 1 | Deepnest_East_14 |  |
| 323 | Deepnest_East_18 | cooked-pack | 20 | 89 | 136 | 52 | 9 | Hopper x9 | breakable 11, breakable_wall 2, reveal_mask 1, persistent 4, shiny 1 | 3 | Deepnest_East_06, Deepnest_East_10, Deepnest_East_11 |  |
| 324 | Deepnest_East_Hornet | cooked-pack | 21 | 93 | 170 | 55 | 0 |  | breakable 1 | 2 | Deepnest_East_12, Room_Wyrm |  |
| 325 | Deepnest_East_Hornet_boss | no-envelope |  |  |  |  | 1 | Hornet Boss x1 | conversation 1 | 0 |  |  |

## Colosseum of Fools

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 30 | Room_Colosseum_01 | cooked-pack | 9 | 46 | 69 | 25 | 0 |  | conversation 4 | 3 | Deepnest_East_09, Room_Colosseum_02 |  |
| 31 | Room_Colosseum_02 | cooked-pack | 40 | 89 | 168 | 46 | 0 |  | breakable_wall 1, reveal_mask 4, persistent 3, bench 1, conversation 2 | 2 | Room_Colosseum_01, Room_Colosseum_Spectate |  |
| 32 | Room_Colosseum_Bronze | cooked-pack | 21 | 71 | 103 | 39 | 1 | Giant Buzzer Col x1 | reveal_mask 1, persistent 2, shiny 1, battle 1 | 2 | Room_Colosseum_01 |  |
| 33 | Room_Colosseum_Silver | cooked-pack | 21 | 71 | 103 | 39 | 3 | Mega Fat Bee x2, Colosseum_Miner x1 | reveal_mask 1, persistent 2, shiny 1, battle 1 | 2 | Room_Colosseum_01 |  |
| 34 | Room_Colosseum_Gold | cooked-pack | 21 | 78 | 103 | 43 | 26 | Mage Balloon Spawner x9, Mage x6, Electric Mage New x4 | reveal_mask 1, persistent 4, shiny 1, battle 1 | 2 | Room_Colosseum_01 | text |
| 35 | Room_Colosseum_Spectate | cooked-pack | 24 | 68 | 102 | 37 | 1 | Colosseum_Miner x1 | breakable_wall 1, reveal_mask 1, persistent 2 | 2 | GG_Lurker, Room_Colosseum_02 |  |

## Ancient Basin and Abyss

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 327 | Abyss_01 | cooked-pack | 24 | 116 | 172 | 68 | 2 | Ruins Flying Sentry x2 | breakable 1, reveal_mask 1, persistent 41, geo_rock 3 | 5 | Abyss_02, Ruins2_04, Waterways_05, Waterways_06, Waterways_07 |  |
| 328 | Abyss_02 | cooked-pack | 32 | 148 | 204 | 90 | 8 | Ruins Flying Sentry x2, Ruins Flying Sentry Javelin x2, Zombie Barger x2 | breakable 5, persistent 10, geo_rock 3, shiny 1 | 2 | Abyss_01, Abyss_03 |  |
| 329 | Abyss_03 | cooked-pack | 18 | 71 | 102 | 40 | 0 |  | breakable 5, door 2, tram 1, conversation 1 | 5 | Abyss_02, Abyss_04, Abyss_17 |  |
| 330 | Abyss_03_b | cooked-pack | 8 | 61 | 103 | 36 | 0 |  | breakable 1, door 2, tram 1, conversation 1 | 3 | Deepnest_37 |  |
| 331 | Abyss_03_c | cooked-pack | 21 | 94 | 169 | 56 | 3 | Bee Hatchling Ambient x3 | breakable 1, breakable_wall 1, reveal_mask 2, persistent 1, door 2, tram 1, conversation 1 | 4 | Deepnest_East_01, Hive_01 |  |
| 332 | Abyss_04 | cooked-pack | 49 | 206 | 337 | 121 | 9 | Abyss Crawler x9 | breakable 15, reveal_mask 3, persistent 6, soul_totem 1, conversation 3, inspect 1 | 4 | Abyss_03, Abyss_05, Abyss_06_Core, Abyss_18 |  |
| 333 | Abyss_05 | cooked-pack | 54 | 113 | 170 | 56 | 0 |  | breakable_wall 1, reveal_mask 7, persistent 1, inspect 1 | 4 | Abyss_04, Abyss_22 |  |
| 334 | Abyss_06_Core | cooked-pack | 90 | 269 | 477 | 164 | 21 | Abyss Crawler x21 | breakable 21, reveal_mask 2, persistent 2, geo_rock 3, conversation 1, inspect 2 | 7 | Abyss_04, Abyss_08, Abyss_12, Abyss_15, Abyss_16 |  |
| 335 | Abyss_08 | cooked-pack | 47 | 212 | 308 | 133 | 0 |  | breakable 11, breakable_wall 1, reveal_mask 1, persistent 4, shiny 2 | 1 | Abyss_06_Core |  |
| 336 | Abyss_09 | cooked-pack | 88 | 167 | 236 | 87 | 22 | Shade Sibling x22 | breakable 14, reveal_mask 1, persistent 1, conversation 1 | 4 | Abyss_10, Abyss_16, Abyss_Lighthouse_room |  |
| 337 | Abyss_10 | cooked-pack | 24 | 113 | 171 | 68 | 0 |  | breakable 20, reveal_mask 1, persistent 2, shiny 1 | 2 | Abyss_09 |  |
| 338 | Abyss_12 | cooked-pack | 24 | 88 | 136 | 50 | 5 | Abyss Crawler x5 | breakable 5 | 1 | Abyss_06_Core | text |
| 339 | Abyss_15 | cooked-pack | 56 | 203 | 337 | 129 | 16 | Shade Sibling x16 | persistent 1, door 1, shiny 1 | 2 | Abyss_06_Core |  |
| 340 | Abyss_16 | cooked-pack | 15 | 76 | 136 | 45 | 0 |  | breakable 8 | 2 | Abyss_06_Core, Abyss_09 |  |
| 341 | Abyss_17 | cooked-pack | 40 | 159 | 203 | 96 | 15 | Lesser Mawlek x8, Ceiling Dropper x4, Spitter x3 | breakable 46, breakable_wall 2, reveal_mask 2, persistent 19, shiny 1, grub 1, conversation 1, battle 1 | 1 | Abyss_03 |  |
| 342 | Abyss_18 | cooked-pack | 27 | 98 | 136 | 56 | 0 |  | breakable 3, persistent 1, geo_rock 1, bench 1 | 2 | Abyss_04, Abyss_19 |  |
| 343 | Abyss_19 | cooked-pack | 49 | 181 | 337 | 111 | 8 | Parasite Balloon x4, Abyss Crawler x3, Infected Knight x1 | breakable 27, breakable_wall 1, reveal_mask 1, persistent 5, geo_rock 2, door 1, shiny 1, grub 1, conversation 2 | 5 | Abyss_18, Abyss_20, Abyss_21 |  |
| 344 | Abyss_20 | cooked-pack | 40 | 158 | 237 | 96 | 12 | Abyss Crawler x4, Mawlek Turret x4, Mawlek Turret Ceiling x2 | breakable 23, reveal_mask 4, persistent 11, shiny 1 | 2 | Abyss_19 |  |
| 345 | Abyss_21 | over-128-views | 136 |  |  |  | 0 |  | breakable 9, conversation 1 | 1 | Abyss_19 |  |
| 346 | Abyss_22 | cooked-pack | 12 | 59 | 103 | 32 | 0 |  | breakable 1, bench 1, door 1, stag 1, conversation 1 | 2 | Abyss_05 |  |
| 347 | Abyss_Lighthouse_room | cooked-pack | 9 | 38 | 35 | 19 | 0 |  | breakable 2, persistent 3, switch 1 | 1 | Abyss_09 |  |

## Royal Waterways

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 349 | Waterways_01 | cooked-pack | 28 | 120 | 237 | 70 | 14 | Ceiling Dropper x5, Inflater x5, Flip Hopper x4 | breakable 6, breakable_wall 1, reveal_mask 2, persistent 7, geo_rock 2, soul_totem 1 | 4 | Ruins1_05b, Waterways_02, Waterways_03, Waterways_04 |  |
| 350 | Waterways_02 | cooked-pack | 44 | 161 | 271 | 95 | 11 | Flukeman x4, Flip Hopper x3, Fluke Fly x2 | breakable 11, breakable_wall 3, reveal_mask 5, persistent 14, bench 1 | 5 | Waterways_01, Waterways_04, Waterways_05, Waterways_06, Waterways_08 |  |
| 351 | Waterways_03 | cooked-pack | 10 | 55 | 70 | 31 | 0 |  | breakable 4, persistent 1, shiny 1, conversation 1 | 1 | Waterways_01 |  |
| 352 | Waterways_04 | cooked-pack | 28 | 117 | 203 | 67 | 13 | Inflater x9, Flukeman x2, Ceiling Dropper x1 | breakable 16, breakable_wall 3, reveal_mask 6, persistent 9, grub 1 | 4 | Waterways_01, Waterways_02, Waterways_04b |  |
| 353 | Waterways_04b | cooked-pack | 24 | 107 | 170 | 62 | 15 | Inflater x7, Fluke Fly x4, Ceiling Dropper x2 | breakable 1, reveal_mask 2, persistent 9, geo_rock 1, shiny 1, soul_totem 1 | 3 | Waterways_04, Waterways_09 |  |
| 354 | Waterways_05 | cooked-pack | 24 | 91 | 137 | 52 | 0 |  | breakable_wall 2, persistent 5, switch 1, shiny 2, conversation 1 | 3 | Abyss_01, Waterways_02, Waterways_15 |  |
| 355 | Waterways_05_boss | no-envelope |  |  |  |  | 1 | Dung Defender x1 |  | 0 |  |  |
| 356 | Waterways_06 | cooked-pack | 24 | 100 | 170 | 57 | 6 | Flip Hopper x3, Fluke Fly x3 | breakable 5 | 2 | Abyss_01, Waterways_02 |  |
| 357 | Waterways_07 | cooked-pack | 35 | 164 | 305 | 102 | 11 | Inflater x4, Flip Hopper x2, Ruins Sentry x2 | breakable 12, breakable_wall 1, reveal_mask 1, persistent 10, geo_rock 1, door 1, soul_totem 1, inspect 1 | 5 | Abyss_01, Ruins_House_03, Waterways_13, Waterways_14 |  |
| 358 | Waterways_08 | cooked-pack | 31 | 185 | 305 | 119 | 104 | fluke_baby x85, Flukeman x9, Ceiling Dropper x5 | breakable_wall 2, reveal_mask 4, persistent 19, geo_rock 1, soul_totem 1 | 3 | GG_Pipeway, Waterways_02, Waterways_12 |  |
| 359 | Waterways_09 | cooked-pack | 12 | 73 | 136 | 42 | 8 | Inflater x8 | breakable 16, breakable_wall 1, persistent 4, switch 1, gate 1, conversation 2, battle 1 | 2 | Fungus2_23, Waterways_04b |  |
| 360 | Waterways_12 | cooked-pack | 12 | 60 | 69 | 34 | 1 | Fluke Fly Spawner x1 | persistent 1, shiny 1 | 1 | Waterways_08 | text |
| 361 | Waterways_12_boss | no-envelope |  |  |  |  | 7 | Fluke Fly Spawner x6, Fluke Mother x1 |  | 0 |  |  |
| 362 | Waterways_13 | cooked-pack | 42 | 163 | 268 | 100 | 14 | Ruins Flying Sentry x5, Ceiling Dropper x2, Inflater x2 | persistent 7, grub 1, battle 1 | 2 | Waterways_07 |  |
| 363 | Waterways_14 | cooked-pack | 44 | 127 | 203 | 70 | 3 | Inflater x3 | breakable 8, breakable_wall 1, reveal_mask 1, persistent 3, grub 1 | 2 | Deepnest_East_02, Waterways_07 |  |
| 364 | Waterways_15 | cooked-pack | 9 | 46 | 68 | 26 | 0 |  | reveal_mask 4, persistent 5, shiny 1, conversation 1 | 2 | Waterways_05 |  |

## The Hive

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 384 | Hive_01 | cooked-pack | 30 | 110 | 171 | 63 | 15 | Hiveling Spawner x10, Bee Hatchling Ambient x3, Bee Stinger x1 | breakable 45, persistent 5, geo_rock 1, bench 1 | 3 | Abyss_03_c, Hive_02 |  |
| 385 | Hive_02 | cooked-pack | 54 | 141 | 172 | 76 | 39 | Hiveling Spawner x20, Bee Hatchling Ambient x13, Bee Stinger x3 | breakable 61, persistent 28, geo_rock 3 | 3 | Hive_01, Hive_03_c |  |
| 386 | Hive_03 | cooked-pack | 60 | 107 | 137 | 53 | 14 | Bee Hatchling Ambient x11, Bee Stinger x2, Big Bee x1 | breakable 25, reveal_mask 1, persistent 6, geo_rock 3, grub 1 | 3 | Deepnest_East_02, Hive_03_c, Hive_04 |  |
| 387 | Hive_03_c | cooked-pack | 48 | 118 | 171 | 64 | 14 | Bee Hatchling Ambient x9, Bee Stinger x3, Big Bee x2 | breakable 31, breakable_wall 1, reveal_mask 1, persistent 8 | 4 | Deepnest_East_01, Hive_02, Hive_03, Hive_04 |  |
| 388 | Hive_04 | cooked-pack | 100 | 159 | 205 | 77 | 32 | Hiveling Spawner x20, Bee Hatchling Ambient x4, Bee Stinger x4 | breakable 58, persistent 14, geo_rock 2, grub 1 | 3 | Hive_03, Hive_03_c, Hive_05 |  |
| 389 | Hive_05 | cooked-pack | 30 | 87 | 137 | 47 | 1 | Hive Knight x1 | breakable 23, persistent 5, shiny 1, conversation 1 | 1 | Hive_04 |  |

## White Palace

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 365 | White_Palace_01 | cooked-pack | 106 | 254 | 237 | 125 | 3 | White Palace Fly x3 | breakable 7, bench 1 | 3 | White_Palace_02, White_Palace_03_hub, White_Palace_11 |  |
| 366 | White_Palace_02 | cooked-pack | 59 | 178 | 202 | 93 | 1 | Royal Gaurd x1 | breakable_wall 1, persistent 4, switch 1, soul_totem 1, battle 1 | 1 | White_Palace_01 |  |
| 367 | White_Palace_03_hub | cooked-pack | 50 | 183 | 203 | 98 | 17 | Enemy x17 | breakable 8, persistent 2, bench 1, switch 1, toll_gate 1, door 1, soul_totem 1 | 7 | White_Palace_01, White_Palace_04, White_Palace_06, White_Palace_14, White_Palace_15 |  |
| 368 | White_Palace_04 | cooked-pack | 54 | 154 | 169 | 80 | 6 | Enemy x6 | persistent 1, soul_totem 1 | 2 | White_Palace_03_hub, White_Palace_14 |  |
| 369 | White_Palace_05 | cooked-pack | 91 | 296 | 237 | 165 | 0 |  |  | 4 | White_Palace_15, White_Palace_16 |  |
| 370 | White_Palace_06 | cooked-pack | 18 | 71 | 102 | 38 | 5 | White Palace Fly x5 | breakable_wall 1, reveal_mask 2, persistent 1, bench 1 | 4 | White_Palace_03_hub, White_Palace_07, White_Palace_18 |  |
| 371 | White_Palace_07 | cooked-pack | 75 | 228 | 237 | 126 | 0 |  | breakable 2 | 2 | White_Palace_06, White_Palace_12 |  |
| 372 | White_Palace_08 | cooked-pack | 28 | 81 | 102 | 43 | 0 |  | inspect 1 | 2 | White_Palace_13 |  |
| 373 | White_Palace_09 | cooked-pack | 71 | 183 | 237 | 94 | 0 |  | breakable 4, breakable_wall 2, reveal_mask 2, persistent 4, shiny 1, soul_totem 1, inspect 1 | 1 | White_Palace_13 |  |
| 374 | White_Palace_11 | cooked-pack | 48 | 95 | 103 | 45 | 1 | Royal Gaurd x1 | persistent 1, door 2 | 3 | White_Palace_01 |  |
| 375 | White_Palace_12 | cooked-pack | 68 | 200 | 203 | 107 | 2 | Enemy x2 | breakable 4, breakable_wall 2, reveal_mask 2, persistent 3, switch 1, toll_gate 2 | 2 | White_Palace_07, White_Palace_13 |  |
| 376 | White_Palace_13 | over-128-views | 135 |  |  |  | 5 | White Palace Fly x5 | breakable 6 | 4 | White_Palace_08, White_Palace_09, White_Palace_12 |  |
| 377 | White_Palace_14 | cooked-pack | 90 | 185 | 203 | 90 | 4 | Enemy x4 | breakable 2, persistent 1, switch 1 | 2 | White_Palace_03_hub, White_Palace_04 |  |
| 378 | White_Palace_15 | cooked-pack | 36 | 138 | 137 | 76 | 4 | Enemy x4 | breakable_wall 1, persistent 4, switch 2, toll_gate 2, soul_totem 1 | 3 | White_Palace_03_hub, White_Palace_05 |  |
| 379 | White_Palace_16 | cooked-pack | 99 | 124 | 103 | 53 | 0 |  |  | 2 | White_Palace_05 |  |
| 380 | White_Palace_17 | cooked-pack | 48 | 173 | 202 | 95 | 0 |  | breakable 1, breakable_wall 1, reveal_mask 1, persistent 3, switch 1, toll_gate 1, soul_totem 2 | 2 | White_Palace_18, White_Palace_19 |  |
| 381 | White_Palace_18 | cooked-pack | 104 | 156 | 136 | 74 | 12 | White Palace Fly x12 | reveal_mask 2, persistent 2, soul_totem 2, inspect 1 | 2 | White_Palace_06, White_Palace_17 |  |
| 382 | White_Palace_19 | cooked-pack | 96 | 143 | 136 | 66 | 6 | White Palace Fly x6 | reveal_mask 2, persistent 3, soul_totem 2 | 2 | White_Palace_17, White_Palace_20 |  |
| 383 | White_Palace_20 | over-128-views | 156 |  |  |  | 2 | Royal Gaurd x2 | persistent 1, soul_totem 1, conversation 1, battle 1 | 1 | White_Palace_19 |  |

## Grimm Troupe

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 390 | Grimm_Divine | cooked-pack | 4 | 36 | 35 | 18 | 0 |  | persistent 3, shiny 3, conversation 1 | 1 | Town |  |
| 391 | Grimm_Main_Tent | cooked-pack | 40 | 84 | 103 | 41 | 0 |  | reveal_mask 2, persistent 4, chest 2, shiny 1, conversation 2 | 2 | Town |  |
| 392 | Grimm_Main_Tent_boss | no-envelope |  |  |  |  | 2 | Grimm Boss x1, Real Bat x1 |  | 0 |  |  |
| 393 | Grimm_Nightmare | cooked-pack | 33 | 76 | 102 | 37 | 2 | Nightmare Grimm Boss x1, Real Bat x1 | persistent 1 | 1 |  | text |

## Dreams

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 394 | Dream_Nailcollection | cooked-pack | 78 | 118 | 102 | 53 | 0 |  | conversation 2 | 4 |  |  |
| 395 | Dream_01_False_Knight | cooked-pack | 35 | 96 | 137 | 51 | 3 | False Knight Dream x1, Head x1, Tinger x1 | persistent 1, door 1, battle 1 | 1 |  |  |
| 396 | Dream_02_Mage_Lord | cooked-pack | 25 | 93 | 138 | 53 | 3 | Dream Mage Lord Phase x2, Dream Mage Lord x1 | door 1 | 1 |  |  |
| 397 | Dream_03_Infected_Knight | cooked-pack | 32 | 78 | 102 | 39 | 1 | Lost Kin x1 | persistent 2, door 1, shiny 1, conversation 2 | 1 |  |  |
| 398 | Dream_04_White_Defender | cooked-pack | 24 | 89 | 136 | 50 | 1 | White Defender x1 |  | 1 |  |  |
| 399 | Dream_Mighty_Zote | cooked-pack | 30 | 60 | 102 | 30 | 9 | Zote Balloon x4, Zoteling x4, Grey Prince x1 | battle 1 | 1 |  | text |
| 400 | Dream_Guardian_Hegemol | cooked-pack | 20 | 45 | 35 | 22 | 0 |  |  | 1 |  |  |
| 401 | Dream_Guardian_Lurien | cooked-pack | 20 | 45 | 35 | 22 | 0 |  |  | 1 |  |  |
| 402 | Dream_Guardian_Monomon | cooked-pack | 20 | 45 | 35 | 22 | 0 |  |  | 1 |  |  |
| 404 | Dream_Backer_Shrine | cooked-pack | 25 | 55 | 68 | 27 | 0 |  | door 2 | 2 | 0. |  |
| 405 | Dream_Room_Believer_Shrine | cooked-pack | 30 | 91 | 69 | 46 | 0 |  | breakable 63, persistent 63, inspect 63 | 1 | Dream_Backer_Shrine |  |
| 406 | Dream_Abyss | cooked-pack | 90 | 252 | 410 | 151 | 0 |  | breakable 19, door 1 | 1 |  |  |
| 407 | Dream_Final_Boss | cooked-pack | 52 | 62 | 35 | 25 | 1 | Radiance x1 | reveal_mask 1, persistent 1 | 1 |  | text |

## Hollow Knight finale

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 408 | Room_Final_Boss_Atrium | cooked-pack | 33 | 69 | 37 | 34 | 0 |  | breakable 4, bench 1 | 2 | Room_Final_Boss_Core, Room_temple | text |
| 409 | Room_Final_Boss_Core | cooked-pack | 12 | 61 | 70 | 34 | 1 | Hollow Knight Boss x1 | breakable 3 | 1 | Room_Final_Boss_atrium | text |

## Godhome

| # | Scene | State | Views | RAM KB | VRAM KB | Disc KB | Enemies | Top families | Interactables | Gates | Exits | Flags |
| ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |
| 423 | GG_Waterways | cooked-pack | 63 | 170 | 336 | 96 | 0 |  | breakable 4, reveal_mask 2, persistent 8, door 1, chest 5, shiny 1, conversation 4 | 3 | GG_Pipeway, Room_GG_Shortcut |  |
| 424 | GG_Atrium | cooked-pack | 120 | 173 | 203 | 80 | 0 |  | reveal_mask 1, persistent 2, bench 1, switch 1, door 3, boss_door 5 | 11 | GG_Atrium_Roof, GG_Blue_Room, GG_Workshop |  |
| 425 | GG_Broken_Vessel | cooked-pack | 49 | 78 | 70 | 37 | 1 | Infected Knight x1 | breakable 2, conversation 1 | 1 |  | boss |
| 426 | GG_Brooding_Mawlek | cooked-pack | 8 | 59 | 102 | 36 | 1 | Mawlek Body x1 |  | 1 |  | boss |
| 427 | GG_Collector | cooked-pack | 40 | 75 | 102 | 37 | 1 | Jar Collector x1 |  | 1 |  | boss |
| 428 | GG_Crystal_Guardian | cooked-pack | 9 | 59 | 103 | 35 | 1 | Mega Zombie Beam Miner x1 | breakable 3 | 1 |  | boss |
| 429 | GG_Crystal_Guardian_2 | cooked-pack | 9 | 53 | 70 | 31 | 1 | Zombie Beam Miner Rematch x1 |  | 1 |  | boss |
| 430 | GG_Dung_Defender | cooked-pack | 24 | 69 | 103 | 37 | 1 | Dung Defender x1 |  | 1 |  | boss |
| 431 | GG_Failed_Champion | cooked-pack | 35 | 74 | 102 | 37 | 3 | False Knight Dream x1, Head x1, Tinger x1 | battle 1 | 1 |  | boss |
| 432 | GG_False_Knight | cooked-pack | 20 | 68 | 102 | 37 | 2 | False Knight New x1, Head x1 | persistent 1, shiny 1, battle 1 | 1 |  | boss |
| 433 | GG_Flukemarm | cooked-pack | 12 | 59 | 70 | 34 | 7 | Fluke Fly Spawner x6, Fluke Mother x1 |  | 1 |  | boss |
| 434 | GG_Ghost_Galien | cooked-pack | 35 | 76 | 103 | 39 | 1 | Ghost Warrior Galien x1 |  | 1 |  | boss |
| 435 | GG_Ghost_Gorb | cooked-pack | 66 | 81 | 69 | 34 | 1 | Ghost Warrior Slug x1 | breakable 1 | 1 |  | boss |
| 436 | GG_Ghost_Hu | cooked-pack | 8 | 58 | 103 | 35 | 1 | Ghost Warrior Hu x1 | breakable 12 | 1 |  | boss |
| 437 | GG_Ghost_Markoth | cooked-pack | 6 | 59 | 103 | 37 | 1 | Ghost Warrior Markoth x1 |  | 1 |  | boss |
| 438 | GG_Ghost_Marmu | cooked-pack | 24 | 77 | 104 | 42 | 1 | Ghost Warrior Marmu x1 |  | 1 |  | boss |
| 439 | GG_Ghost_No_Eyes | cooked-pack | 18 | 71 | 103 | 40 | 1 | Ghost Warrior No Eyes x1 |  | 1 |  | boss |
| 440 | GG_Ghost_Xero | cooked-pack | 12 | 53 | 70 | 30 | 1 | Ghost Warrior Xero x1 | breakable 4 | 1 |  | boss |
| 441 | GG_God_Tamer | cooked-pack | 21 | 67 | 103 | 36 | 2 | Lancer x1, Lobster x1 |  | 1 |  | boss |
| 442 | GG_Grey_Prince_Zote | cooked-pack | 30 | 61 | 70 | 30 | 9 | Zote Balloon x4, Zoteling x4, Grey Prince x1 |  | 1 |  | boss, text |
| 443 | GG_Grimm | cooked-pack | 40 | 71 | 102 | 34 | 2 | Grimm Boss x1, Real Bat x1 |  | 1 |  | boss |
| 444 | GG_Grimm_Nightmare | cooked-pack | 33 | 57 | 69 | 27 | 2 | Nightmare Grimm Boss x1, Real Bat x1 |  | 1 |  | boss, text |
| 445 | GG_Gruz_Mother | cooked-pack | 21 | 64 | 102 | 35 | 1 | Giant Fly x1 | battle 1 | 1 |  | boss |
| 446 | GG_Hive_Knight | cooked-pack | 30 | 71 | 103 | 36 | 1 | Hive Knight x1 | breakable 11 | 1 |  | boss |
| 447 | GG_Hollow_Knight | cooked-pack | 12 | 52 | 36 | 29 | 1 | HK Prime x1 | reveal_mask 2, persistent 2 | 1 |  | boss, text |
| 448 | GG_Hornet_1 | cooked-pack | 20 | 65 | 70 | 36 | 1 | Hornet Boss x1 |  | 1 |  | boss |
| 449 | GG_Hornet_2 | cooked-pack | 21 | 61 | 70 | 32 | 1 | Hornet Boss x1 |  | 1 |  | boss |
| 450 | GG_Lost_Kin | cooked-pack | 32 | 63 | 70 | 31 | 1 | Lost Kin x1 | conversation 1 | 1 |  | boss |
| 451 | GG_Lurker | cooked-pack | 122 | 289 | 238 | 165 | 1 | Pale Lurker x1 | breakable 2, reveal_mask 2, persistent 4, geo_rock 1, shiny 2, soul_totem 1 | 1 | Room_Colosseum_Spectate |  |
| 452 | GG_Mantis_Lords | cooked-pack | 24 | 66 | 102 | 35 | 6 | Mantis Lord S x4, Mantis Lord x2 | battle 1 | 1 |  | boss |
| 453 | GG_Mega_Moss_Charger | cooked-pack | 12 | 78 | 137 | 47 | 1 | Mega Moss Charger x1 |  | 1 |  | boss |
| 454 | GG_Nailmasters | cooked-pack | 24 | 70 | 103 | 38 | 2 | Mato x1, Oro x1 |  | 1 |  | boss |
| 455 | GG_Nosk | cooked-pack | 16 | 60 | 70 | 34 | 1 | Mimic Spider x1 |  | 1 |  | boss |
| 456 | GG_Oblobbles | cooked-pack | 21 | 71 | 103 | 39 | 2 | Mega Fat Bee x2 |  | 1 |  | boss |
| 457 | GG_Painter | cooked-pack | 24 | 72 | 102 | 39 | 1 | Sheo Boss x1 |  | 1 |  | boss |
| 458 | GG_Pipeway | cooked-pack | 28 | 192 | 204 | 124 | 25 | fluke_baby x14, Fluke Fly x5, Fat Fluke x4 | breakable 9, breakable_wall 2, persistent 8, geo_rock 1 | 2 | GG_Waterways, Waterways_08 |  |
| 459 | GG_Radiance | cooked-pack | 52 | 76 | 69 | 33 | 1 | Absolute Radiance x1 |  | 1 |  | boss, text |
| 460 | GG_Sly | cooked-pack | 24 | 60 | 70 | 32 | 1 | Sly Boss x1 | breakable 7 | 1 |  | boss |
| 461 | GG_Soul_Master | cooked-pack | 15 | 59 | 70 | 33 | 2 | Mage Lord x1, Mage Lord Phase x1 | breakable 2 | 1 |  | boss |
| 462 | GG_Soul_Tyrant | cooked-pack | 25 | 67 | 103 | 36 | 3 | Dream Mage Lord Phase x2, Dream Mage Lord x1 | breakable 5 | 1 |  | boss |
| 463 | GG_Spa | cooked-pack | 21 | 71 | 103 | 41 | 0 |  | breakable 1, reveal_mask 1, persistent 2, bench 1 | 2 |  | boss |
| 464 | GG_Traitor_Lord | cooked-pack | 20 | 71 | 103 | 40 | 1 | Mantis Traitor Lord x1 | battle 1 | 1 |  | boss |
| 465 | GG_Unlock | no-envelope |  |  |  |  | 0 |  |  | 0 |  | text |
| 466 | GG_Uumuu | cooked-pack | 65 | 87 | 69 | 34 | 1 | Mega Jellyfish GG x1 |  | 1 |  | boss |
| 467 | GG_Vengefly | cooked-pack | 16 | 53 | 69 | 28 | 1 | Giant Buzzer Col x1 |  | 1 |  | boss |
| 468 | GG_Watcher_Knights | cooked-pack | 36 | 78 | 103 | 40 | 6 | Black Knight x6 | battle 1 | 1 |  | boss |
| 469 | GG_White_Defender | cooked-pack | 24 | 72 | 70 | 37 | 1 | White Defender x1 |  | 1 |  | boss |
| 470 | GG_Workshop | cooked-pack | 90 | 168 | 205 | 86 | 0 |  | breakable_wall 2, reveal_mask 8, persistent 5, bench 1, conversation 3, inspect 37, boss_statue 37 | 38 | GG_Atrium |  |
| 471 | Room_GG_Shortcut | cooked-pack | 48 | 140 | 238 | 82 | 5 | Fungoon Baby x5 | breakable 11, breakable_wall 1, reveal_mask 5, persistent 3, geo_rock 2, conversation 1 | 2 | GG_Waterways, Ruins1_04 |  |
| 472 | GG_End_Sequence | cooked-pack | 6 | 34 | 35 | 17 | 0 |  |  | 0 |  |  |
| 473 | GG_Atrium_Roof | cooked-pack | 119 | 186 | 203 | 82 | 0 |  | breakable_wall 1, reveal_mask 6, persistent 8, bench 1, switch 1, door 1, boss_door 1 | 5 | GG_Atrium, GG_Land_of_Storms |  |
| 474 | GG_Blue_Room | cooked-pack | 21 | 53 | 69 | 27 | 0 |  | breakable 2, reveal_mask 1, persistent 1 | 1 | GG_Atrium |  |
| 475 | GG_Engine | cooked-pack | 21 | 48 | 36 | 23 | 0 |  | reveal_mask 1, persistent 1, conversation 1 | 2 |  | boss |
| 476 | GG_Engine_Prime | cooked-pack | 21 | 49 | 36 | 25 | 0 |  | reveal_mask 1, persistent 1, conversation 1 | 2 |  | boss |
| 477 | GG_Engine_Root | cooked-pack | 21 | 50 | 36 | 24 | 0 |  | reveal_mask 1, persistent 1, conversation 1 | 2 |  | boss |
| 478 | GG_Mage_Knight | cooked-pack | 30 | 63 | 70 | 32 | 1 | Mage Knight x1 | breakable 2 | 1 |  | boss |
| 479 | GG_Vengefly_V | cooked-pack | 16 | 53 | 69 | 28 | 2 | Giant Buzzer Col x2 |  | 1 |  | boss |
| 480 | GG_Entrance_Cutscene | no-envelope |  |  |  |  | 0 |  | conversation 1 | 0 |  |  |
| 481 | GG_Mighty_Zote | cooked-pack | 21 | 60 | 70 | 32 | 19 | Zote Crew Normal x4, Zote Crew Fat x3, Ordeal Zoteling x2 | breakable 4 | 1 |  | boss, text |
| 482 | GG_Land_of_Storms | cooked-pack | 75 | 108 | 103 | 52 | 0 |  | persistent 1, door 1, shiny 1 | 1 | GG_Atrium_Roof |  |
| 483 | GG_Boss_Door_Entrance | no-envelope |  |  |  |  | 0 |  | conversation 1 | 0 |  |  |
| 484 | GG_Gruz_Mother_V | cooked-pack | 21 | 64 | 70 | 34 | 1 | Giant Fly x1 | battle 1 | 1 |  | boss |
| 485 | GG_Brooding_Mawlek_V | cooked-pack | 8 | 59 | 102 | 36 | 1 | Mawlek Body x1 |  | 1 |  | boss |
| 486 | GG_Mantis_Lords_V | cooked-pack | 24 | 66 | 102 | 35 | 8 | Mantis Lord S x6, Mantis Lord x2 | battle 1 | 1 |  | boss |
| 487 | GG_Nosk_Hornet | cooked-pack | 16 | 54 | 69 | 29 | 1 | Hornet Nosk x1 | battle 1 | 1 |  | boss |
| 488 | GG_Uumuu_V | cooked-pack | 65 | 86 | 69 | 34 | 1 | Mega Jellyfish GG x1 |  | 1 |  | boss |
| 489 | GG_Ghost_Gorb_V | cooked-pack | 66 | 80 | 69 | 33 | 1 | Ghost Warrior Slug x1 |  | 1 |  | boss |
| 490 | GG_Ghost_Markoth_V | cooked-pack | 6 | 51 | 69 | 30 | 1 | Ghost Warrior Markoth x1 |  | 1 |  | boss |
| 491 | GG_Ghost_Marmu_V | cooked-pack | 24 | 69 | 70 | 36 | 1 | Ghost Warrior Marmu x1 |  | 1 |  | boss |
| 492 | GG_Ghost_No_Eyes_V | cooked-pack | 18 | 69 | 103 | 39 | 1 | Ghost Warrior No Eyes x1 |  | 1 |  | boss |
| 493 | GG_Ghost_Xero_V | cooked-pack | 12 | 49 | 69 | 27 | 1 | Ghost Warrior Xero x1 |  | 1 |  | boss |
| 494 | GG_Mage_Knight_V | cooked-pack | 30 | 63 | 70 | 32 | 37 | Mage Balloon Spawner x36, Mage Knight x1 | breakable 2, battle 1 | 1 |  | boss |
| 495 | GG_Collector_V | cooked-pack | 40 | 75 | 102 | 37 | 1 | Jar Collector x1 |  | 1 |  | boss |
| 496 | GG_Nosk_V | cooked-pack | 16 | 61 | 70 | 35 | 1 | Mimic Spider x1 |  | 1 |  | boss |
| 497 | GG_Wyrm | cooked-pack | 21 | 70 | 135 | 39 | 0 |  | reveal_mask 1, persistent 1, conversation 1 | 2 |  | boss |
| 498 | GG_Unn | cooked-pack | 21 | 53 | 69 | 27 | 0 |  | reveal_mask 1, persistent 1, conversation 1 | 2 |  | boss |
| 500 | GG_Unlock_Wastes | cooked-pack | 12 | 60 | 136 | 36 | 0 |  | conversation 1, inspect 1 | 1 |  |  |

