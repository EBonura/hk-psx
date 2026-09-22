//! Single-command driver: `cargo run --release -- build` cooks (cached), builds
//! the guest, replaces the sole disc pair, writes the build report and replays
//! the validation routes. The Unity extraction and cookers stay in `host/*.py`;
//! this binary owns the order, the caches and the pass/fail verdict.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Cooker inputs, one repo-relative path per line; shared with host/regions.py.
const COOK_INPUTS: &str = include_str!("../cook_inputs.txt");
const ASSET_SCRIPTS: &[&str] = &[
    "title_music.py", "cook_audio.py", "cook_hud.py", "ambience.py", "focus_audio.py", "read_points.py",
    "geo.py", "geo_audio.py", "runner_audio.py", "lifeblood.py", "break_effects.py", "great_door.py",
    // The arena gates, after both tables that share the guest's scripted-edge
    // scratch: it reads data/lifeblood.rs and data/great_door.rs to check the
    // union in one catalogue slot rather than its own rows alone.
    "battle_gates.py",
    "shade.py", "ability_art.py",
    // After read_points.py: the charm catalogue wraps its descriptions against
    // the glyph advances that cook writes into data/read_points.rs.
    "charms.py",
    // Sly's shop, which the guest links now that Room_shop is admitted. It
    // needs geo.py's report for the economy, read_points.py's advances to wrap
    // its descriptions and items.py's fuse rules, so it follows all three.
    "items.py",
    "shops.py",
    // The compiled FSM programs. Independent of the others: it reads the source
    // scenes and .hkpsx/selected-regions.json and writes data/scripts.rs.
    "cook_scripts.py",
];
/// Route tapes replayed against the final CUE; the ones flagged in ROUTES also run the scene-gate verifier.
/// Final telemetry a route must reach besides completing its tape.
const REQUIRED: &[(&str, &str, u64)] = &[
    // well-drop resumes at the Dirtmouth bench and walks right into the well, so the
    // Town to Crossroads gate is exercised without replaying King's Pass.
    ("well-drop", "HK_SCENE_GATE_LOADS", 1),
    // town-reset2 resumes, walks, then Select resets the session back to the spawn.
    ("town-reset2", "HK_REGION_ID", 1),
    // crossroads-gate carries well-drop on into the Crossroads: after the well it walks
    // west along the shaft floor and takes the Crossroads_01 side gate into Crossroads_07,
    // so a scene gate between two Crossroads scenes is covered again. Two gate loads and
    // the Crossroads_07 entry view are what it is here to prove; the deaths count keeps a
    // respawn from standing in for the traversal.
    ("crossroads-gate", "HK_SCENE_GATE_LOADS", 2), ("crossroads-gate", "HK_REGION_ID", 150),
    ("crossroads-gate", "HK_DEATHS", 0), ("crossroads-gate", "HK_SAVE_LOADED", 1),
    // The traversal also fights, and nothing pinned that: it swings twice, kills one
    // of the two families living in these scenes and takes a mask doing it. Exact on
    // purpose. A Climber left alive at full health is the point of the last one, so a
    // regression that stops the Climber contacting the Knight shows here.
    ("crossroads-gate", "HK_ENEMY_KILLS", 1), ("crossroads-gate", "HK_ENEMY_HITS", 2),
    ("crossroads-gate", "HK_HEALTH", 4), ("crossroads-gate", "HK_CLIMBER_HP", 8),
    // Continue boots from tools/cards/town-continue.mcd (a town-bench run's card): the save
    // screen's first slot resumes in the Town view with that record's wallet and opened Great Door. A catalogue change
    // that moves the Town slot invalidates the fixture: copy .hkpsx/validate/town-bench.mcd over it.
    ("town-continue", "HK_SAVE_LOADED", 1), ("town-continue", "HK_MENU_ROW", 0), ("town-continue", "HK_REGION_ID", 100),
    ("town-continue", "HK_GEO_WALLET", 6), ("town-continue", "HK_GREAT_DOOR_HITS", 13),
    // town-shade boots from a card whose record already carries a Hollow Shade six units
    // from the Dirtmouth bench with a 99-Geo pool. It must spawn, chase and hurt the
    // Knight, die to the nail, and hand the pool back on top of the saved six.
    ("town-shade", "HK_SHADE_KILLS", 1), ("town-shade", "HK_SHADE_PRESENT", 0), ("town-shade", "HK_GEO_WALLET", 105),
    // Two masks, not the three this cost before build 120: ATTACK_QUEUE_STEPS now
    // carries a swing pressed a few ticks early, so the same tape kills the Shade
    // sooner and eats one fewer hit. Exact on purpose, so a later regression shows.
    ("town-shade", "HK_HEALTH", 3),
    // kings-death walks the King's Pass traversal with the nail silent past the fifth
    // breakable, so the Crawler there survives to kill the Knight. Death must record a
    // Shade at the spot with nailDamage * clamp(maxHealth / 2, 1, 99) health, and must
    // not touch the card: this port only writes when the player accepts the bench prompt.
    ("kings-death", "HK_DEATHS", 1), ("kings-death", "HK_ENEMY_KILLS", 0),
    ("kings-death", "HK_SHADE_PRESENT", 1), ("kings-death", "HK_SHADE_HP", 10),
    ("kings-death", "HK_SAVE_WRITES", 0), ("kings-death", "HK_SAVE_ERRORS", 0),
    // kings-return boots from the card kings-death itself wrote, so the Shade it spawns
    // comes from a real death rather than a hand-seeded record. Walking the same
    // traversal back must find it there and recover it without dying again. Regenerate
    // the fixture from .hkpsx/validate/kings-death.mcd whenever the save record changes.
    ("kings-return", "HK_SAVE_LOADED", 1), ("kings-return", "HK_SHADE_KILLS", 1),
    ("kings-return", "HK_SHADE_PRESENT", 0), ("kings-return", "HK_DEATHS", 0),
    // Walking back past the Crawler that did the killing costs three of the five
    // masks, and the Knight kills it this time. Unpinned until now, so the return
    // traversal could have stopped fighting entirely and still passed.
    // Re-baselined at build 141, when tiled frames added a little per-frame cost
    // to everything drawn. kings-return is the most timing-sensitive route this
    // project has, being its longest continuous contact-heavy traversal, and it
    // came back 4.1 units short having taken one hit fewer. What says that is
    // timing rather than behaviour: its kill, its two hits, its Shade recovery,
    // its five breakables and its zero deaths are all unchanged, and nine other
    // routes returned byte-identical positions, including the longest ones.
    ("kings-return", "HK_ENEMY_KILLS", 1), ("kings-return", "HK_ENEMY_HITS", 2),
    // well-drop falls the length of the well and must arrive unhurt: the drop is not
    // a hazard and nothing down there reaches the Knight on the way.
    ("well-drop", "HK_HEALTH", 5), ("well-drop", "HK_DEATHS", 0),
    // bench-save resumes at the Dirtmouth bench, sits, and accepts the prompt. It is the
    // only way this port writes the card, and what the power-cut check interrupts.
    ("bench-save", "HK_BENCH_RESTS", 1), ("bench-save", "HK_SAVE_WRITES", 1), ("bench-save", "HK_SAVE_ERRORS", 0),
    // town-elderbug resumes in Town, walks into Elderbug's npc_control talk trigger and
    // holds a whole conversation: UP opens it, six presses of X page through all six
    // cooked entries and close it. SOURCE back to 0 is what proves it closed rather than
    // ran out of tape; OPENED stays 1 because the trigger the Knight is standing in must
    // not reopen it behind the closing press. metElderbug is written on the way through.
    // A .pxtape is opaque once written, so this is the string that made it, through
    // tools/validate.py's poll_tape at count 900:
    //   9:start:1,12:cross:1,400:left:120,600:right:84,700:up:4,
    //   730:cross:2,755:cross:2,780:cross:2,805:cross:2,830:cross:2,855:cross:2
    ("town-elderbug", "HK_NPC_OPENED", 1), ("town-elderbug", "HK_NPC_SOURCE", 0),
    ("town-elderbug", "HK_MET_ELDERBUG", 1), ("town-elderbug", "HK_REGION_ID", 100),
    ("town-elderbug", "HK_SAVE_LOADED", 1), ("town-elderbug", "HK_DEATHS", 0),
    // The conversation is a chain, so the cursor moving is what says the next
    // visit gets the next conversation rather than the introduction again.
    ("town-elderbug", "HK_NPC_MET", 1),
    // The first cooked PlayMaker FSM to run on the disc. Town's Area Resetter
    // waits in Detect for its Trigger2dEvent volume at x 182.7 to 187.4, y 2.14
    // to 3.14, which is the mouth of the well, takes TOUCH to Reset and writes
    // currentArea. FLAGS bit 1 is that field, so 2 is the write having landed
    // in the persistent store rather than the transition merely having fired.
    // crossroads-gate carries the same walk on, and ends with no instance
    // resident because Crossroads_07 has none.
    ("well-drop", "HK_SCRIPT_TRANSITIONS", 1), ("well-drop", "HK_SCRIPT_WRITES", 1),
    ("well-drop", "HK_SCRIPT_FLAGS", 2), ("well-drop", "HK_SCRIPT_ACTIVE", 1),
    ("crossroads-gate", "HK_SCRIPT_TRANSITIONS", 1), ("crossroads-gate", "HK_SCRIPT_WRITES", 1),
    ("crossroads-gate", "HK_SCRIPT_FLAGS", 2),
    // The bank emits no native call, so one arriving means the bank and the
    // runtime have diverged. Same for a halt: every program is meant to fit its
    // op budget, so a halt is a cooked program the executor could not run.
    ("well-drop", "HK_SCRIPT_HALTS", 0), ("well-drop", "HK_SCRIPT_NATIVE", 0),
    ("kings-death", "HK_SCRIPT_HALTS", 0), ("kings-death", "HK_SCRIPT_NATIVE", 0),
    // Instances resident per scene: one in King's Pass, two in Town. This is
    // what says the bank is being seated at all, on routes where nothing fires.
    // Nothing reaches Tutorial_01's own acting volume (Set Seen Focus Tablet at
    // x 102.0 to 103.7, y 27.98 to 37.9): kings-death stays below y 25 and
    // kings-return passes underneath at y 11.4. That needs the King's Pass
    // climb, which no route does yet.
    ("kings-death", "HK_SCRIPT_ACTIVE", 1),
    ("town-elderbug", "HK_SCRIPT_ACTIVE", 2), ("town-elderbug", "HK_SCRIPT_HALTS", 0),
    // The ability routes. Every one boots fresh and grants its ability from the
    // pause Cheats page, because no admitted scene holds an ability pickup, and
    // every one presses exactly one ability button. HK_ABILITY_DRAWN counts
    // frames an ability clip owned the Knight's body, so paired with the cheat
    // bit it says the ability ran rather than that the Knight moved.
    //
    // Vengeful Spirit: Restore fills SOUL to 99 and exactly FIREBALL_PARAMS.cost
    // leaves the pool. The only other consumer of 33 is Focus, which the zero
    // rules out.
    ("cheat-spell", "HK_CHEATS", 1024), ("cheat-spell", "HK_SOUL", 66),
    ("cheat-spell", "HK_FOCUS_STARTED", 0),
    // Mothwing Cloak: the tape presses no direction at all, so the alternative
    // to a dash is X unchanged. The delta is dash_speed times dash_ticks exactly.
    ("cheat-dash", "HK_CHEATS", 16), ("cheat-dash", "HK_PLAYER_X", 2651581),
    ("cheat-dash", "HK_ABILITY_DRAWN", 5),
    // Monarch Wings against ctrl-wings, the same tape with the grant removed.
    // The Knight lands on the same floor either way, so the double jump shows
    // only in the clip count, which is why this route had no assertion at all
    // until HK_ABILITY_DRAWN was watched.
    ("cheat-wings", "HK_CHEATS", 64),
    ("ctrl-wings", "HK_CHEATS", 0), ("ctrl-wings", "HK_ABILITY_DRAWN", 0),
    // Crystal Heart: 48 polls of charge, then 23.3 units of travel with no
    // direction pressed, which is not an ordinary dash's 5.0.
    ("cheat-heart", "HK_CHEATS", 128), ("cheat-heart", "HK_PLAYER_X", 3851091),
    // Dream Nail: SOUL on this port comes from a nail hit, a Focus refund or
    // DREAM_NAIL_PARAMS.soul. Enemy hits and Focus are both zero, so the 33 is
    // EnemyDreamnailReaction on the Crawler and nothing else. Invincibility is
    // granted so Crawler contact cannot interrupt the take-control sequence.
    ("cheat-dream", "HK_CHEATS", 513), ("cheat-dream", "HK_SOUL", 33),
    ("cheat-dream", "HK_ENEMY_HITS", 0), ("cheat-dream", "HK_ENEMY_KILLS", 0),
    ("cheat-dream", "HK_HEALTH", 5), ("cheat-dream", "HK_BREAK_COUNT", 5),
    // The King's Pass climb, which reproduced in the probe and had never been
    // done on the disc, and the first cooked script to fire in Tutorial_01:
    // FLAGS bit 0 is seenFocusTablet. The climb needs no ability, it needs to
    // survive two Crawler hits, which is what the Invincibility grant buys;
    // ctrl-climb is the same tape without the grant and reaches none of it.
    ("kings-climb", "HK_CHEATS", 1), ("kings-climb", "HK_SCRIPT_TRANSITIONS", 1),
    ("kings-climb", "HK_SCRIPT_WRITES", 1), ("kings-climb", "HK_SCRIPT_FLAGS", 1),
    ("kings-climb", "HK_SCRIPT_HALTS", 0), ("kings-climb", "HK_SCRIPT_NATIVE", 0),
    ("kings-climb", "HK_REGION_ID", 30), ("kings-climb", "HK_DEATHS", 0),
    ("kings-climb", "HK_HEALTH", 5), ("kings-climb", "HK_ENEMY_KILLS", 2),
    ("kings-climb", "HK_ENEMY_HITS", 4), ("kings-climb", "HK_BREAK_COUNT", 11),
    ("kings-climb", "HK_SOUL", 44),
    ("ctrl-climb", "HK_CHEATS", 0), ("ctrl-climb", "HK_SCRIPT_FLAGS", 0),
    ("ctrl-climb", "HK_SCRIPT_WRITES", 0),
    // Sly's shop, through a door that could not fire until today. town-shop
    // resumes in Dirtmouth, walks left into door_sly's trigger, presses UP to
    // take the door, crosses Room_shop to the counter and opens the shelf.
    //
    // The door is the half worth pinning: `world::gate` point-tested the
    // Knight's origin, which sits 1.39 units above his feet, against a collider
    // that is a strip of floor 0.23 units tall, so no door in this game could
    // ever fire. SCENE_GATE_LOADS 1 into region 694 is that rule working.
    //
    // OPENED 1 with OPEN 0 and CLOSED 1 is a shelf that went up and came down,
    // rather than a tape that ran out with it still showing.
    //
    // The Knight buys twice. He boots from town-shop.mcd, which is
    // town-continue seeded with a 1,000 Geo wallet against the 6 that card
    // carries, because Sly's cheapest stock is a 60-Geo Rancid Egg and nothing
    // in the admitted slice earns enough to reach it. Six bytes separate the
    // two cards: the wallet word and the checksum.
    //
    // Neither press moves the cursor. On a fresh board row 0 is the 150-Geo
    // Mask Shard, and buying it slides the 500-Geo second shard onto row 0, so
    // two presses in place spend 650. A third would reach the 550-Geo Vessel
    // Fragment and refuse TooPoor at 350, so an overrun cannot silently spend.
    //
    // SHARDS 2 with MASKS 5 is the pair worth asserting together: shards fuse
    // into a mask at four, so two of them must raise the count and must not
    // raise the mask total.
    //
    // The route string, since a .pxtape is opaque, through poll_tape at 980:
    //   9:start:1,12:cross:1,400:left:83,600:up:4,720:right:34,
    //   800:up:4,812:cross:2,832:cross:2,860:circle:2
    ("town-shop", "HK_SCENE_GATE_LOADS", 1), ("town-shop", "HK_REGION_ID", 694),
    ("town-shop", "HK_SHOP_OPENED", 1), ("town-shop", "HK_SHOP_CLOSED", 1),
    ("town-shop", "HK_SHOP_OPEN", 0), ("town-shop", "HK_SHOP_PURCHASES", 2),
    ("town-shop", "HK_SHOP_GEO_SPENT", 650), ("town-shop", "HK_SHOP_SHARDS", 2),
    ("town-shop", "HK_SHOP_MASKS", 5), ("town-shop", "HK_SHOP_FRAGMENTS", 0),
    ("town-shop", "HK_SAVE_LOADED", 1), ("town-shop", "HK_DEATHS", 0),
    // The False Knight, fought fairly and killed. No cheats, five masks at the
    // end and not one of them lost: HK_CHEATS 0 and HK_HEALTH 5 hold at every
    // poll of the tape, not only at the last one. Three staggers, three
    // conversions, the death event, the arena cleared and its gates reopened.
    //
    // The tape before this one ran with Invincibility on, set by its own
    // pause-menu polls, so the port had proved a boss that could be killed and
    // had never proved a fight that could be won. Strip those seven events from
    // it and the Knight dies without landing a single stagger.
    //
    // STAGGERS equal to CONVERSIONS is the assertion worth having. The source
    // lets a hero stagger the boss out of its own rage, after which the fight
    // never ends because Rage Check only reads the stagger count once the rage
    // counter reaches zero; an earlier tape measured 8 staggers against 1
    // conversion, and the recook that sealed the arena left the cheated tape at
    // 4 against 2. Equal counts are the proof this one held fire through both
    // rages instead.
    //
    // Three things had to be found before any tape could finish this fight. The
    // exposed Head sits at y 30.24 to 32.47 and a Knight standing on the arena
    // floor reaches y 30.18 with an up slash, so it is unreachable from the
    // ground by 0.06 units and the tape jumps and swings at the apex. Every
    // landed hit recoils the Knight 0.69 units away from his facing. And the
    // arena has to seal: with both end gates cooked as terrain that recoil can
    // no longer walk him off the floor, which is what left an earlier tape
    // stuck at exactly 10 hp, and it gives the hero a bounded band from x 19 to
    // x 39 to dodge in. This route did not exist until the gates did.
    //
    // The card is seeded rather than earned, so this proves the fight and not
    // the journey to it. The route string is tools/tapes/boss-fight.route, kept
    // because a .pxtape is opaque; it is 239 events over 4,609 polls, and the
    // only buttons in it are square, left, right, cross and one start.
    //
    // Fragile to anything that moves the boss path, the actor bounds or the
    // nail geometry. Three that silently break a tape and cost real time to
    // find: Actor::local_bounds mirrors the placement box when the walk
    // direction leaves the cooked initial_direction, which flips this boss's
    // collision offset by 0.142 units the moment it turns right;
    // respond_to_hurt cancels the swing and the nail recoil on any landed hit,
    // whatever the source; and the barrel summoner seeds from 1 out of
    // EnemyWorld::new rather than from the scene id, because nothing in this
    // route reloads the scene for clear_shots to re-seed it.
    ("boss-fight", "HK_FK_TRIGGERED", 1), ("boss-fight", "HK_FK_DROPPED", 1),
    ("boss-fight", "HK_FK_STAGGERS", 3), ("boss-fight", "HK_FK_CONVERSIONS", 3),
    ("boss-fight", "HK_FK_DEATHS", 1), ("boss-fight", "HK_FK_STUNNED", 3),
    ("boss-fight", "HK_FK_ARENA", 4), ("boss-fight", "HK_FK_ACTIVATED", 1),
    ("boss-fight", "HK_ARENA_GATE_CLOSES", 1), ("boss-fight", "HK_ARENA_GATE_OPENS", 1),
    ("boss-fight", "HK_HEALTH", 5), ("boss-fight", "HK_DEATHS", 0),
    ("boss-fight", "HK_ENEMY_KILLS", 0), ("boss-fight", "HK_CHEATS", 0),
    ("boss-fight", "HK_SAVE_LOADED", 1), ("boss-fight", "HK_REGION_ID", 240),
    // Every barrel the rage flung reached terrain or the hero. Spawns and
    // breaks are separate counters because a barrel the pool recycled under
    // pressure never breaks, so equal counts are what tells a fight the player
    // dodged from one where the rage's own barrels evicted each other.
    ("boss-fight", "HK_FK_BARRELS", 24), ("boss-fight", "HK_FK_BARRELS_BROKEN", 24),
    // Losing to the False Knight, and walking back in for a second go.
    //
    // boss-fight above is the win. This is the loss, and the two together are
    // what P21 actually asks for. A Knight who presses nothing after walking
    // into the trigger is dead in 1,002 polls, on five separate hits.
    //
    // What the second half proves is the retry. The death leaves a Shade in the
    // arena, respawns at the seat with five masks, resets the arena to Waiting
    // and restores the boss to 65 and 40, and the closed gate goes back to its
    // placement state on the same tick the scene reloads, so nothing is sealed
    // in with nothing to fight. Walking right again drops the boss a second
    // time: TRIGGERED and DROPPED are 2, and GATE_CLOSES is 2.
    //
    // The tape stops at poll 1,600 and that is not where the fight ends. At
    // poll 1,897, about 490 polls into the second fight, the frame collapses:
    // two missed VBlanks per poll, every poll, with the input queue climbing
    // from 5 to 16 in 21 polls until the sampler faults QueueFull and the guest
    // panics. Health is steady at 3 through all of it, so it is not damage. A
    // fight with a Shade in the room is the ordinary case after any death, and
    // it is the one case the port cannot currently survive. Do not extend this
    // tape past 1,600 expecting it to pass; fix that first.
    ("boss-death", "HK_CHEATS", 0), ("boss-death", "HK_DEATHS", 1),
    ("boss-death", "HK_SHADE_PRESENT", 1), ("boss-death", "HK_SHADE_HP", 10),
    ("boss-death", "HK_FK_TRIGGERED", 2), ("boss-death", "HK_FK_DROPPED", 2),
    ("boss-death", "HK_FK_HP", 65), ("boss-death", "HK_FK_HEAD_HP", 40),
    ("boss-death", "HK_FK_ARENA", 2), ("boss-death", "HK_FK_DEATHS", 0),
    ("boss-death", "HK_FK_STAGGERS", 0), ("boss-death", "HK_ARENA_GATE_CLOSES", 2),
    ("boss-death", "HK_SCENE_LOADS", 1), ("boss-death", "HK_ENEMY_KILLS", 0),
    ("boss-death", "HK_SAVE_LOADED", 1), ("boss-death", "HK_SAVE_ERRORS", 0),
    ("boss-death", "HK_REGION_ID", 239),
    // Greenpath, walked rather than cooked. The Knight boots from a seeded card
    // ten units inside Fungus1_01 and holds left, crossing into Fungus1_01b and
    // then into Fungus1_02, which is the room every other Greenpath scene is
    // reached through. Two scene gates, three scene loads, five masks, no
    // faults.
    //
    // The region trail is the assertion worth having beyond the gate count.
    // Inside Fungus1_02 he crosses five distinct regions, and that scene is one
    // of the two in the catalogue carrying a recorded view layout rather than
    // the derived grid, because six of its fifteen grid views want a sixth
    // static page. So this route is what says those hand-recorded boxes hold up
    // on hardware, not just in the cook.
    //
    // The route string, since a .pxtape is opaque, through poll_tape at 3000:
    //   9:start:1,12:cross:1,150:left:260,600:left:400,1100:left:400,
    //   1600:left:400,2100:left:400,2600:left:350
    ("greenpath-walk", "HK_SCENE_GATE_LOADS", 2), ("greenpath-walk", "HK_SCENE_LOADS", 3),
    ("greenpath-walk", "HK_REGION_ID", 742), ("greenpath-walk", "HK_HEALTH", 5),
    ("greenpath-walk", "HK_DEATHS", 0), ("greenpath-walk", "HK_SAVE_LOADED", 1),
    ("greenpath-walk", "HK_ROOM_LOAD_ERROR", 0), ("greenpath-walk", "HK_CD_STREAM_ERROR", 0)];
