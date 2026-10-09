//! Single-command driver: `cargo hk-build build` cooks (cached), builds
//! the guest, replaces the sole disc pair, writes the build report and replays
//! the validation routes. The Unity extraction and cookers stay in `host/*.py`;
//! this binary owns the order, the caches and the pass/fail verdict.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

mod disc;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Cooker inputs, one repo-relative path per line; shared with host/regions.py.
const COOK_INPUTS: &str = include_str!("../cook_inputs.txt");
const ASSET_SCRIPTS: &[&str] = &[
    // The title and fight songs as XA-ADPCM, ported to Rust (hk-cook src/xa_music.rs).
    "rust:xa-music", "cook_audio.py", "cook_hud.py",
    // The two SPU banks stacked above ambience, before it: ambience reads their
    // bases and sizes from data/focus-audio.rs and data/runner-audio.rs to
    // place itself below them (and refuses an overlap), and neither bank's
    // size depends on ambience.
    "focus_audio.py", "runner_audio.py", "ambience.py", "read_points.py",
    "geo.py", "geo_audio.py", "lifeblood.py",
    // Breakable and secret particle effects, ported to Rust (hk-cook).
    "rust:break-effects",
    "great_door.py",
    // Chests and pickups: their frames are bound in regions.json by the region
    // postpass, which this turns into data/pickups.rs.
    "pickups.py",
    // The arena gates, after both tables that share the guest's scripted-edge
    // scratch: it reads data/lifeblood.rs and data/great_door.rs to check the
    // union in one catalogue slot rather than its own rows alone.
    "battle_gates.py",
    // The Blockers' Terrain Block edges, after the gates: its scratch budget
    // check adds the gates' rows in the same catalogue slot.
    "blocker_terrain.py",
    // Soul totems and area titles, read from the source scenes; the totems
    // before the scene sounds, which give their scenes the totem's sound.
    "soul_totems.py", "title_cards.py",
    // Per-scene one-shot banks: placed around ambience's stems (ambience.json)
    // and selected from the gates, benches, secrets and actors the cook admitted.
    // Ported to Rust (hk-cook src/scene_sfx.rs).
    "rust:scene-sfx",
    "shade.py", "ability_art.py",
    // Goams, stalactites and grub jars: their placements come from the region
    // report and the cooked scene draws regions.py wrote. Ported to Rust
    // (hk-cook), so it runs in-process.
    "rust:props",
    // Area music premixes. After ambience.py,
    // whose report it reads for the gate graph through host/ambience.py.
    "area_music.py",
    // After read_points.py: the charm catalogue wraps its descriptions against
    // the glyph advances that cook writes into data/read_points.rs.
    "charms.py",
    // Sly's shop, which the guest links now that Room_shop is admitted. It
    // needs geo.py's report for the economy, read_points.py's advances to wrap
    // its descriptions and items.py's fuse rules, so it follows all three.
    "items.py",
    "shops.py",
    // The quick map's art and tables, then Cornifer's conversation, which
    // wraps its pages against read_points.py's advances and reads his cooked
    // placement out of data/regions.json.
    "game_map.py", "cornifer.py",
    // The Snail Shaman's art, the spell orb and their conversation.
    "shaman.py",
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
    // Sly's shop is shut on a fresh save, as in the source: Dirtmouth's
    // `Check Opened` turns `Sly_shop/open` off until `slyRescued`, and door_sly
    // is a child of it, so no gate cooks there. Sly is rescued in
    // Room_ruinhouse, which is not on this disc, so the shop stays shut for the
    // whole slice (its logic is still covered by tests/shop_runtime.rs).
    // town-shop is the same tape as before, on its 1,000 Geo card: it walks to
    // where the door was, presses UP and the buttons that used to buy twice,
    // and nothing opens and nothing is spent.
    ("town-shop", "HK_SCENE_GATE_LOADS", 0), ("town-shop", "HK_SHOP_OPENED", 0),
    ("town-shop", "HK_SHOP_PURCHASES", 0), ("town-shop", "HK_SHOP_GEO_SPENT", 0),
    ("town-shop", "HK_GEO_WALLET", 1000),
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
    ("boss-fight", "HK_SAVE_LOADED", 1),
    // The last jump breaks the floor and the fight ends in the room below it:
    // the Knight follows the boss down, finishes the Head in `Opened 2` and
    // stands there while the Death Head drops out and the arena opens. The
    // route's death exposure is a repeated jump-and-swing chosen because it
    // still kills with a poll dropped or doubled anywhere in it, which is what
    // tools/boss_sim.py's own search could not promise.
    ("boss-fight", "HK_REGION_ID", 232), ("boss-fight", "HK_FK_FLOOR", 2),
    ("boss-fight", "HK_FK_HEAD_HITS", 28),
    // `Floor Break` takes the mixer to `Silent`: the Boss1 XA has faded and
    // stopped by the time the Head is finished below, and it started once.
    ("boss-fight", "HK_MUSIC_BOSS", 0), ("boss-fight", "HK_MUSIC_BOSS_STARTS", 1),
    // Every False Knight voice the fight asks Crossroads_10's scene bank for is
    // there (host/hk-cook/src/scene_sfx.rs): none falls through to a miss.
    ("boss-fight", "HK_SCENE_SFX_MISSED", 0),
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
    ("boss-death", "HK_REGION_ID", 239), ("boss-death", "HK_FK_FLOOR", 0),
    // The slam wave. The Knight backs away to the arena's left end, so the
    // boss slams from range (`S Check Hero Pos` skips the leap from twelve
    // units) and each `S Attack Recover` sends a Shockwave Wave across the
    // floor at it. The first one lands, the next three are jumped; the tape is
    // tools/boss_sim.py's, which shares hk_sim::shockwave with the guest, and
    // the disc took exactly the hits it predicted. The music is still the
    // fight's XA when the tape ends, because nothing broke the floor.
    ("boss-wave", "HK_SAVE_LOADED", 1), ("boss-wave", "HK_FK_TRIGGERED", 1),
    ("boss-wave", "HK_FK_WAVES", 4), ("boss-wave", "HK_FK_WAVE_HITS", 1),
    ("boss-wave", "HK_HEALTH", 2), ("boss-wave", "HK_DEATHS", 0), ("boss-wave", "HK_CHEATS", 0),
    ("boss-wave", "HK_FK_STAGGERS", 0), ("boss-wave", "HK_MUSIC_BOSS", 1),
    ("boss-wave", "HK_REGION_ID", 239),
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
    ("greenpath-walk", "HK_ROOM_LOAD_ERROR", 0), ("greenpath-walk", "HK_CD_STREAM_ERROR", 0),
    // grub-jar boots from a seeded card (tools/seed_card.py) on the Crossroads_05
    // ledge beside its grub jar, facing it, and swings: the first swing breaks
    // the jar, the grub is freed and gone by the end, and the world store
    // holds one Grub item. Seeded, so it proves the jar, not the way to it.
    //   9:start:1,12:cross:1,200:square:2,230:square:2,260:square:2 (450 polls)
    ("grub-jar", "HK_GRUBS_FREED", 1), ("grub-jar", "HK_GRUBS", 1), ("grub-jar", "HK_CHEATS", 0),
    ("grub-jar", "HK_DEATHS", 0), ("grub-jar", "HK_HEALTH", 5), ("grub-jar", "HK_REGION_ID", 194),
    ("grub-jar", "HK_SAVE_LOADED", 1),
    // baldur-spell is the Greenpath exit: a seeded card right of Crossroads_11_alt's
    // Elder Baldur, the spell and three other cheats on from the pause menu
    // (CHEATS 1031 says so: Vengeful Spirit cannot be earned on this branch
    // yet), walk left to it and cast every 45 polls. Four spell hits kill it,
    // its Terrain Block lifts (0be2f3e) and the Knight walks on past it into
    // region 338, before the drop into the acid. tools/tapes/baldur-spell.route.
    ("baldur-spell", "HK_ENEMY_KILLS", 1), ("baldur-spell", "HK_ENEMY_HITS", 4),
    ("baldur-spell", "HK_DEATHS", 0), ("baldur-spell", "HK_REGION_ID", 338),
    ("baldur-spell", "HK_CHEATS", 1031), ("baldur-spell", "HK_SAVE_LOADED", 1),
    // mawlek-fight boots from a seeded card (tools/seed_card.py from grub-jar.mcd)
    // on the Crossroads_09 floor left of the arena: the only walk in is from
    // Crossroads_36, past an Elder Baldur the spell cheat has to kill, and the
    // Crossroads_33 side is `full_wall_left` until the arena is won. Seeded, so
    // it proves the fight, not the way to it. Invincibility and the max nail
    // from the pause menu (KILL_CHEATS 3 says so), walk right into `Alert Range
    // New`: it wakes, leaps out of the background, roars, the EnemyBattle cue
    // starts as an XA song, and the Knight swings at it every 16 polls. Fifteen max
    // nail hits kill it, the corpse steams and blows, Boss Defeat plays from
    // XA channel 3 (HK_MUSIC_BOSS_TRACK) to its end, and the gates open 10.5 s after BATTLE END.
    // The Heart Piece `End Wait` shows 5.5 s after BATTLE END is then a running
    // jump up and right from the arena floor: one mask shard (SHOP_SHARDS 1),
    // saved beside the arena's `Activated` (WORLD_ITEMS 2).
    // The counts are this tape's; SWIPES, PARRIES and HEAD_SHOTS are minimums.
    // tools/tapes/mawlek-fight.route.
    ("mawlek-fight", "HK_MW_WOKEN", 1), ("mawlek-fight", "HK_MW_DEATHS", 1),
    ("mawlek-fight", "HK_MW_BLOWN", 1), ("mawlek-fight", "HK_MW_ARENA", 4),
    ("mawlek-fight", "HK_MW_ACTIVATED", 1), ("mawlek-fight", "HK_MW_KILL_CHEATS", 3),
    ("mawlek-fight", "HK_MW_HITS", 15), ("mawlek-fight", "HK_MW_SPRAYS", 1),
    ("mawlek-fight", "HK_MW_LEAPS", 4), ("mawlek-fight", "HK_MW_PHASE", 22),
    ("mawlek-fight", "HK_ARENA_GATE_CLOSES", 1), ("mawlek-fight", "HK_ARENA_GATE_OPENS", 1),
    ("mawlek-fight", "HK_MUSIC_BOSS", 0), ("mawlek-fight", "HK_MUSIC_BOSS_STARTS", 2),
    ("mawlek-fight", "HK_MUSIC_BOSS_TRACK", 3), ("mawlek-fight", "HK_SCENE_SFX_MISSED", 0),
    ("mawlek-fight", "HK_DEATHS", 0), ("mawlek-fight", "HK_WORLD_ITEMS", 2),
    ("mawlek-fight", "HK_SHOP_SHARDS", 1),
    ("mawlek-fight", "HK_SAVE_LOADED", 1), ("mawlek-fight", "HK_CHEATS", 3),
    ("mawlek-fight", "HK_REGION_ID", 225),
    // gruz-fight boots from a seeded card (tools/seed_card.py from secret-c04.mcd)
    // on Gruz Mother's ledge in Crossroads_04, left of where it sleeps. Seeded,
    // so it proves the fight, not the way to it. Invincibility and the max nail
    // from the pause menu (KILL_CHEATS 3 says so), walk right into `Battle
    // Range` and hit it once: it wakes (the Gruz Mother title, BATTLE START
    // seals the arena, the EnemyBattle cue), and the Knight steps back and
    // watches it buzz, charge and slam for twelve seconds before swinging every
    // 16 polls. Five max-nail hits kill it; the corpse steams and blows (Boss
    // Defeat on XA), the burster drops 50 Geo, lands, gurgles and bursts,
    // releasing the seven reserve flies, whose deaths (`Battle Enemies` 7) end
    // the arena: BG OPEN two seconds after the last, `Activated` saved.
    // SCENE_SFX_MISSED counts the Gruz Mother clips Crossroads_04's full scene
    // bank refuses (host/hk-cook/src/scene_sfx.rs). tools/tapes/gruz-fight.route.
    ("gruz-fight", "HK_GZ_WOKEN", 1), ("gruz-fight", "HK_GZ_DEATHS", 1),
    ("gruz-fight", "HK_GZ_BLOWN", 1), ("gruz-fight", "HK_GZ_RELEASED", 1),
    ("gruz-fight", "HK_GZ_FLY_DEATHS", 7), ("gruz-fight", "HK_GZ_ARENA", 4),
    ("gruz-fight", "HK_GZ_ACTIVATED", 1), ("gruz-fight", "HK_GZ_KILL_CHEATS", 3),
    ("gruz-fight", "HK_GZ_HITS", 5), ("gruz-fight", "HK_GZ_PHASE", 26),
    ("gruz-fight", "HK_ARENA_GATE_CLOSES", 1), ("gruz-fight", "HK_ARENA_GATE_OPENS", 1),
    ("gruz-fight", "HK_DEATHS", 0), ("gruz-fight", "HK_CHEATS", 3),
    // door-jiji is a door whose room is not on the disc: a seeded card by
    // Dirtmouth's door_jiji, walk right, UP opens the source's locked-door line
    // (Prompts JIJI_DOOR_NOKEY), Cross closes it and UP opens it again. No gate
    // load. tools/tapes/door-jiji.route.
    ("door-jiji", "HK_READ_OPENED", 2), ("door-jiji", "HK_READ_CLOSED", 1),
    ("door-jiji", "HK_READ_SOURCE", 4407), ("door-jiji", "HK_SCENE_GATE_LOADS", 0),
    ("door-jiji", "HK_REGION_ID", 102), ("door-jiji", "HK_DEATHS", 0), ("door-jiji", "HK_CHEATS", 0),
    // husk-guard: a seeded card eleven units left of Crossroads_48's Husk Guard,
    // and the Knight stands still for 1,500 polls. The guard wakes on sight,
    // walks in, and alternates club slams (short of the Knight) with stomps,
    // whose two shockwaves (counted with the False Knight's, HK_FK_WAVES) are
    // what reach him: four stomps, eight waves, four masks, no death.
    ("husk-guard", "HK_FK_WAVES", 8), ("husk-guard", "HK_FK_WAVE_HITS", 4),
    ("husk-guard", "HK_DEATHS", 0), ("husk-guard", "HK_REGION_ID", 540),
    ("husk-guard", "HK_CHEATS", 0), ("husk-guard", "HK_SAVE_LOADED", 1),
    // The journey: Start Game to the False Knight and back, on one card and
    // four power cycles, with no fixture anywhere. See JOURNEY for the order
    // and tools/tapes/journey-*.route for what each segment presses.
    //
    // journey-kings starts a new game on an empty card, climbs King's Pass,
    // breaks the Great Door into Dirtmouth and saves at the Dirtmouth bench.
    // The six walls it breaks on the way are persistent breakables, so the
    // save is 166 + 6 * 4 = 190 bytes. Invincibility carries it past the
    // King's Pass Crawlers, whose knockback knocks every open-loop climb off
    // its route (ledger, route coverage); CHEATS 1 says so rather than hiding it.
    ("journey-kings", "HK_SAVE_LOADED", 0), ("journey-kings", "HK_CHEATS", 1),
    ("journey-kings", "HK_GREAT_DOOR_OPENED", 1), ("journey-kings", "HK_SCENE_GATE_LOADS", 1),
    ("journey-kings", "HK_BENCH_RESTS", 1), ("journey-kings", "HK_SAVE_WRITES", 1),
    ("journey-kings", "HK_SAVE_ERRORS", 0), ("journey-kings", "HK_SAVE_BYTES", 198),
    ("journey-kings", "HK_WORLD_ITEMS", 8), ("journey-kings", "HK_WORLD_OVERFLOW", 0),
    ("journey-kings", "HK_DEATHS", 0), ("journey-kings", "HK_REGION_ID", 100),
    // journey-crossroads continues from that save and crosses nine scene gates
    // to the Crossroads_47 stag bench: the well, the Crossroads_07 shaft to its
    // floor, Crossroads_33, 08, 13, 42 and 19, up through Crossroads_19's top
    // gate into the bottom of Crossroads_03 and out of its left2. The two
    // shorter ways are shut in the source on a fresh save (the Crossroads_33
    // sliding wall and Crossroads_03's toll gates), and the Crossroads_19 top
    // gate is the one this port could not take until its bottom-gate entry
    // stopped dropping the Knight back through the hole he came up.
    ("journey-crossroads", "HK_SAVE_LOADED", 1), ("journey-crossroads", "HK_WORLD_RESTORED", 8),
    ("journey-crossroads", "HK_SCENE_GATE_LOADS", 9), ("journey-crossroads", "HK_SAVE_WRITES", 1),
    ("journey-crossroads", "HK_SAVE_ERRORS", 0), ("journey-crossroads", "HK_WORLD_ITEMS", 9),
    ("journey-crossroads", "HK_DEATHS", 0), ("journey-crossroads", "HK_REGION_ID", 535),
    // journey-false-knight continues from the stag bench, climbs Crossroads_03
    // to its left1, crosses Crossroads_21 and climbs Crossroads_10 to the arena
    // floor, turns Invincibility off in the pause menu and walks into the
    // trigger from the right. The fight is tools/boss_sim.py's search from that
    // arrival, and FK_KILL_CHEATS 0 is the cheat bits on the tick the boss died,
    // which is what makes the kill fair even though the travel either side of
    // it is not. Since the floor break, the fight ends in the room below the
    // arena: the tape keeps the original travel and the first three phases,
    // and the death exposure is the same repeated jump-and-swing the
    // boss-fight route uses. Then Invincibility back on, along the bottom of
    // Crossroads_10 to its right1 once the gates reopen, the original route
    // from Crossroads_21 on back to the stag bench, and a save that carries
    // the arena's Activated (a seventh item) and falseKnightDefeated with
    // falseKnightFirstPlop.
    ("journey-false-knight", "HK_SAVE_LOADED", 1), ("journey-false-knight", "HK_WORLD_RESTORED", 9),
    ("journey-false-knight", "HK_FK_TRIGGERED", 1), ("journey-false-knight", "HK_FK_DEATHS", 1),
    ("journey-false-knight", "HK_FK_KILL_CHEATS", 0), ("journey-false-knight", "HK_FK_STAGGERS", 3),
    ("journey-false-knight", "HK_FK_CONVERSIONS", 3), ("journey-false-knight", "HK_FK_ARENA", 4),
    ("journey-false-knight", "HK_FK_ACTIVATED", 1), ("journey-false-knight", "HK_FK_BARRELS", 26),
    // 26 barrels: what the rage phases of the boss_sim fight (BOSS_SIM_RELOAD=205: the arena
    // trigger reloads the region and reseeds the 50 Hz phase) drop.
    ("journey-false-knight", "HK_FK_BARRELS_BROKEN", 26),
    ("journey-false-knight", "HK_ARENA_GATE_CLOSES", 1), ("journey-false-knight", "HK_ARENA_GATE_OPENS", 1),
    ("journey-false-knight", "HK_DEATHS", 0), ("journey-false-knight", "HK_SAVE_WRITES", 1),
    ("journey-false-knight", "HK_SAVE_ERRORS", 0), ("journey-false-knight", "HK_WORLD_ITEMS", 10),
    ("journey-false-knight", "HK_WORLD_PLAYER", 3), ("journey-false-knight", "HK_SAVE_BYTES", 206),
    ("journey-false-knight", "HK_REGION_ID", 535), ("journey-false-knight", "HK_FK_FLOOR", 2),
    // journey-reload boots that card and walks the same way back towards the
    // arena, and where it used to cross the arena floor it now falls through
    // the broken one before it reaches the Battle Scene trigger. The boss
    // never leaves the ceiling: nothing triggered, nothing dropped, the gates
    // quick-opened on entry and never closed, and the Crossroads_10 gates are
    // open (ARENA_GATES 1 is Crossroads_04's gate, closed on load, alone). The
    // seven items restored are the six King's Pass walls and the arena.
    ("journey-reload", "HK_SAVE_LOADED", 1), ("journey-reload", "HK_WORLD_RESTORED", 10),
    ("journey-reload", "HK_WORLD_ITEMS", 10), ("journey-reload", "HK_WORLD_PLAYER", 3),
    ("journey-reload", "HK_FK_TRIGGERED", 0), ("journey-reload", "HK_FK_DROPPED", 0),
    ("journey-reload", "HK_FK_DEATHS", 0), ("journey-reload", "HK_FK_ACTIVATED", 1),
    ("journey-reload", "HK_FK_ARENA", 4), ("journey-reload", "HK_ARENA_GATE_CLOSES", 0),
    ("journey-reload", "HK_ARENA_GATE_OPENS", 1), ("journey-reload", "HK_ARENA_GATES", 1),
    // The floor stays broken on the reload, so the walk that used to cross
    // the arena floor drops through the hole into the room below instead.
    ("journey-reload", "HK_DEATHS", 0), ("journey-reload", "HK_REGION_ID", 232),
    ("journey-reload", "HK_FK_FLOOR", 2),
    // The map and its mapper. cornifer-map boots from tools/cards/cornifer-map.mcd,
    // town-continue's record seeded in Crossroads_33 beside Cornifer with 100 Geo
    // (tools/seed_card.py), talks to him, pages through the meeting, buys the map
    // for 30 at the yes/no box, clears the first-map prompt and the Iselda pages,
    // holds L2 for the quick map, then talks again (the introduction). The 16
    // rooms are Cornifer's charted Crossroads rooms, rough, with no quill.
    ("cornifer-map", "HK_CORNIFER_SOLD", 1), ("cornifer-map", "HK_CORNIFER_TALKS", 2),
    ("cornifer-map", "HK_GEO_WALLET", 70), ("cornifer-map", "HK_MAP_OPENS", 1),
    ("cornifer-map", "HK_MAP_ROOMS_DRAWN", 16), ("cornifer-map", "HK_REGION_ID", 410),
    ("cornifer-map", "HK_DEATHS", 0), ("cornifer-map", "HK_SAVE_LOADED", 1),
    // Vengeful Spirit earned in play. mound-spell boots from
    // tools/cards/mound-spell.mcd (the same record seeded in the Ancestral Mound
    // beside the Snail Shaman), hears the meeting, waits out the summon, jumps
    // into the orb, and after the black, the get-item message and the fade
    // gets up and walks off. The spell, 99 SOUL (100 capped by the vessel) and
    // the source's own SaveGame at the wake point are what it pins.
    ("mound-spell", "HK_SPELL_EARNED", 1), ("mound-spell", "HK_SHAMAN_STATE", 2),
    ("mound-spell", "HK_SHAMAN_TALKS", 1), ("mound-spell", "HK_SOUL", 99),
    ("mound-spell", "HK_SAVE_WRITES", 1), ("mound-spell", "HK_SAVE_ERRORS", 0),
    ("mound-spell", "HK_DEATHS", 0), ("mound-spell", "HK_SAVE_LOADED", 1),
    // The Greenpath exit with the earned spell and no cheats. spell-exit boots
    // from tools/cards/spell-exit.mcd: the record the Snail Shaman's SaveGame
    // wrote in a mound-spell run (fireballLevel 1), reseated right of
    // Crossroads_11_alt's Elder Baldur (tools/seed_card.py). SOUL starts at 0,
    // so the four casts come from the four rollers the Baldur spits once the
    // spell is had: three nail hits each for 33 SOUL, then a cast, the last
    // after a step left under a goop. Then the Knight walks on into region 338.
    ("spell-exit", "HK_BLOCKERS_DEAD", 1), ("spell-exit", "HK_ENEMY_HITS", 4),
    ("spell-exit", "HK_ROLLERS_SPAWNED", 4), ("spell-exit", "HK_ROLLERS_KILLED", 4),
    ("spell-exit", "HK_ROLLER_HITS", 12), ("spell-exit", "HK_CHEATS", 0),
    ("spell-exit", "HK_DEATHS", 0), ("spell-exit", "HK_REGION_ID", 338),
    ("spell-exit", "HK_SAVE_LOADED", 1),
    // Secrets (docs/SECRETS.md). Each secret-* route boots seated beside one
    // hidden wall or cracked floor and nails it to breaking with the source
    // hit count. Crossroads_37 has no route: that room overruns the input
    // queue on this build and the base alike (docs/SECRETS.md).
    ("secret-kp", "HK_SECRET_HITS", 3), ("secret-kp", "HK_SECRET_BREAKS", 1), ("secret-kp", "HK_SECRET_REFUSED", 0),
    ("secret-kp", "HK_DEATHS", 0), ("secret-kp", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c07", "HK_SECRET_HITS", 4), ("secret-c07", "HK_SECRET_BREAKS", 1), ("secret-c07", "HK_SECRET_REFUSED", 0),
    ("secret-c07", "HK_DEATHS", 0), ("secret-c07", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c03", "HK_SECRET_HITS", 4), ("secret-c03", "HK_SECRET_BREAKS", 1), ("secret-c03", "HK_SECRET_REFUSED", 0),
    ("secret-c03", "HK_DEATHS", 0), ("secret-c03", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c04", "HK_SECRET_HITS", 3), ("secret-c04", "HK_SECRET_BREAKS", 1), ("secret-c04", "HK_SECRET_REFUSED", 0),
    ("secret-c04", "HK_DEATHS", 0), ("secret-c04", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c08", "HK_SECRET_HITS", 4), ("secret-c08", "HK_SECRET_BREAKS", 1), ("secret-c08", "HK_SECRET_REFUSED", 0),
    ("secret-c08", "HK_DEATHS", 0), ("secret-c08", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c09", "HK_SECRET_HITS", 3), ("secret-c09", "HK_SECRET_BREAKS", 1), ("secret-c09", "HK_SECRET_REFUSED", 0),
    ("secret-c09", "HK_DEATHS", 0), ("secret-c09", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c10", "HK_SECRET_HITS", 4), ("secret-c10", "HK_SECRET_BREAKS", 1), ("secret-c10", "HK_SECRET_REFUSED", 0),
    ("secret-c10", "HK_DEATHS", 0), ("secret-c10", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c13", "HK_SECRET_HITS", 3), ("secret-c13", "HK_SECRET_BREAKS", 1), ("secret-c13", "HK_SECRET_REFUSED", 0),
    ("secret-c13", "HK_DEATHS", 0), ("secret-c13", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c18", "HK_SECRET_HITS", 4), ("secret-c18", "HK_SECRET_BREAKS", 1), ("secret-c18", "HK_SECRET_REFUSED", 0),
    ("secret-c18", "HK_DEATHS", 0), ("secret-c18", "HK_SCENE_SFX_MISSED", 0),
    ("secret-c21", "HK_SECRET_HITS", 4), ("secret-c21", "HK_SECRET_BREAKS", 1), ("secret-c21", "HK_SECRET_REFUSED", 0),
    ("secret-c21", "HK_DEATHS", 0), ("secret-c21", "HK_SCENE_SFX_MISSED", 0),
    ("secret-f08a", "HK_SECRET_HITS", 3), ("secret-f08a", "HK_SECRET_BREAKS", 1), ("secret-f08a", "HK_SECRET_REFUSED", 0),
    ("secret-f08a", "HK_DEATHS", 0), ("secret-f08a", "HK_SCENE_SFX_MISSED", 0),
    ("secret-f08b", "HK_SECRET_HITS", 3), ("secret-f08b", "HK_SECRET_BREAKS", 1), ("secret-f08b", "HK_SECRET_REFUSED", 0),
    ("secret-f08b", "HK_DEATHS", 0), ("secret-f08b", "HK_SCENE_SFX_MISSED", 0),
    // The -reload routes boot a card that already holds the broken wall and
    // walk through where it stood: no hits, and the Knight ends past it.
    ("secret-c03-reload", "HK_SECRET_HITS", 0), ("secret-c03-reload", "HK_SECRET_BREAKS", 0),
    // 643737 before the pad-fix SDK pin and the analog request at boot; 634672 on every
    // run since. The cause is not isolated between the two.
    ("secret-c03-reload", "HK_PLAYER_X", 634672), ("secret-c03-reload", "HK_REGION_ID", 159),
    ("secret-c10-reload", "HK_SECRET_HITS", 0), ("secret-c10-reload", "HK_SECRET_BREAKS", 0),
    ("secret-c10-reload", "HK_PLAYER_X", 3162112), ("secret-c10-reload", "HK_REGION_ID", 245),
    ("secret-c18-reload", "HK_SECRET_HITS", 0), ("secret-c18-reload", "HK_SECRET_BREAKS", 0),
    ("secret-c18-reload", "HK_PLAYER_X", 1589248), ("secret-c18-reload", "HK_REGION_ID", 298),
    // Crossroads_46 (Ancestral Mound): walking in fades the three egg-room masks.
    ("secret-c46-eggs", "HK_REVEAL_MASKS_HIDDEN", 3), ("secret-c46-eggs", "HK_REVEAL_MASKS_VISIBLE", 0),
    ("secret-c46-eggs", "HK_DEATHS", 0),
    // Crossroads_37's floor, nailed early (polls 60, 96, 132) so its hit and
    // break particles land while the room's thirteen Husks are awake. This
    // tape panicked the guest at poll 159 ("input sampler contract",
    // QueueFull): standing still there the simulation fell behind the pad by
    // a tick every 10 to 30 polls, because every Walker swept all its view's
    // edges with the exact i64 ray test twice a tick. replay_cue fails any
    // route whose simulation skips a tick or drops a sample.
    ("secret-c37", "HK_SECRET_HITS", 3), ("secret-c37", "HK_SECRET_BREAKS", 1), ("secret-c37", "HK_SECRET_REFUSED", 0),
    ("secret-c37", "HK_DEATHS", 0), ("secret-c37", "HK_SCENE_SFX_MISSED", 0), ("secret-c37", "HK_REGION_ID", 454),
    // Standing still in Crossroads_37 for 3,000 polls, the case that panicked
    // at poll 125 on the ship6 disc.
    ("c37-stand", "HK_DEATHS", 0), ("c37-stand", "HK_REGION_ID", 454), ("c37-stand", "HK_SECRET_HITS", 0),
];
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
/// Frames a route may still show late art for (`HK_MODULE_ART_LATE`). Both deaths
/// leave a Shade whose art package is not resident at the respawn for 44 frames:
/// pop-in, which the rule forbids, listed so a new case fails instead.
const ART_LATE_ALLOWED: &[(&str, u64)] = &[("kings-death", 44), ("boss-death", 44)];
/// Frames over two vblanks a route may still have (see `pacing`): the 2026-10-08
/// measurement (hkperf sweep and this replay, the larger of the two) plus 30%,
/// four frames for the counts that matter and one for a single stray, since frame
/// pacing moves by tens of frames between builds with no relevant change. This is
/// the debt against the 30 fps rule and only ever goes down; a route not listed
/// has none to spare, and no boss route has more than a view bind's one frame.
const PACING_CEILINGS: &[(&str, u64)] = &[
    ("boss-fight", 2), ("boss-wave", 2),
    ("cornifer-map", 1), ("focus", 1), ("cheat-spell", 1), ("cheat-dash", 1), ("cheat-wings", 1), ("ctrl-wings", 1), ("cheat-heart", 1), ("c37-stand", 6), ("journey-false-knight", 475), ("journey-reload", 327), ("journey-kings", 301), ("kings-climb", 264), ("journey-crossroads", 233),
    ("ctrl-climb", 202), ("f17-charger", 154), ("secret-f08a", 154), ("secret-c03", 151), ("crossroads-gate", 137),
    ("kings-return", 136), ("kp-playtest", 131), ("cheat-dream", 124), ("kings-death", 123), ("mound-spell", 118),
    ("secret-c03-reload", 115), ("gruz-fight", 108), ("secret-kp", 73), ("f01-moss", 66), ("greenpath-walk", 64), ("well-drop", 60),
    ("secret-c37", 55), ("secret-c04", 28), ("baldur-spell", 25), ("secret-c13", 25), ("secret-c07", 24), ("secret-c08", 24),
    ("secret-c10", 23), ("secret-c21", 23), ("spell-exit", 20), ("secret-c18", 13), ("c37-stand", 4), ("grub-jar", 3),
    ("husk-guard", 3), ("secret-f08b", 3), ("bench-save", 2), ("mawlek-fight", 2), ("secret-c09", 2), ("secret-c10-reload", 2),
    ("secret-c18-reload", 2), ("secret-c46-eggs", 2),
];
const REQUIRED_MIN: &[(&str, &str, u64)] = &[
    // Survived the traversal at all. HK_DEATHS 0 above is the real assertion;
    // this only says the Knight was not left on one mask by something new.
    ("kings-return", "HK_HEALTH", 2),
    // An ability clip owned the Knight's body. The count is frame timing; that
    // it is not zero is the ability having run, and each control route with the
    // grant removed is pinned at exactly 0 in REQUIRED.
    // Brooding Mawlek's arms swung, one swing met the nail, the Head spat.
    ("mawlek-fight", "HK_MW_SWIPES", 1), ("mawlek-fight", "HK_MW_PARRIES", 1),
    ("mawlek-fight", "HK_MW_HEAD_SHOTS", 1),
    ("cheat-spell", "HK_ABILITY_DRAWN", 20),
    ("cheat-heart", "HK_ABILITY_DRAWN", 50),
    ("cheat-dream", "HK_ABILITY_DRAWN", 400),
    ("cheat-wings", "HK_ABILITY_DRAWN", 5),
    ("cheat-dash", "HK_ABILITY_DRAWN", 5),
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
    ("boss-death", "boss-fight.mcd"), ("boss-wave", "boss-fight.mcd"), ("greenpath-walk", "greenpath.mcd"),
    ("grub-jar", "grub-jar.mcd"), ("baldur-spell", "baldur-spell.mcd"), ("door-jiji", "door-jiji.mcd"),
    ("husk-guard", "husk-guard.mcd"),
    ("cornifer-map", "cornifer-map.mcd"), ("mound-spell", "mound-spell.mcd"), ("spell-exit", "spell-exit.mcd"),
    ("mawlek-fight", "mawlek.mcd"), ("gruz-fight", "gruz-fight.mcd"),
    ("secret-kp", "secret-kp.mcd"), ("secret-c07", "secret-c07.mcd"), ("secret-c03", "secret-c03.mcd"),
    ("secret-c04", "secret-c04.mcd"), ("secret-c08", "secret-c08.mcd"), ("secret-c09", "secret-c09.mcd"),
    ("secret-c10", "secret-c10.mcd"), ("secret-c13", "secret-c13.mcd"), ("secret-c18", "secret-c18.mcd"),
    ("secret-c21", "secret-c21.mcd"), ("secret-f08a", "secret-f08a.mcd"), ("secret-f08b", "secret-f08b.mcd"),
    ("secret-c03-reload", "secret-c03-reload.mcd"), ("secret-c10-reload", "secret-c10-reload.mcd"), ("secret-c18-reload", "secret-c18-reload.mcd"),
    ("secret-c46-eggs", "secret-c46-eggs.mcd"), ("secret-c37", "secret-c37.mcd"), ("c37-stand", "secret-c37.mcd")];
/// The journey's segments, in order. Each boots from the card the one before
/// it wrote, and the first from no card at all; `validate` runs them one after
/// another beside the independent ROUTES.
const JOURNEY: &[&str] = &["journey-kings", "journey-crossroads", "journey-false-knight", "journey-reload"];
const ROUTES: &[(&str, bool)] = &[("focus", false), ("well-drop", true), ("town-reset2", true),
    ("crossroads-gate", true), ("town-continue", false), ("town-shade", false), ("kings-death", false),
    ("kings-return", false), ("bench-save", false), ("town-elderbug", false),
    ("cheat-spell", false), ("cheat-dash", false), ("cheat-wings", false), ("ctrl-wings", false),
    ("cheat-heart", false), ("cheat-dream", false), ("kings-climb", false), ("ctrl-climb", false),
    ("town-shop", false), ("boss-fight", false), ("boss-death", false), ("boss-wave", false),
    ("greenpath-walk", true), ("grub-jar", false), ("baldur-spell", false),
    ("door-jiji", false), ("husk-guard", false), ("cornifer-map", false), ("mound-spell", false),
    ("spell-exit", false), ("mawlek-fight", false), ("gruz-fight", false),
    ("secret-kp", false), ("secret-c07", false), ("secret-c03", false), ("secret-c04", false),
    ("secret-c08", false), ("secret-c09", false), ("secret-c10", false), ("secret-c13", false),
    ("secret-c18", false), ("secret-c21", false), ("secret-f08a", false), ("secret-f08b", false),
    ("secret-c03-reload", false), ("secret-c10-reload", false), ("secret-c18-reload", false), ("secret-c46-eggs", false),
    ("secret-c37", false), ("c37-stand", false),
    // Manny's King's Pass playtest of 2026-10-03 walks back and forth across
    // the bind between views 2 and 12, where scenery went dark.
    ("kp-playtest", false)];

/// Tapes `pgo` profiles when none is given: the King's Pass climb and the
/// Crossroads gate walk. With every delay-slot filler search on, this recipe
/// took the ordinary build from 20.80 to 22.74 fps on the first and from 19.57
/// to 20.79 on the second, counting gameplay frames only (boot and scene-gate
/// loads excluded), and left 89,440 bytes free below the stack reservation.
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
    /// Replays run at once by validate (default: every route).
    jobs: Option<usize>,
    /// Delete each passing route's replay dumps once it is judged, keeping its record.
    prune: bool,
    /// `disc`: the guest build directory to pack (default build/), the library
    /// to put the pair in (default the disc library) and Red Book tracks to
    /// add, for comparing against an older disc.
    work: Option<PathBuf>,
    library: Option<PathBuf>,
    cdda: Vec<PathBuf>,
}

/// The repository root: this package lives in host/hk-build.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
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
    run_python(root, &all)
}

/// Run the venv Python, again if it dies of SIGSEGV. UnityPy's native texture
/// and audio decoders crash now and then on inputs they read fine the next
/// time (seen in ability_art.py and cook_audio.py on unchanged inputs); a
/// rerun rewrites whatever the crashed run left behind.
fn run_python(root: &Path, args: &[&str]) -> Result<()> {
    use std::os::unix::process::ExitStatusExt;
    let program = venv_python(root);
    for attempt in 1..=3 {
        println!("+ {} {}", program.display(), args.join(" "));
        let status = Command::new(&program).args(args).current_dir(root).status()?;
        if status.success() {
            return Ok(());
        }
        if status.signal() != Some(11) || attempt == 3 {
            return Err(format!("{} failed with {status}", program.display()).into());
        }
        println!("  SIGSEGV (attempt {attempt} of 3), running it again");
    }
    unreachable!()
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
            && root.join("data/menu.rs").is_file()
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
/// Every file under `dir`, recursively.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

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
    // The Rust cookers and the readers they share (host/hk-cook, hk-unity,
    // hk-pil, hk-dotnet) decide asset outputs too.
    for dir in ["host/hk-cook/src", "host/hk-unity/src", "host/hk-unity/data", "host/hk-pil/src", "host/hk-dotnet/src"] {
        collect_files(&root.join(dir), &mut files)?;
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
    for state in ["packed-scenes.json", "scene-certificates.json", "ambience.json", "xa-music.json", "area-music.json", "focus-audio.json",
        "geo-provenance.json", "geo-audio-provenance.json", "lifeblood-provenance.json", "hud-provenance.json",
        "audio-provenance.json", "great-door.json", "battle-gates.json", "scene-sfx.json", "break-effects/report.json", "opaque-tiles/report.json",
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
        match script.strip_prefix("rust:") {
            Some(tool) => rust_step(root, tool)?,
            None => python_step(root, &format!("host/{script}"), &[])?,
        }
    }
    run("cargo", &["run", "--quiet", "--manifest-path", "shared/hk-format/Cargo.toml", "--example", "check", "--", "data/room.hk"], root)?;
    python_step_module(root, "build_guest", "prepass")?;
    // Certificates bind to the final deduplicated bank geometry and palette
    // words, so they follow the pack. Tiles before groups (groups read the
    // tile report), both before the per-scene bundles.
    for tool in ["opaque-tiles", "opaque-groups", "scene-certificates"] {
        rust_step(root, tool)?;
    }
    let outputs = asset_outputs(root)?;
    std::fs::write(&cache, serde_json::to_string_pretty(&serde_json::json!({"key": key, "outputs": outputs}))?)?;
    Ok(false)
}

/// Where the finished disc goes: the user's disc library, which a build replaces
/// only once the new pair is complete.
fn disc_library() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Downloads").join("ps1 games")
}

/// Pack the disc from the guest build in `work` (its patched EXE, `modules.json`
/// and the cooked chunks): WORLD.PAK in read order and the XA song file.
fn package_disc(root: &Path, work: &Path, library: &Path) -> Result<()> {
    let songs = disc::Songs { xa: vec![root.join("data/music.xa")], cdda: Vec::new() };
    disc::package(root, &work.join("hk-psx-patched.exe"), work, library, &songs)
}

/// Run one of the cookers already ported to Rust (host/hk-cook), in-process.
fn rust_step(root: &Path, tool: &str) -> Result<()> {
    println!("+ hk-cook {tool}");
    let start = std::time::Instant::now();
    match tool {
        "props" => hk_cook::props::main(root, None)?,
        "break-effects" => hk_cook::break_effects::main(root, None)?,
        "scene-sfx" => hk_cook::scene_sfx::main(root, None)?,
        "xa-music" => hk_cook::xa_music::main(root, None)?,
        "opaque-tiles" => hk_cook::opaque_tiles::cook(root, 2, false)?,
        "opaque-groups" => hk_cook::opaque_groups::main(root)?,
        "scene-certificates" => hk_cook::scene_certificates::main(root, &[])?,
        other => return Err(format!("no Rust cooker named {other}").into()),
    }
    println!("  hk-cook {tool}: {:.1?}", start.elapsed());
    Ok(())
}

/// Run one function of a host module in the project venv.
fn python_step_module(root: &Path, module: &str, function: &str) -> Result<()> {
    let code = format!("import sys;sys.path.insert(0,'host');import {module};{module}.{function}()");
    run_python(root, &["-c", &code])
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
    // Hydrate the pinned SDK before either suite: both compile guest modules
    // against its crates, so after a pin bump they would fail on a missing
    // .psoxide rather than on anything in this checkout.
    sdk(root, &sdk_source(root, options))?;
    // The XA encoder is exported from the same checkout (hk-cook xa_music).
    std::env::set_var("HK_SDK_SOURCE", sdk_source(root, options));
    cook_cached(root, options.recook)?;
    // The asset cookers (charms, items, shops, NPCs, scripts) read the admitted
    // scene list from .hkpsx/selected-regions.json, which the prepass used to
    // write only after them: a fresh clone had none, and every other build fed
    // them the previous cook's list. It is a copy of this cook's report.
    std::fs::copy(root.join("data/regions.json"), root.join(".hkpsx/selected-regions.json"))?;
    menu_cached(root, options.recook)?;
    assets_cached(root, options.recook)?;
    // The boot art chunk and its offsets (data/boot_art.rs) are generated data
    // the guest-compile tests include. build_guest.py writes them too, but only
    // after the suites below, so a checkout that had never built since the
    // boot art landed failed its host tests. Packing four cooked files is
    // cheap, and the asset cache does not track these outputs, so write them on
    // every build rather than behind that cache.
    python_step_module(root, "build_guest", "boot_art")?;
    // Likewise the room-module tables (data/modules.rs, data/props_art.rs),
    // which build_guest.py otherwise writes only when it links the guest.
    python_step_module(root, "build_guest", "module_tables")?;
    // The host suite is cheap and nothing else ran it, so a cooker regression
    // used to survive whole builds: adding a required constant to
    // `generated_params` broke a fixture and nine builds shipped before anyone
    // noticed. It runs after the cook because its fixtures read what the cook
    // generates (data/*.rs, the audio banks, data/regions.json), which a fresh
    // clone does not have; on a warm checkout the cook is a cache hit, so this
    // still runs before anything slow. HK_PRE_BUILD skips what can only be
    // checked against this build's own report (docs/BUDGET.md), which runs
    // after build_report below.
    let status = Command::new(venv_python(root))
        .args(["-m", "unittest", "discover", "-s", "tests", "-q"])
        .env("HK_PRE_BUILD", "1")
        .current_dir(root)
        .status()?;
    if !status.success() {
        return Err(format!("the host test suite failed with {status}").into());
    }
    // And the simulation's own suite, for the same reason: the charm board
    // landed with four hk-sim test targets failing to compile, and nothing here
    // ran them, so it shipped. These path-include the guest modules (and so
    // their generated data), so they are the only check that the guest's own
    // logic still holds before its link.
    run("cargo", &["test", "-q", "--manifest-path", "shared/hk-sim/Cargo.toml"], root)?;
    let telemetry: &[&str] = if options.telemetry { &["--telemetry"] } else { &[] };
    // assets_cached has run the prepass or proven its outputs current either
    // way, so the guest build never repeats it (it ran pack_scenes a second
    // time on every build that recooked assets).
    let mut guest_args = telemetry.to_vec();
    guest_args.push("--no-prepass");
    if pgo {
        profile_guided_guest(root, options, &guest_args)?;
    } else {
        python_step(root, "host/build_guest.py", &guest_args)?;
        package_disc(root, &root.join("build"), &disc_library())?;
    }
    let mut report_args = telemetry.to_vec();
    if pgo {
        // Its own report file: docs/BUDGET.md pins the ordinary build's figures.
        report_args.push("--pgo");
    }
    python_step(root, "host/build_report.py", &report_args)?;
    check_hot_order(root, !pgo && !options.telemetry)?;
    if options.validate {
        validate(root, options)?;
    }
    if !pgo {
        // docs/BUDGET.md pins the ordinary build's figures, so it is checked
        // against the report this build wrote, after the replays, so a stale
        // table never hides their verdict.
        run(venv_python(root), &["-m", "unittest", "tests.test_budget_doc"], root)?;
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
    package_disc(root, &collect, &collect.join("disc"))?;
    let mut args = telemetry.to_vec();
    args.extend(["--no-prepass", "--profile", "elf", "--work", elf.to_str().unwrap()]);
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
    python_step(root, "host/build_guest.py", &args)?;
    package_disc(root, &root.join("build"), &disc_library())
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

/// The emulator the route replays run on: --frontend or HK_PSX_FRONTEND when
/// given, else the one pinned in emulator.lock.json, the same `pgo` uses.
/// Validation used to take whatever ../PSoXide-emulator last built, so a
/// verdict depended on the sibling checkout's state rather than this one's.
fn frontend(root: &Path, options: &Options) -> Result<PathBuf> {
    let given = options.frontend.clone().or_else(|| std::env::var_os("HK_PSX_FRONTEND").map(PathBuf::from));
    let frontend = match given {
        Some(path) => path,
        None => pinned_frontend(root, options)?.0,
    };
    if !frontend.is_file() {
        return Err(format!("emulator frontend not found at {} (pass --frontend)", frontend.display()).into());
    }
    Ok(frontend)
}

/// Replay every route tape against the final CUE; a fault or an incomplete tape fails the build.
/// The replay of one route tape, against `card` when the route boots with one.
fn replay_command(root: &Path, frontend: &Path, out: &Path, name: &str, card: Option<PathBuf>) -> Command {
    let tape = root.join(format!("tools/tapes/{name}.pxtape"));
    let mut args = vec![root.join("tools/replay_cue.py"), "--tape".into(), tape, "--output".into(), out.join(name),
        "--screenshot-interval".into(), "400".into(), "--frontend".into(), frontend.to_path_buf()];
    if let Some(card) = card {
        args.extend(["--memcard".into(), card]);
    }
    let mut command = Command::new(venv_python(root));
    command.args(args).current_dir(root).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    command
}

/// The renderer's I-cache placement (game/build.rs, game/hot-text-order.txt)
/// took effect: every listed symbol's section is in the link map, at
/// ascending addresses in list order, with nothing else between them. A
/// renamed or inlined function would otherwise drop out of the list silently
/// and the layout would drift again. The names are mangled, so they carry the
/// crate's hash (`Cs..._6hk_psx`), which changes with the features and the
/// dependency graph: an ordinary build fails and prints the list under the
/// hash it found, a telemetry or pgo build only warns.
fn check_hot_order(root: &Path, strict: bool) -> Result<()> {
    let order = std::fs::read_to_string(root.join("game/hot-text-order.txt"))?;
    let wanted: Vec<&str> = order.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let build: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build.json"))?)?;
    let map = build["link_map"]["path"].as_str().ok_or("build.json has no link map")?;
    // LLD input-section rows: VMA LMA Size Align <object>:(<section>).
    let mut sections: Vec<(u32, u32, String)> = Vec::new();
    for line in std::fs::read_to_string(map)?.lines() {
        let Some(start) = line.find(":(.text.") else { continue };
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        let (Ok(address), Ok(size)) = (u32::from_str_radix(fields[0], 16), u32::from_str_radix(fields[2], 16)) else { continue };
        let name = line[start + 8..].trim_end_matches(')').to_string();
        sections.push((address, size, name));
    }
    sections.sort();
    let crate_hash = |name: &str| name.find("_6hk_psx").and_then(|end| name[..end].rfind("Cs").map(|start| name[start..end].to_string()));
    let missing: Vec<&str> = wanted.iter().copied().filter(|w| !sections.iter().any(|s| s.2 == *w)).collect();
    if !missing.is_empty() {
        let found = sections.iter().find_map(|s| crate_hash(&s.2));
        let listed = crate_hash(wanted[0]);
        let mut message = format!("hot-text-order.txt: {} of {} functions are not in the link ({})", missing.len(), wanted.len(), missing[0]);
        if let (Some(found), Some(listed)) = (found, listed) {
            if found != listed {
                let list: Vec<String> = wanted.iter().map(|w| w.replace(&listed, &found)).collect();
                message += &format!("; the crate hash is {found}, the list has {listed}. The list under this hash:\n{}", list.join("\n"));
            }
        }
        if strict {
            return Err(message.into());
        }
        println!("warning: {message}");
        return Ok(());
    }
    let mut previous: Option<usize> = None;
    for symbol in &wanted {
        let at = sections.iter().position(|s| s.2 == *symbol).unwrap_or(0);
        if let Some(p) = previous {
            if at != p + 1 {
                return Err(format!("hot-text-order.txt: {symbol} is not placed right after {}", sections[p].2).into());
            }
        }
        previous = Some(at);
    }
    let first = sections.iter().find(|s| s.2 == wanted[0]).map(|s| s.0).unwrap_or(0);
    println!("Hot text order holds: {} functions from {first:#010x}", wanted.len());
    Ok(())
}

/// One `#[no_mangle]` word of a replay's final RAM (`<output>/ram.bin`), at its
/// address in the build's link map (.hkpsx/build.json `link_map`), for
/// symbols tools/replay_cue.py does not watch.
fn ram_word(root: &Path, output: &Path, symbol: &str) -> Result<u32> {
    let build: Value = serde_json::from_slice(&std::fs::read(root.join(".hkpsx/build.json"))?)?;
    let map = build["link_map"]["path"].as_str().ok_or("build.json has no link map")?;
    // LLD map rows: VMA LMA Size Align Name.
    let address = std::fs::read_to_string(map)?
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.len() == 5 && fields[4] == symbol).then(|| u32::from_str_radix(fields[0], 16).ok()).flatten()
        })
        .ok_or_else(|| format!("{symbol} is not in {map}"))?;
    let ram = std::fs::read(output.join("ram.bin"))?;
    let at = (address & 0x1f_ffff) as usize;
    let bytes = ram.get(at..at + 4).ok_or("ram.bin is shorter than the symbol's address")?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

/// Frame pacing of one replay: gameplay frames (the run of vblanks between two
/// display flips) over the 30 fps bar of two vblanks. Only gameplay ticks count:
/// `HK_GAME_MODE` at its most common value and `HK_ROOM_LOAD_STATE` at its
/// most common (resident) value, so the title screen, loads and gates stay
/// out (the same rule as tools/frame-pacing). Returns (frames, over two
/// vblanks, longest frame in vblanks, longest run of consecutive over frames).
fn pacing(output: &Path) -> Result<(u64, u64, u64, u64)> {
    let command: Value = serde_json::from_slice(&std::fs::read(output.join("command.json"))?)?;
    let column = |name: &str| command["watches"][name].as_str().map(|a| format!("ram_{:0>8}", a.trim_start_matches("0x")));
    let text = std::fs::read_to_string(output.join("route.csv"))?;
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().ok_or("route.csv is empty")?.split(',').collect();
    let at = |name: &str| header.iter().position(|h| *h == name);
    let flip = at("display_start_changed").ok_or("route.csv has no display_start_changed")?;
    let (game, load) = (column("HK_GAME_MODE").and_then(|c| at(&c)), column("HK_ROOM_LOAD_STATE").and_then(|c| at(&c)));
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split(',').collect()).collect();
    let mode = |index: Option<usize>| -> Option<&str> {
        let index = index?;
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for r in &rows { *counts.entry(r[index]).or_default() += 1; }
        counts.into_iter().max_by_key(|&(_, n)| n).map(|(v, _)| v)
    };
    let (play, ready) = (mode(game), mode(load));
    let gameplay = |r: &Vec<&str>| game.is_none_or(|g| Some(r[g]) == play) && load.is_none_or(|l| Some(r[l]) == ready);
    let (mut frames, mut over, mut longest, mut streak, mut worst_streak, mut since) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    for r in &rows {
        if !gameplay(r) { since = 0; continue; }
        since += 1;
        if r[flip] == "1" {
            if since > 0 {
                frames += 1;
                longest = longest.max(since);
                if since > 2 { over += 1; streak += 1; worst_streak = worst_streak.max(streak); } else { streak = 0; }
            }
            since = 0;
        }
    }
    Ok((frames, over, longest, worst_streak))
}

/// Replay every route tape against the final CUE; a fault or an incomplete tape fails the build.
fn validate(root: &Path, options: &Options) -> Result<()> {
    let frontend = frontend(root, options)?;
    let out = root.join(".hkpsx/validate");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out)?;
    // Replays are independent deterministic emulator runs: by default spawn
    // them all at once and collect in route order, so the wall time is the
    // longest route. `--jobs N` runs N at a time and `--prune` deletes each
    // passing route's dumps once it is judged (keeping its command.json), for a disk that cannot hold
    // every route's dumps at once (about 50 MB each).
    let jobs = options.jobs.unwrap_or(ROUTES.len()).max(1);
    let spawn = |name: &str| {
        let card = MEMCARDS.iter().find(|(route, _)| *route == name).map(|(_, card)| {
            // The replay writes the card back: run against a scratch copy so the fixture stays put.
            let scratch = out.join(format!("{name}.mcd"));
            let _ = std::fs::copy(root.join("tools/cards").join(card), &scratch);
            scratch
        });
        replay_command(root, &frontend, &out, name, card).spawn()
    };
    // The journey is one save carried through power cycles, so its segments
    // run in order beside the independent routes: the first boots with no card
    // at all and every later one boots from a copy of the card its predecessor
    // wrote. A segment whose predecessor failed still runs, against whatever
    // that predecessor left, and fails on its own pins.
    let journey = {
        let (root, frontend, out) = (root.to_path_buf(), frontend.clone(), out.clone());
        std::thread::spawn(move || {
            let mut results = Vec::new();
            for (index, name) in JOURNEY.iter().enumerate() {
                let card = out.join(format!("{name}.mcd"));
                if index > 0 {
                    let _ = std::fs::copy(out.join(format!("{}.mcd", JOURNEY[index - 1])), &card);
                }
                results.push((*name, replay_command(&root, &frontend, &out, name, Some(card)).output()));
            }
            results
        })
    };
    let mut summary = serde_json::Map::new();
    let mut failed = Vec::new();
    let mut evaluate = |name: &'static str, gates: bool, replay: std::io::Result<std::process::Output>| -> Result<()> {
        let output = out.join(name);
        let replay = replay?;
        let text = String::from_utf8_lossy(&replay.stdout).to_string();
        std::fs::write(out.join(format!("{name}.log")), format!("{text}{}", String::from_utf8_lossy(&replay.stderr)))?;
        let verdict: Value = text.find('{').and_then(|start| serde_json::from_str(&text[start..]).ok()).unwrap_or(Value::Null);
        let mut ok = replay.status.success() && verdict["completed"] == true;
        if ok && gates {
            let check = Command::new(venv_python(root))
                .args([root.join("tools/validate_scene_gates.py").to_str().unwrap(), output.to_str().unwrap()])
                .current_dir(root)
                .output()?;
            std::fs::write(out.join(format!("{name}-gates.log")), [check.stdout.as_slice(), check.stderr.as_slice()].concat())?;
            ok = check.status.success();
        }
        let ram = &verdict["final_ram"];
        // Every route: no scenery was drawn with another draw's vignette word
        // (render::HK_VIGNETTE_STALE). A view bind used to leave the old view's
        // words in place for up to sixteen frames, so scenery went dark or
        // vanished at each bind (Manny's King's Pass tape, 2026-10-03).
        match ram_word(root, &output, "HK_VIGNETTE_STALE") {
            Ok(0) => {}
            Ok(stale) => {
                println!("{name}: HK_VIGNETTE_STALE={stale} expected 0");
                ok = false;
            }
            Err(e) => {
                println!("{name}: HK_VIGNETTE_STALE unreadable: {e}");
                ok = false;
            }
        }
        // Every route: the camera never sat outside the cooked range of the view
        // being drawn (render::HK_CAMERA_VIEW_MISS), and no prop's art arrived
        // after its first visible frame (modules::HK_MODULE_ART_LATE). Either
        // one is scenery that vanishes or pops in while walking (Manny's King's
        // Pass tape, 2026-10-03).
        for symbol in ["HK_CAMERA_VIEW_MISS", "HK_MODULE_ART_LATE"] {
            match ram_word(root, &output, symbol) {
                Ok(0) => {}
                Ok(count) if symbol == "HK_MODULE_ART_LATE" && ART_LATE_ALLOWED.iter().any(|(route, most)| *route == name && count as u64 <= *most) => {}
                Ok(count) => {
                    println!("{name}: {symbol}={count} expected 0");
                    ok = false;
                }
                Err(e) => {
                    println!("{name}: {symbol} unreadable: {e}");
                    ok = false;
                }
            }
        }
        for (route, key, expected) in REQUIRED {
            if *route == name && ram[*key].as_u64() != Some(*expected) {
                println!("{name}: {key}={} expected {expected}", ram[*key]);
                ok = false;
            }
        }
        for (route, key, least) in REQUIRED_MIN {
            if *route == name && ram[*key].as_u64().is_none_or(|v| v < *least) {
                println!("{name}: {key}={} expected at least {least}", ram[*key]);
                ok = false;
            }
        }
        // The 30 fps bar: no gameplay frame over two vblanks. A route may not
        // exceed its ceiling in PACING_CEILINGS (zero when unlisted); the list
        // is the debt that remains and only ever goes down.
        match pacing(&output) {
            Ok((frames, over, longest, streak)) => {
                let ceiling = PACING_CEILINGS.iter().find(|(route, _)| *route == name).map_or(0, |c| c.1);
                println!("{name}: pacing {over} of {frames} frames over 2 vblanks (ceiling {ceiling}), longest {longest}, worst streak {streak}");
                if over > ceiling {
                    ok = false;
                }
            }
            Err(e) => {
                println!("{name}: pacing unreadable: {e}");
                ok = false;
            }
        }
        println!(
            "{} {name}: completed={} faults={} x={} y={} region={} scene_loads={} gate_loads={}",
            if ok { "PASS" } else { "FAIL" }, verdict["completed"], verdict["faults"], ram["HK_PLAYER_X"], ram["HK_PLAYER_Y"],
            ram["HK_REGION_ID"], ram["HK_SCENE_LOADS"], ram["HK_SCENE_GATE_LOADS"]
        );
        if !ok {
            failed.push(name);
        } else if options.prune {
            // The dumps and captures go; the replay's record stays, which
            // the power-cut check reads (bench-save/command.json).
            for entry in std::fs::read_dir(&output)? {
                let path = entry?.path();
                let keep = path.file_name().is_some_and(|n| ["command.json", "replay.log", "frontend-help.txt"].iter().any(|k| n == *k));
                if keep {
                    continue;
                }
                if path.is_dir() { std::fs::remove_dir_all(&path)? } else { std::fs::remove_file(&path)? }
            }
        }
        summary.insert(name.to_string(), serde_json::json!({"pass": ok, "verdict": verdict}));
        Ok(())
    };
    for chunk in ROUTES.chunks(jobs) {
        let children: Vec<_> = chunk.iter().map(|(name, _)| spawn(name)).collect();
        for ((name, gates), child) in chunk.iter().zip(children) {
            evaluate(name, *gates, child.and_then(|c| c.wait_with_output()))?;
        }
    }
    for (name, output) in journey.join().map_err(|_| "the journey replay thread panicked")? {
        evaluate(name, true, output)?;
    }
    let bin = root.join("dist/hk-psx.exe");
    summary.insert("exe_sha256".into(), Value::String(if bin.is_file() { sha256_file(&bin)? } else { String::new() }));
    std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&Value::Object(summary))?)?;
    if failed.is_empty() {
        println!("All {} routes pass on the final CUE.", ROUTES.len() + JOURNEY.len());
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
        "usage: cargo hk-build <build|pgo|validate|disc|help> [--recook] [--telemetry] [--no-validate]\n       \
         [--hollow-knight DIR] [--sdk-source DIR] [--emulator-source DIR] [--frontend PATH] [--tape PATH]... [--jobs N] [--prune]\n\
         build     cook (cached), build the guest, replace the sole hk-psx.bin/.cue, report, then validate\n\
         pgo       build, with the guest optimised by a profile of --tape replays (default: kings-climb\n          \
         and crossroads-gate), replayed on the emulator pinned in emulator.lock.json (built once\n          \
         from --emulator-source, default ../PSoXide-emulator); needs a checkout path without spaces\n\
         validate  replay the route tapes in tools/tapes against the current CUE, on the pinned\n          \
         emulator unless --frontend (or HK_PSX_FRONTEND) names another\n\
         disc      pack the disc from the guest build in --work (default build/) into --library\n          \
         (default the disc library): WORLD.PAK and the XA songs, plus any --cdda-track"
    );
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_else(|| "build".into());
    let mut options = Options {
        hollow_knight: None, sdk_source: None, emulator_source: None, frontend: None, telemetry: false, recook: false, validate: true, tapes: Vec::new(), jobs: None, prune: false, work: None, library: None, cdda: Vec::new(),
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
            "--jobs" => options.jobs = Some(args.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| usage())),
            "--prune" => options.prune = true,
            "--work" => options.work = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--library" => options.library = Some(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            "--cdda-track" => options.cdda.push(PathBuf::from(args.next().unwrap_or_else(|| usage()))),
            _ => usage(),
        }
    }
    let root = root();
    let result = match action.as_str() {
        "build" => build(&root, &options, false),
        "pgo" => build(&root, &options, true),
        "validate" => validate(&root, &options),
        "disc" => {
            let work = options.work.clone().unwrap_or_else(|| root.join("build"));
            let library = options.library.clone().unwrap_or_else(disc_library);
            let songs = disc::Songs { xa: vec![root.join("data/music.xa")], cdda: options.cdda.clone() };
            disc::package(&root, &work.join("hk-psx-patched.exe"), &work, &library, &songs)
        }
        _ => usage(),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
