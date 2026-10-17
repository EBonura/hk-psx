//! Boss tier, families 11 and 12: colliders and combat. An object's colliders are the adapter's; the table says whether
//! they are on, whether they are triggers, how hard they hurt the hero, and the box a `SetBoxCollider2DSizeVector` set.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use crate::fsm3_world::{flag, k};
use hk_format::fsm3::ops::{collider, combat};

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let t = c.otarget();
    if op == collider::SET_BOX_COLLIDER2_DSIZE_VECTOR {
        // SetBoxCollider2DSizeVector [gameObject, size, offset] (two 2-vectors)
        let b = c.skip();
        let (sx, sy, ox, oy) = (c.comp(b, 0), c.comp(b, 1), c.comp(b + 1, 0), c.comp(b + 1, 1));
        if let Some(e) = c.world().get_mut(t) {
            e.box_size = [sx, sy];
            e.box_offset = [ox, oy];
        }
    } else if op == collider::SET_BOX_COLLIDER_TRIGGER {
        // SetBoxColliderTrigger [gameObject, trigger]
        let on = c.bit(c.skip());
        if let Some(e) = c.world().get_mut(t) {
            e.set(flag::TRIGGER, on);
        }
    } else if op == collider::SET_COLLIDER {
        // SetCollider [gameObject, active]
        let on = c.bit(c.skip());
        if let Some(e) = c.world().get_mut(t) {
            e.set(flag::COLLIDER, on);
        }
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

/// Family 12.
#[inline(never)]
pub fn combat_entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    let t = c.otarget();
    if op == combat::SET_DAMAGE_HERO_AMOUNT {
        // SetDamageHeroAmount [gameObject, damageDealt]
        let n = c.int(c.skip());
        if let Some(e) = c.world().get_mut(t) {
            e.damage = n as u8;
        }
    } else if op == combat::SET_INVINCIBLE {
        // SetInvincible [gameObject, invincible, fromDirection]: the HealthManager is the adapter's.
        let b = c.skip();
        let (on, dir) = (c.bit(b) as u8, c.int(b + 1));
        c.world().request(k::INVINCIBLE, t, on, [dir, 0, 0]);
    } else if op == combat::SET_RECOIL_SPEED {
        // SetRecoilSpeed [gameObject, newRecoilSpeed]
        let s = c.fixed(c.skip());
        c.world().request(k::RECOIL, t, 0, [s, 0, 0]);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}