/// Telemetry a route must reach at least, rather than exactly.
///
/// Reserved for counters that sit on a knife edge. The frame is stall-bound, so
/// per-tick cost changes how many simulation ticks fall between two input polls,
/// and these four flip on any change at all: kings-return's surviving mask count
/// went 2, 3, 4 across three builds while its kill, its two hits, its Shade
/// recovery, its five breakables and its zero deaths never moved, and
/// cheat-spell's drawn-frame count went 22, 23, 22. Pinning those exactly
/// reports every unrelated change as a failure, which is noise rather than
/// signal.
///
/// The perf-regression signal this table gives up is not lost: fourteen routes
/// still hold exact positions, and nine of them stayed byte-identical through
/// the change that moved these.
const REQUIRED_MIN: &[(&str, &str, u64)] = &[
    // Survived the traversal at all. HK_DEATHS 0 above is the real assertion;
    // this only says the Knight was not left on one mask by something new.
    ("kings-return", "HK_HEALTH", 2),
    // An ability clip owned the Knight's body. The count is frame timing; that
    // it is not zero is the ability having run, and each control route with the
    // grant removed is pinned at exactly 0 in REQUIRED.
    ("cheat-spell", "HK_ABILITY_DRAWN", 20),
    ("cheat-heart", "HK_ABILITY_DRAWN", 50),
    ("cheat-dream", "HK_ABILITY_DRAWN", 400),
    ("cheat-wings", "HK_ABILITY_DRAWN", 5),
    // Alive when the tape runs out. How many masks the second fight has cost by
    // then is frame timing; HK_DEATHS 1 above is the assertion that he died
    // exactly once and got back up.
    ("boss-death", "HK_HEALTH", 1),
];

