//! Which enemies an arena brings and takes away, and what that does to the
//! guest's actor pool (host/battle.py is the Python twin).
//!
//! Crossroads_22 fights in waves of ordinary enemies. Its `Battle Control` FSM
//! activates the children of `Wave 1` .. `Wave 4` one wave at a time, each only
//! after the last wave's members are dead, and its `Remove on battle start` FSM
//! kills what stood in the room before (the world Hatcher and four Spitters).
//! No moment of the fight holds every placement at once, so the 32 actor slots
//! only have to hold the largest moment, not the sum.

use crate::common::{component_records, get, Result};
use hk_unity::scene::Scene;
use hk_unity::Value;

const BATTLE_CONTROL: &str = "Battle Control";
const REMOVE_ON_START: &str = "Remove on battle start";

/// (1 based wave or 0, killed when the battle starts) of one enemy.
pub type Member = (u8, bool);

pub(crate) fn has_fsm(records: &[(i64, &str, &Value)], name: &str) -> bool {
    records.iter().any(|r| {
        r.1 == "PlayMakerFSM"
            && r.2
                .get("fsm")
                .and_then(|f| f.get("name"))
                .and_then(Value::str)
                .as_deref()
                == Some(name)
    })
}

/// The states only Crossroads_22's four wave `Battle Control` has. Crossroads_08's
/// is a different, two wave FSM that this port does not drive, so its enemies
/// are not arena members and stand with the scene as they always have.
const WAVE_STATES: [&str; 9] = [
    "Wave 1",
    "Wave 2",
    "Wave 3",
    "Wave 4",
    "Pause W 1",
    "Pause W 2",
    "Pause W 3",
    "End Pause",
    "Blob Open",
];

/// `wave_arena`: whether these components carry the four wave `Battle Control`.
fn wave_arena(records: &[(i64, &str, &Value)]) -> bool {
    records
        .iter()
        .filter(|r| r.1 == "PlayMakerFSM")
        .filter_map(|r| r.2.get("fsm"))
        .filter(|f| f.get("name").and_then(Value::str).as_deref() == Some(BATTLE_CONTROL))
        .any(|f| {
            let names: Vec<String> = f
                .get("states")
                .and_then(Value::list)
                .unwrap_or(&[])
                .iter()
                .filter_map(|s| s.get("name").and_then(Value::str))
                .collect();
            WAVE_STATES.iter().all(|w| names.iter().any(|n| n == w))
        })
}

pub(crate) fn parent(sc: &Scene, gid: i64) -> Result<Option<i64>> {
    let tid = *sc.go_transform.get(&gid).ok_or("object has no transform")?;
    let father = get(
        get(sc.transform(tid).ok_or("transform missing")?, "m_Father")?,
        "m_PathID",
    )?
    .int()
    .unwrap_or(0);
    if father == 0 {
        return Ok(None);
    }
    let go = get(
        get(
            sc.transform(father).ok_or("parent transform missing")?,
            "m_GameObject",
        )?,
        "m_PathID",
    )?
    .int()
    .unwrap_or(0);
    Ok(Some(go))
}

pub(crate) fn name_of(sc: &Scene, gid: i64) -> Result<String> {
    Ok(get(sc.go(gid).ok_or("no such GameObject")?, "m_Name")?
        .str()
        .unwrap_or_default())
}

/// `Wave N` (the arena's wave group), as its number.
fn wave_number(name: &str) -> Option<u8> {
    name.strip_prefix("Wave ")?.parse().ok()
}

/// `membership`: the 1 based wave the enemy on `gid` is summoned in, or 0, and
/// whether `Remove on battle start` kills it when the fight begins. A Hatcher
/// Baby carries that FSM but ignores the event in `Inert`, so it is not removed.
pub fn membership(sc: &Scene, gid: i64, records: &[(i64, &str, &Value)]) -> Result<Member> {
    let removable =
        has_fsm(records, REMOVE_ON_START) && !name_of(sc, gid)?.starts_with("Hatcher Baby");
    let mut wave = 0;
    let mut at = parent(sc, gid)?;
    while let Some(group) = at {
        if let Some(n) = wave_number(&name_of(sc, group)?) {
            if let Some(above) = parent(sc, group)? {
                if wave_arena(&component_records(sc, above)) {
                    wave = n;
                    break;
                }
            }
        }
        at = parent(sc, group)?;
    }
    Ok((wave, removable))
}

/// `pool_peak`: slots the guest needs for `members`. Before the battle
/// everything outside a wave stands; once it starts the removable ones are gone
/// and one wave at a time stands in their place.
pub fn pool_peak(members: &[Member]) -> usize {
    let standing = members.iter().filter(|m| m.0 == 0).count();
    let kept = members.iter().filter(|m| m.0 == 0 && !m.1).count();
    let mut sizes = [0usize; 256];
    for m in members.iter().filter(|m| m.0 != 0) {
        sizes[m.0 as usize] += 1;
    }
    standing.max(kept + sizes.iter().copied().max().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_holds_the_largest_moment_not_the_sum() {
        // Crossroads_22: a cage of 23, the placed Hatcher and four Spitters that the
        // battle removes, then waves of 2, 3, 3 and 4.
        let mut members: Vec<Member> = vec![(0, false); 23];
        members.push((0, true));
        members.extend([(0, true); 4]);
        for (wave, size) in [(1, 2), (2, 3), (3, 3), (4, 4)] {
            members.extend(vec![(wave, false); size]);
        }
        assert_eq!(pool_peak(&members), 28);
        // Without the removal the same scene needs 23 + 5 + 4 at once.
        let kept: Vec<Member> = members.iter().map(|m| (m.0, false)).collect();
        assert_eq!(pool_peak(&kept), 32);
        assert_eq!(pool_peak(&[]), 0);
    }

    #[test]
    fn waves_are_named_by_number() {
        assert_eq!(wave_number("Wave 3"), Some(3));
        assert_eq!(wave_number("Wave"), None);
        assert_eq!(wave_number("Battle Scene"), None);
    }
}
