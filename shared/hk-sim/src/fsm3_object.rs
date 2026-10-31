//! HKFSM03 family 8: objects and spawning.
use crate::fsm3::{r_stateless, Ctx, R_CONT};
use hk_format::fsm3::{ops::object, ENT_CAMERA, ENT_SELF};

#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == object::GET_OWNER {
        // GetOwner [store]: the actor itself.
        c.set_obj(0, ENT_SELF);
        r_stateless(c.flags)
    } else if op == object::FIND_GAME_OBJECT {
        // FindGameObject [objectName, withTag, store]: the cook admits only the camera query.
        c.set_obj(2, ENT_CAMERA);
        r_stateless(c.flags)
    } else if op == object::GET_PARENT {
        // GetParent [gameObject, store]: the parent of every child object this tier runs is the actor.
        c.set_obj(c.skip(), ENT_SELF);
        r_stateless(c.flags)
    } else if op == object::FIND_CHILD {
        // FindChild [gameObject, childName, store]: the tier keeps no child objects; what a child would do is a request on the actor.
        c.set_obj(c.skip() + 1, ENT_SELF);
        r_stateless(c.flags)
    } else {
        R_CONT
    }
}