/// Routes that boot with a port-1 memory card: a fixture under tools/cards, or a
/// fresh formatted card when the fixture is absent (the run writes it back there).
const MEMCARDS: &[(&str, &str)] = &[("town-continue", "town-continue.mcd"),
    ("town-shade", "town-shade.mcd"), ("kings-death", "kings-death.mcd"), ("kings-return", "kings-return.mcd"),
    ("bench-save", "town-continue.mcd"), ("well-drop", "town-continue.mcd"), ("town-reset2", "town-continue.mcd"),
    ("crossroads-gate", "town-continue.mcd"), ("town-elderbug", "town-continue.mcd"),
    ("town-shop", "town-shop.mcd"), ("boss-fight", "boss-fight.mcd"),
    ("boss-death", "boss-fight.mcd"), ("greenpath-walk", "greenpath.mcd")];
const ROUTES: &[(&str, bool)] = &[("focus", false), ("well-drop", true), ("town-reset2", true),
    ("crossroads-gate", true), ("town-continue", false), ("town-shade", false), ("kings-death", false),
    ("kings-return", false), ("bench-save", false), ("town-elderbug", false),
    ("cheat-spell", false), ("cheat-dash", false), ("cheat-wings", false), ("ctrl-wings", false),
    ("cheat-heart", false), ("cheat-dream", false), ("kings-climb", false), ("ctrl-climb", false),
    ("town-shop", true), ("boss-fight", false), ("boss-death", false), ("greenpath-walk", true)];

