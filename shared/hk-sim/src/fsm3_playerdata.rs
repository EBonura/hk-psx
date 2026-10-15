//! HKFSM03 family 13: PlayerData and boss bookkeeping.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use hk_format::fsm3::ops::playerdata;

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == playerdata::SET_PLAYER_DATA_BOOL {
        // SetPlayerDataBool [field slot, value]
        let v = c.bit(1) as i32;
        write(c, v);
    } else if op == playerdata::SET_PLAYER_DATA_INT {
        // SetPlayerDataInt [field slot, value]
        let v = c.int(1);
        write(c, v);
    } else if op == playerdata::GET_PLAYER_DATA_BOOL {
        // GetPlayerDataBool [field slot, store]
        let v = c.pd_read() != 0;
        c.set_bit(1, v);
    } else if op == playerdata::GET_PLAYER_DATA_INT {
        // GetPlayerDataInt [field slot, store]
        let v = c.pd_read();
        c.set(1, v);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

#[inline(never)]
fn write(c: &mut Ctx<'_, '_>, v: i32) {
    let slot = c.int(0) as usize;
    let e = c.env();
    if !e.pd.is_null() {
        // SAFETY: the actor points `pd` at the reserve and the cook only emits slots inside it.
        unsafe { *e.pd.add(slot) = v };
        e.pd_writes = e.pd_writes.saturating_add(1);
    }
}
