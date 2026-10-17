//! Boss tier, family 13: PlayerData. The cooker resolves field names to slots of the save record's reserve
//! (`Env::pd`), so a read or write is `[slot, ...]`; the first operand of the read forms is the (unused) owner object
//! when the action names one.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use crate::fsm3_world::k;
use hk_format::fsm3::ops::playerdata;

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == playerdata::GET_PLAYER_DATA_INT {
        // GetPlayerDataInt [gameObject?, field slot, store]
        let b = c.skip();
        let v = c.pd_read_at(b);
        c.set(b + 1, v);
    } else if op == playerdata::GET_PLAYER_DATA_BOOL {
        // GetPlayerDataBool [gameObject?, field slot, store]
        let b = c.skip();
        let v = c.pd_read_at(b) != 0;
        c.set_bit(b + 1, v);
    } else if op == playerdata::GGCHECK_IF_BOSS_SCENE {
        // GGCheckIfBossScene [bossSceneEvent, regularSceneEvent]: the base game has no Godhome scene.
        return crate::fsm3::r_with(crate::fsm3::r_stateless(c.flags) & crate::fsm3::R_FIN != 0, c.event(1));
    } else if op == playerdata::SET_PLAYER_DATA_BOOL {
        // SetPlayerDataBool [field slot, value]
        let v = c.bit(1) as i32;
        write(c, v);
    } else if op == playerdata::SET_PLAYER_DATA_INT {
        // SetPlayerDataInt [field slot, value]
        let v = c.int(1);
        write(c, v);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

/// Write the reserve and tell the engine: a few fields have a native side (disablePause, hornet1Defeated).
#[inline(never)]
fn write(c: &mut Ctx<'_, '_>, v: i32) {
    let slot = c.int(0);
    let e = c.env();
    if !e.pd.is_null() {
        // SAFETY: the actor points `pd` at the reserve and the cook only emits slots inside it.
        unsafe { *e.pd.add(slot as usize) = v };
        e.pd_writes = e.pd_writes.saturating_add(1);
    }
    c.world().request(k::PLAYERDATA, hk_format::fsm3::NONE8, 0, [slot, v, 0]);
}