/// Tapes `pgo` profiles when none is given: the King's Pass climb and the
/// Crossroads gate walk. Against the ordinary build at a261a97 this recipe
/// measured 19.77 to 21.79 rendered FPS on the first and 14.82 to 15.71 on the
/// second, leaving 19,808 bytes free below the stack reservation.
const PGO_TAPES: &[&str] = &["kings-climb", "crossroads-gate"];

struct Options {
    hollow_knight: Option<PathBuf>,
    sdk_source: Option<PathBuf>,
    emulator_source: Option<PathBuf>,
    frontend: Option<PathBuf>,
    telemetry: bool,
    recook: bool,
    validate: bool,
    tapes: Vec<PathBuf>,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn read_json(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}

fn run(program: impl AsRef<Path>, args: &[&str], cwd: &Path) -> Result<()> {
    let program = program.as_ref();
    println!("+ {} {}", program.display(), args.join(" "));
    let status = Command::new(program).args(args).current_dir(cwd).status()?;
    if !status.success() {
        return Err(format!("{} failed with {status}", program.display()).into());
    }
    Ok(())
}

fn venv_python(root: &Path) -> PathBuf {
    root.join(".venv/bin/python")
}

fn python_step(root: &Path, script: &str, args: &[&str]) -> Result<()> {
    let script = root.join(script);
    let mut all = vec![script.to_str().unwrap()];
    all.extend_from_slice(args);
    run(venv_python(root), &all, root)
}

fn ensure_python(root: &Path) -> Result<()> {
    let venv = venv_python(root);
    if !venv.exists() {
        run("python3", &["-m", "venv", root.join(".venv").to_str().unwrap()], root)?;
    }
    let requirements = root.join("host/requirements.lock");
    let stamp = root.join(".hkpsx/python-lock.sha256");
    let current = sha256_file(&requirements)?;
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(&current) {
        run(&venv, &["-m", "pip", "install", "--requirement", requirements.to_str().unwrap()], root)?;
        std::fs::write(&stamp, current)?;
    }
    Ok(())
}

fn sdk_source(root: &Path, options: &Options) -> PathBuf {
    options.sdk_source.clone().unwrap_or_else(|| root.parent().unwrap().join("PSoXide"))
}

/// Hydrate the pinned PSoXide SDK snapshot once; a changed cache is an error, never silently replaced.
fn sdk(root: &Path, source: &Path) -> Result<()> {
    let pin = read_json(&root.join("sdk.lock.json"))?;
    let revision = pin["revision"].as_str().ok_or("sdk.lock.json lacks revision")?;
    let destination = root.join(".psoxide");
    let state = root.join(".hkpsx/sdk.json");
    if state.exists() {
        let record = read_json(&state)?;
        let files = record["files"].as_object().ok_or("sdk.json lacks files")?;
        let intact = record["revision"] == revision
            && files.iter().all(|(p, digest)| {
                let path = destination.join(p);
                path.is_file() && sha256_file(&path).ok().as_deref() == digest.as_str()
            });
        if intact {
            return Ok(());
        }
        return Err("Pinned SDK cache changed. Move .psoxide and .hkpsx/sdk.json aside before rehydrating.".into());
    }
    let archive = root.join(".hkpsx/sdk-archive.tar");
    run("git", &["-C", source.to_str().unwrap(), "archive", revision, "-o", archive.to_str().unwrap()], root)?;
    std::fs::create_dir_all(&destination)?;
    let listing = Command::new("tar").args(["-tf", archive.to_str().unwrap()]).output()?;
    if !listing.status.success() {
        return Err("tar listing failed".into());
    }
    run("tar", &["-xf", archive.to_str().unwrap(), "-C", destination.to_str().unwrap()], root)?;
    let mut files = serde_json::Map::new();
    for member in String::from_utf8(listing.stdout)?.lines().filter(|m| !m.ends_with('/')) {
        files.insert(member.to_string(), Value::String(sha256_file(&destination.join(member))?));
    }
    std::fs::write(&state, serde_json::to_string_pretty(&serde_json::json!({"revision": revision, "files": files}))?)?;
    std::fs::remove_file(archive)?;
    Ok(())
}

fn selected_source(root: &Path) -> Result<PathBuf> {
    let doctor = read_json(&root.join(".hkpsx/doctor.json"))?;
    Ok(PathBuf::from(doctor["installs"][0]["data_directory"].as_str().ok_or("doctor.json lacks a selected install")?))
}

fn inputs_unchanged(source: &Path, provenance: &Value) -> bool {
    provenance["inputs"].as_object().is_some_and(|inputs| {
        inputs.iter().all(|(p, meta)| {
            let path = source.join(p);
            path.is_file() && sha256_file(&path).ok().as_deref() == meta["sha256"].as_str()
        })
    })
}

fn code_fingerprint(root: &Path) -> Result<Value> {
    let mut code = serde_json::Map::new();
    for line in COOK_INPUTS.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        code.insert(line.to_string(), Value::String(sha256_file(&root.join(line))?));
    }
    Ok(Value::Object(code))
}

