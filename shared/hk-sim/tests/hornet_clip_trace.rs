//! Which animated objects play which clips, tick by tick, in simulated fights (docs/BOSS-ART.md).
//!
//! The Knight walks up to the Encounter, then chases her and strikes whenever she is in reach, at a damage that stretches the
//! fight to a few minutes, over several seeds. The trace (one line per change of an object's clip or frozen frame) is written to
//! `$HK_TRACE_OUT` for `hk-cook boss-measure`; without the variable the test does nothing.
#![cfg(feature = "boss")]
use hk_sim::fsm3_world::{flag, NO_CLIP};
use hk_sim::hornet_fight::{Effect, Fight, Input, Phase, Strike};
use hk_sim::ONE;
use std::fmt::Write;

fn floor(_: usize) -> [i32; 4] {
    [0, 27 * ONE + 3000, 100 * ONE, 27 * ONE + 3000]
}

#[test]
fn trace_clips_over_simulated_fights() {
    let Ok(out) = std::env::var("HK_TRACE_OUT") else { return };
    let mut text = String::new();
    for seed in [3u32, 11, 29, 77] {
        let mut f = Box::new(Fight::blank());
        f.begin(seed, false, [0; 8]);
        let mut hero = [20 * ONE, 27 * ONE + 3000];
        let mut convo = false;
        let mut seen = vec![(u32::MAX, i16::MIN); 32];
        let mut fought = 0u32;
        for tick in 0..60 * 400u32 {
            let at = f.boss.world.world_of(0).0;
            // Walk in to the Encounter, then keep two units from her and strike at her height when close.
            let target = if tick < 900 { 27 * ONE } else { at[0] + if tick / 300 % 2 == 0 { -2 * ONE } else { 2 * ONE } };
            let step = ONE / 8;
            hero[0] += (target - hero[0]).clamp(-step, step);
            let near = (at[0] - hero[0]).abs() < 3 * ONE && tick > 900;
            let poly = [[at[0] - ONE, at[1] - 2 * ONE], [at[0] + ONE, at[1] - 2 * ONE], [at[0] + ONE, at[1] + ONE], [at[0] - ONE, at[1] + ONE]];
            let strike = (near && tick % 40 == 0).then_some(Strike { polygon: &poly, kind: 0, damage: 1, facing: 1 });
            let body = [hero[0] - ONE / 4, hero[1], hero[0] + ONE / 4, hero[1] + ONE + ONE / 2];
            let input = Input {
                hero,
                hero_body: body,
                camera: [hero[0], hero[1], -38 * ONE],
                dt_scaled: ONE,
                dt_real: ONE,
                strike,
                conversation_done: std::mem::take(&mut convo),
            };
            let mut effects = vec![];
            f.step(&input, 1, &floor, &mut |e| effects.push(e));
            convo |= effects.iter().any(|e| matches!(e, Effect::Conversation { .. }));
            if strike.is_some() {
                fought += 1;
            }
            let (tree, world) = match f.phase {
                Phase::Asleep | Phase::Fighting => (0u8, &f.boss.world),
                Phase::Dying => (1u8, f.corpse_world()),
                Phase::Gone => break,
            };
            if tree == 1 {
                // The corpse is a short fixed sequence: trace it once per seed.
                for i in 0..world.n {
                    let e = &world.ent[i as usize];
                    if e.on(flag::MESH) && e.clip != NO_CLIP && world.active_in_hierarchy(i) {
                        let slot = 16 + (i as usize % 16);
                        if seen[slot] != (e.clip, e.frame) {
                            seen[slot] = (e.clip, e.frame);
                            writeln!(text, "{seed}\t{tick}\t{tree}\t{i}\t{}\t{}\t{}\t{}", e.clip, e.frame, e.clip_from, f.health.hp).unwrap();
                        }
                    }
                }
                continue;
            }
            for i in 0..world.n.min(16) {
                let e = &world.ent[i as usize];
                let visible = e.on(flag::MESH) && e.clip != NO_CLIP && world.active_in_hierarchy(i);
                let key = if visible { (e.clip, e.frame) } else { (u32::MAX, i16::MIN) };
                if seen[i as usize] != key {
                    seen[i as usize] = key;
                    if visible {
                        writeln!(text, "{seed}\t{tick}\t{tree}\t{i}\t{}\t{}\t{}\t{}", e.clip, e.frame, e.clip_from, f.health.hp).unwrap();
                    }
                }
            }
            if f.phase == Phase::Dying && tick > 60 * 400 {
                break;
            }
        }
        writeln!(text, "# seed {seed} ticks struck {fought} hp {} phase {:?}", f.health.hp, f.phase as u8).unwrap();
    }
    std::fs::write(out, text).unwrap();
}