/// Recook only when the Windows inputs, the cooker code or the generated outputs changed.
fn cook_cached(root: &Path, force: bool) -> Result<()> {
    let provenance = root.join(".hkpsx/regions-provenance.json");
    let cache = root.join(".hkpsx/regions-cook-cache.json");
    let code = code_fingerprint(root)?;
    let source = selected_source(root)?;
    if !force && provenance.exists() && cache.exists() {
        let prov = read_json(&provenance)?;
        let old = read_json(&cache)?;
        let outputs_intact = old["outputs"].as_object().is_some_and(|outputs| {
            outputs.iter().all(|(p, h)| {
                let path = root.join(p);
                path.is_file() && sha256_file(&path).ok().as_deref() == h.as_str()
            })
        });
        if prov["source"].as_str() == source.to_str() && old["code"] == code && inputs_unchanged(&source, &prov) && outputs_intact {
            println!("Cook cache hit: source hashes and generated outputs match.");
            return Ok(());
        }
    }
    python_step(root, "host/regions.py", &[])?;
    let mut outputs: BTreeMap<String, String> = BTreeMap::new();
    for name in ["data/room.hk", "data/params.rs", "data/regions.json"] {
        outputs.insert(name.into(), sha256_file(&root.join(name))?);
    }
    for entry in std::fs::read_dir(root.join("data/regions"))? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("chunk_") && name.ends_with(".hk") {
            outputs.insert(format!("data/regions/{name}"), sha256_file(&path)?);
        }
    }
    std::fs::write(&cache, serde_json::to_string_pretty(&serde_json::json!({"code": code, "outputs": outputs}))?)?;
    Ok(())
}

fn menu_cached(root: &Path, force: bool) -> Result<()> {
    let provenance = root.join(".hkpsx/menu-provenance.json");
    let cache = root.join(".hkpsx/menu-cache.json");
    let mut code = serde_json::Map::new();
    for name in ["cook_menu.py", "source.py", "requirements.lock"] {
        code.insert(name.into(), Value::String(sha256_file(&root.join("host").join(name))?));
    }
    let code = Value::Object(code);
    let source = selected_source(root)?;
    if !force && provenance.exists() && cache.exists() {
        let prov = read_json(&provenance)?;
        let old = read_json(&cache)?;
        let output = root.join(prov["output"]["path"].as_str().unwrap_or(""));
        if old["code"] == code
            && prov["source"].as_str() == source.to_str()
            && output.is_file()
            && sha256_file(&output).ok().as_deref() == prov["output"]["sha256"].as_str()
            && inputs_unchanged(&source, &prov)
        {
            println!("Menu cache hit: source hashes and generated output match.");
            return Ok(());
        }
    }
    python_step(root, "host/cook_menu.py", &[])?;
    std::fs::write(&cache, serde_json::to_string_pretty(&serde_json::json!({"code": code}))?)?;
    Ok(())
}

/// Files whose bytes decide every asset script and the guest prepass: all host
/// and tool Python, the cooked region report and the cook cache verdict.
fn assets_key(root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in ["host", "tools"] {
        for entry in std::fs::read_dir(root.join(dir))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "py" || e == "c") {
                files.push(path);
            }
        }
    }
    files.push(root.join("data/regions.json"));
    files.push(root.join(".hkpsx/regions-cook-cache.json"));
    files.push(root.join(".hkpsx/menu-provenance.json"));
    files.sort();
    for path in files {
        hasher.update(path.strip_prefix(root).unwrap_or(&path).to_string_lossy().as_bytes());
        hasher.update(sha256_file(&path)?.as_bytes());
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Every generated file the guest link reads: all of data/ plus the packed
/// scene, certificate and effect-art state under .hkpsx.
fn asset_outputs(root: &Path) -> Result<BTreeMap<String, String>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else if path.is_file() && !path.file_name().is_some_and(|n| n.to_string_lossy().ends_with(".tmp")) {
                out.insert(path.strip_prefix(root).unwrap().to_string_lossy().to_string(), sha256_file(&path)?);
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, &root.join("data"), &mut out)?;
    for state in ["packed-scenes.json", "scene-certificates.json", "ambience.json", "title-music.json", "focus-audio.json",
        "geo-provenance.json", "geo-audio-provenance.json", "lifeblood-provenance.json", "hud-provenance.json",
        "audio-provenance.json", "great-door.json", "battle-gates.json", "break-effects/report.json", "opaque-tiles/report.json",
        "opaque-groups/report.json", "scenery-geometry-budgets.json", "selected-regions.json"] {
        let path = root.join(".hkpsx").join(state);
        if path.is_file() {
            out.insert(format!(".hkpsx/{state}"), sha256_file(&path)?);
        }
    }
    Ok(out)
}

/// Run the asset scripts and the guest prepass unless their inputs and every
/// generated output match the previous successful run. Returns true on a hit.
fn assets_cached(root: &Path, force: bool) -> Result<bool> {
    let cache = root.join(".hkpsx/assets-cache.json");
    let key = assets_key(root)?;
    if !force && cache.exists() {
        let old = read_json(&cache)?;
        if old["key"].as_str() == Some(key.as_str()) {
            let intact = old["outputs"].as_object().is_some_and(|outputs| {
                outputs.iter().all(|(p, h)| {
                    let path = root.join(p);
                    path.is_file() && sha256_file(&path).ok().as_deref() == h.as_str()
                })
            });
            if intact {
                println!("Asset cache hit: cooker code, region report and generated outputs match.");
                return Ok(true);
            }
        }
    }
    for script in ASSET_SCRIPTS {
        python_step(root, &format!("host/{script}"), &[])?;
    }
    run("cargo", &["run", "--quiet", "--manifest-path", "shared/hk-format/Cargo.toml", "--example", "check", "--", "data/room.hk"], root)?;
    python_step_module(root, "build_guest", "prepass")?;
    let outputs = asset_outputs(root)?;
    std::fs::write(&cache, serde_json::to_string_pretty(&serde_json::json!({"key": key, "outputs": outputs}))?)?;
    Ok(false)
}

/// Run one function of a host module in the project venv.
fn python_step_module(root: &Path, module: &str, function: &str) -> Result<()> {
    let code = format!("import sys;sys.path.insert(0,'host');import {module};{module}.{function}()");
    run(venv_python(root), &["-c", &code], root)
}

/// `pgo` is `build` with a profile-guided guest in place of the ordinary one.
fn build(root: &Path, options: &Options, pgo: bool) -> Result<()> {
    std::fs::create_dir_all(root.join(".hkpsx"))?;
    let doctor = root.join("tools/doctor.py");
    let mut args = vec![doctor.to_str().unwrap()];
    if let Some(path) = &options.hollow_knight {
        args.extend(["--hollow-knight", path.to_str().unwrap()]);
    }
    run("python3", &args, root)?;
    ensure_python(root)?;
    // The host suite is cheap and nothing else ran it, so a cooker regression
    // used to survive whole builds: adding a required constant to
    // `generated_params` broke a fixture and nine builds shipped before anyone
    // noticed. Fail here rather than after ten minutes of cooking.
    run(venv_python(root), &["-m", "unittest", "discover", "-s", "tests", "-q"], root)?;
    // And the simulation's own suite, for the same reason one build later: the
    // charm board landed with four hk-sim test targets failing to compile, and
    // nothing here ran them, so it shipped. These path-include the guest
    // modules, so they are the only check that the guest's own logic still
    // holds before ten minutes of cooking.
    run("cargo", &["test", "-q", "--manifest-path", "shared/hk-sim/Cargo.toml"], root)?;
    sdk(root, &sdk_source(root, options))?;
    cook_cached(root, options.recook)?;
    menu_cached(root, options.recook)?;
    let assets_hit = assets_cached(root, options.recook)?;
    let telemetry: &[&str] = if options.telemetry { &["--telemetry"] } else { &[] };
    let mut guest_args = telemetry.to_vec();
    if assets_hit {
        guest_args.push("--no-prepass");
    }
    if pgo {
        profile_guided_guest(root, options, &guest_args)?;
    } else {
        python_step(root, "host/build_guest.py", &guest_args)?;
    }
    let mut report_args = telemetry.to_vec();
    if pgo {
        // Its own report file: docs/BUDGET.md pins the ordinary build's figures.
        report_args.push("--pgo");
    }
    python_step(root, "host/build_report.py", &report_args)?;
    if options.validate {
        validate(root, options)?;
    }
    Ok(())
}

/// Build the canonical guest optimised with the emulator's own instruction counts.
///
/// PSoXide counts every guest instruction exactly, so no instrumented build is
/// needed: a build with profiling line tables replays each tape, the pinned
/// SDK's psoxide-pgo turns the PC histogram into an LLVM sample profile through
/// the matching ELF's DWARF, and the canonical build is compiled with it.
/// Profile names carry crate hashes that depend on the checkout path, so the
/// profile is rebuilt here every time and never committed.
fn profile_guided_guest(root: &Path, options: &Options, guest_args: &[&str]) -> Result<()> {
    if root.to_string_lossy().chars().any(char::is_whitespace) {
        return Err(format!(
            "pgo cannot run from {}: the guest RUSTFLAGS are split on whitespace, so the profile \
             and linker paths inside this checkout cannot be passed. Use a checkout path without spaces.",
            root.display()
        )
        .into());
    }
    let (frontend, emulator_revision) = pinned_frontend(root, options)?;
    let tapes: Vec<PathBuf> = if options.tapes.is_empty() {
        PGO_TAPES.iter().map(|name| root.join(format!("tools/tapes/{name}.pxtape"))).collect()
    } else {
        options.tapes.clone()
    };
    let work = root.join(".hkpsx/pgo");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work)?;
    let (collect, elf) = (work.join("collect"), work.join("elf"));
    let telemetry: &[&str] = if options.telemetry { &["--telemetry"] } else { &[] };
    let mut args = guest_args.to_vec();
    args.extend(["--profile", "collect", "--work", collect.to_str().unwrap()]);
    python_step(root, "host/build_guest.py", &args)?;
    let mut args = telemetry.to_vec();
    args.extend(["--no-prepass", "--no-disc", "--profile", "elf", "--work", elf.to_str().unwrap()]);
    python_step(root, "host/build_guest.py", &args)?;

    let mut samples: BTreeMap<String, u64> = BTreeMap::new();
    for (index, tape) in tapes.iter().enumerate() {
        let data = std::fs::read(tape)?;
        if data.len() < 16 || &data[..8] != b"PXITAPE2" {
            return Err(format!("{} is not a poll-bound tape", tape.display()).into());
        }
        // Stop where the tape ends, as tools/replay_cue.py does.
        let count = u32::from_le_bytes(data[8..12].try_into()?);
        let start = u32::from_le_bytes(data[12..16].try_into()?);
        let log = work.join(format!("pc-{index}.csv"));
        let mut command = Command::new(&frontend);
        command
            .args(["launch", "--path"])
            .arg(collect.join("disc/hk-psx.cue"))
            .args(["--embedded-playtest", "--config-dir"])
            .arg(work.join(format!("emulator-{index}")))
            .args(["--steps", "6000000000", "--input-tape"])
            .arg(tape)
            .args(["--stop-at-poll", &(start + count).to_string(), "--pc-sample-log"])
            .arg(&log)
            // Prime, so the sampler cannot fall into step with a loop.
            .args(["--pc-sample-instructions", "61"]);
        let stem = tape.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if let Some((_, card)) = MEMCARDS.iter().find(|(route, _)| *route == stem) {
            // The replay writes the card back: use a scratch copy so the fixture stays put.
            let scratch = work.join(format!("{stem}.mcd"));
            std::fs::copy(root.join("tools/cards").join(card), &scratch)?;
            command.arg("--memcard").arg(scratch);
        }
        println!("+ {command:?}");
        if !command.status()?.success() {
            return Err(format!("profiling replay of {} failed", tape.display()).into());
        }
        for line in std::fs::read_to_string(&log)?.lines().skip(1) {
            let mut fields = line.split(',');
            if let (Some(pc), Some(Ok(count))) = (fields.next(), fields.next().map(str::parse::<u64>)) {
                *samples.entry(pc.to_string()).or_default() += count;
            }
        }
        std::fs::remove_file(&log)?;
    }
    if samples.is_empty() {
        return Err("the profiling replays recorded no samples".into());
    }
    let merged = work.join("pc.csv");
    let mut text = String::from("pc,samples\n");
    for (pc, count) in &samples {
        text.push_str(&format!("{pc},{count}\n"));
    }
    std::fs::write(&merged, text)?;
    let profile = work.join("hk-psx.prof");
    let tool = root.join(".psoxide/tools/psoxide-pgo/Cargo.toml");
    run("cargo", &["run", "--locked", "--release", "--manifest-path", tool.to_str().unwrap(), "--",
        elf.join("hk-psx.elf").to_str().unwrap(), merged.to_str().unwrap(), profile.to_str().unwrap()], root)?;
    std::fs::remove_file(&merged)?;
    // What the profile came from, so two runs can be compared by hash.
    let provenance = serde_json::json!({
        "emulator_revision": emulator_revision,
        "frontend_sha256": sha256_file(&frontend)?,
        "tapes": tapes.iter().map(|t| Ok(serde_json::json!({"path": t, "sha256": sha256_file(t)?})))
            .collect::<Result<Vec<_>>>()?,
        "profile_sha256": sha256_file(&profile)?,
    });
    std::fs::write(work.join("provenance.json"), serde_json::to_string_pretty(&provenance)?)?;
    let mut args = telemetry.to_vec();
    args.extend(["--no-prepass", "--profile", profile.to_str().unwrap()]);
    python_step(root, "host/build_guest.py", &args)
}

/// The emulator `pgo` profiles with: the revision in emulator.lock.json,
/// exported from the sibling PSoXide-emulator checkout (or --emulator-source)
/// and built once under .hkpsx/emulator/<revision>.
///
/// The PC histogram is the emulator's instruction stream, so it moves with the
/// emulator's timing: vblank waits, CD and DMA latencies all change how often a
/// loop runs. Two runs profiled by two different frontends produced two
/// different PGO images from the same commit. Pinning the emulator makes the
/// profile, and so the image, a function of this checkout alone. --frontend
/// still picks the emulator the validation routes replay on.
fn pinned_frontend(root: &Path, options: &Options) -> Result<(PathBuf, String)> {
    let pin = read_json(&root.join("emulator.lock.json"))?;
    let revision = pin["revision"].as_str().ok_or("emulator.lock.json lacks revision")?.to_string();
    let tree = root.join(".hkpsx/emulator").join(&revision);
    let target = tree.join("target");
    let binary = target.join("release/frontend");
    let stamp = tree.join(".hk-frontend.json");
    if let (Ok(record), true) = (read_json(&stamp), binary.is_file()) {
        if record["revision"] == revision.as_str() && record["sha256"].as_str() == Some(&sha256_file(&binary)?) {
            return Ok((binary, revision));
        }
    }
    let source = options.emulator_source.clone().unwrap_or_else(|| root.parent().unwrap().join("PSoXide-emulator"));
    std::fs::create_dir_all(&tree)?;
    let archive = root.join(".hkpsx/emulator-archive.tar");
    run("git", &["-C", source.to_str().unwrap(), "archive", &revision, "-o", archive.to_str().unwrap()], root)?;
    run("tar", &["-xf", archive.to_str().unwrap(), "-C", tree.to_str().unwrap()], root)?;
    std::fs::remove_file(&archive)?;
    // Its SDK component comes from the same checkout this build's SDK does.
    let sdk = format!("sdk={}", sdk_source(root, options).display());
    run("python3", &["tools/bootstrap-components.py", "--root", tree.to_str().unwrap(), "--source", &sdk], &tree)?;
    println!("+ cargo build --locked --release -p frontend (CARGO_TARGET_DIR={})", target.display());
    let status = Command::new("cargo")
        .args(["build", "--locked", "--release", "-p", "frontend"])
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(&tree)
        .status()?;
    if !status.success() {
        return Err(format!("building the pinned frontend at {revision} failed with {status}").into());
    }
    std::fs::write(&stamp, serde_json::to_string_pretty(&serde_json::json!({
        "revision": revision, "sha256": sha256_file(&binary)?}))?)?;
    Ok((binary, revision))
}

fn frontend(root: &Path, options: &Options) -> Result<PathBuf> {
    let frontend = options
        .frontend
        .clone()
        .or_else(|| std::env::var_os("HK_PSX_FRONTEND").map(PathBuf::from))
        .unwrap_or_else(|| root.parent().unwrap().join("PSoXide-emulator/target/release/frontend"));
    if !frontend.is_file() {
        return Err(format!("emulator frontend not found at {} (pass --frontend)", frontend.display()).into());
    }
    Ok(frontend)
}

/// Replay every route tape against the final CUE; a fault or an incomplete tape fails the build.
fn validate(root: &Path, options: &Options) -> Result<()> {
    let frontend = frontend(root, options)?;
    let out = root.join(".hkpsx/validate");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    // Replays are independent deterministic emulator runs: spawn them all at
    // once and collect in route order, so the wall time is the longest route.
    let children: Vec<_> = ROUTES
        .iter()
        .map(|(name, _)| {
            let tape = root.join(format!("tools/tapes/{name}.pxtape"));
            let output = out.join(name);
            let mut args = vec![root.join("tools/replay_cue.py"), "--tape".into(), tape, "--output".into(), output,
                "--screenshot-interval".into(), "400".into(), "--frontend".into(), frontend.clone()];
            if let Some((_, card)) = MEMCARDS.iter().find(|(route, _)| route == name) {
                // The replay writes the card back: run against a scratch copy so the fixture stays put.
                let scratch = out.join(format!("{name}.mcd"));
                let _ = std::fs::copy(root.join("tools/cards").join(card), &scratch);
                args.extend(["--memcard".into(), scratch]);
            }
            Command::new(venv_python(root))
                .args(args)
                .current_dir(root)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
        })
        .collect();
    let mut summary = serde_json::Map::new();
    let mut failed = Vec::new();
    for ((name, gates), child) in ROUTES.iter().zip(children) {
        let output = out.join(name);
        let replay = child?.wait_with_output()?;
        let text = String::from_utf8_lossy(&replay.stdout).to_string();
        std::fs::write(out.join(format!("{name}.log")), format!("{text}{}", String::from_utf8_lossy(&replay.stderr)))?;
        let verdict: Value = text.find('{').and_then(|start| serde_json::from_str(&text[start..]).ok()).unwrap_or(Value::Null);
        let mut ok = replay.status.success() && verdict["completed"] == true;
        if ok && *gates {
            let check = Command::new(venv_python(root))
                .args([root.join("tools/validate_scene_gates.py").to_str().unwrap(), output.to_str().unwrap()])
                .current_dir(root)
                .output()?;
            std::fs::write(out.join(format!("{name}-gates.log")), [check.stdout.as_slice(), check.stderr.as_slice()].concat())?;
            ok = check.status.success();
        }
        let ram = &verdict["final_ram"];
        for (route, key, expected) in REQUIRED {
            if route == name && ram[*key].as_u64() != Some(*expected) {
                println!("{name}: {key}={} expected {expected}", ram[*key]);
                ok = false;
            }
        }
        for (route, key, least) in REQUIRED_MIN {
            if route == name && ram[*key].as_u64().is_none_or(|v| v < *least) {
                println!("{name}: {key}={} expected at least {least}", ram[*key]);
                ok = false;
            }
        }
        println!(
            "{} {name}: completed={} faults={} x={} y={} region={} scene_loads={} gate_loads={}",
            if ok { "PASS" } else { "FAIL" }, verdict["completed"], verdict["faults"], ram["HK_PLAYER_X"], ram["HK_PLAYER_Y"],
            ram["HK_REGION_ID"], ram["HK_SCENE_LOADS"], ram["HK_SCENE_GATE_LOADS"]
        );
        if !ok {
            failed.push(*name);
        }
        summary.insert(name.to_string(), serde_json::json!({"pass": ok, "verdict": verdict}));
    }
    let bin = root.join("dist/hk-psx.exe");
    summary.insert("exe_sha256".into(), Value::String(if bin.is_file() { sha256_file(&bin)? } else { String::new() }));
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&Value::Object(summary))?)?;
    if failed.is_empty() {
        println!("All {} routes pass on the final CUE.", ROUTES.len());
        // Card writes free a file's directory entry before rewriting it, so a
        // save that used one file had a window where an interruption left
        // nothing loadable. Prove the alternating slots close it.
        let cut = Command::new(venv_python(root))
            .args([root.join("tools/validate_power_cut.py").to_str().unwrap(),
                "--output", out.join("power-cut").to_str().unwrap()])
            .current_dir(root)
            .output()?;
        std::fs::write(out.join("power-cut.log"), [cut.stdout.as_slice(), cut.stderr.as_slice()].concat())?;
        print!("{}", String::from_utf8_lossy(&cut.stdout));
        if !cut.status.success() {
            return Err(format!("power-cut check failed: {}", String::from_utf8_lossy(&cut.stderr)).into());
        }
        Ok(())
    } else {
        Err(format!("routes failed: {}", failed.join(", ")).into())
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: cargo run --release -- <build|pgo|validate|help> [--recook] [--telemetry] [--no-validate]\n       \
         [--hollow-knight DIR] [--sdk-source DIR] [--emulator-source DIR] [--frontend PATH] [--tape PATH]...\n\
         build     cook (cached), build the guest, replace the sole hk-psx.bin/.cue, report, then validate\n\
         pgo       build, with the guest optimised by a profile of --tape replays (default: kings-climb\n          \
         and crossroads-gate), replayed on the emulator pinned in emulator.lock.json (built once\n          \
         from --emulator-source, default ../PSoXide-emulator); needs a checkout path without spaces\n\
         validate  replay the route tapes in tools/tapes against the current CUE"
    );
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_else(|| "build".into());
    let mut options = Options {
        hollow_knight: None, sdk_source: None, emulator_source: None, frontend: None, telemetry: false, recook: false, validate: true, tapes: Vec::new(),
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--recook" => options.recook = true,
            "--telemetry" => options.telemetry = true,
            "--no-validate" => options.validate = false,
            "--hollow-knight" => options.hollow_knight = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--sdk-source" => options.sdk_source = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--emulator-source" => options.emulator_source = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--frontend" => options.frontend = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--tape" => options.tapes.push(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            _ => usage(),
        }
    }
    let root = root();
    let result = match action.as_str() {
        "build" => build(&root, &options, false),
        "pgo" => build(&root, &options, true),
        "validate" => validate(&root, &options),
        _ => usage(),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
