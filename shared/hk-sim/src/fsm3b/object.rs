//! Boss tier, family 8: objects and spawning. The objects are the tree's scene-object table.
use crate::fsm3::{r_stateless, Ctx, R_CONT, R_FIN};
use crate::fsm3_world::{flag, k, mul, World};
use hk_format::fsm3::{ops::object, ENT_CAMERA, ENT_HERO, NONE8};

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
pub fn entry(c: &mut Ctx<'_, '_>, op: u8) -> u16 {
    if op == object::GET_OWNER {
        // GetOwner [store]: the object the FSM is attached to.
        let owner = c.fsm_owner(c.f);
        c.set_obj(0, owner);
    } else if op == object::GET_PARENT {
        // GetParent [gameObject, store]
        let b = c.skip();
        let h = c.otarget();
        let p = c.world().get(h).map_or(NONE8, |e| e.parent);
        c.set_obj(b, p);
    } else if op == object::FIND_CHILD {
        // FindChild [gameObject, childName, store]
        let b = c.skip();
        let parent = c.otarget();
        let name = c.int(b) as u16;
        let child = c.world().find_child(parent, name);
        c.set_obj(b + 1, child);
    } else if op == object::FIND_GAME_OBJECT {
        // FindGameObject [objectName, withTag, store]: an object of the tree by name, or one of the outside objects the adapter listed.
        let name = c.int(0) as u16;
        let w = c.world();
        let mut found = NONE8;
        for i in 0..w.n {
            if w.ent[i as usize].name == name {
                found = i;
                break;
            }
        }
        for (id, h) in w.outside {
            if found == NONE8 && id == name {
                found = h;
            }
        }
        c.set_obj(2, found);
    } else if op == object::GET_TAG {
        // GetTag [gameObject, store]
        let t = c.obj(0);
        let tag = c.world().get(t).map_or(0, |e| e.tag as i32);
        c.set(1, tag);
    } else if op == object::ACTIVATE_GAME_OBJECT {
        // ActivateGameObject [gameObject, activate, recursive, resetOnExit]: the cook refuses a resetOnExit.
        let (t, on) = (c.otarget(), c.bit(c.skip()));
        let recursive = c.bit(c.skip() + 1);
        set_active(c, t, on, recursive);
    } else if op == object::ACTIVATE_ALL_CHILDREN {
        // ActivateAllChildren [gameObject, activate]
        let (t, on) = (c.otarget(), c.bit(c.skip()));
        let n = c.world().n;
        // An object the tree does not hold (a container of the scene) has no children here to switch.
        if c.world().get(t).is_some() {
            for i in 0..n {
                if c.world().ent[i as usize].parent == t {
                    set_active(c, i, on, false);
                }
            }
        }
    } else if op == object::DESTROY_SELF {
        // DestroySelf [detachChildren]: the object leaves the scene, its FSMs with it.
        let owner = c.fsm_owner(c.f);
        set_active(c, owner, false, true);
        c.world().request(k::DESTROY, owner, 0, [0; 3]);
        return R_FIN;
    } else if op == object::SET_PARENT {
        set_parent(c);
    } else if op == object::CREATE_OBJECT {
        // CreateObject [gameObject, spawnPoint, position, rotation, storeObject, networkInstantiate, networkGroup]
        let (prefab, point) = (c.obj(0), c.obj(1));
        let at = super::position_of(c, point);
        let (dx, dy) = (c.comp(2, 0), c.comp(2, 1));
        c.world().request(k::SPAWN, NONE8, 1, [prefab as i32, at[0] + dx, at[1] + dy]);
        let h = c.world().spawned;
        c.set_obj(4, h);
    } else if op == object::DESTROY_OBJECT {
        // DestroyObject [gameObject, delay, detachChildren]
        let t = c.obj(0);
        c.world().request(k::DESTROY, t, 1, [0; 3]);
        if c.world().get(t).is_some() {
            set_active(c, t, false, true);
        }
    } else if op == object::SPAWN_RANDOM_OBJECTS_V2 {
        // SpawnRandomObjectsV2 [gameObject, spawnPoint, position, spawnMin, spawnMax, speedMin, speedMax, angleMin, angleMax,
        // originVariationX, originVariationY]: a scatter of pool objects; the adapter draws their velocities.
        let (prefab, point) = (c.obj(0), c.obj(1));
        let at = super::position_of(c, point);
        let (lo, hi) = (c.int(3), c.int(4));
        let count = lo + ((c.rand() >> 8) as i32 % (hi - lo + 1).max(1));
        c.world().request(k::SPAWN_RANDOM, point, 0, [prefab as i32, count, at[0]]);
        let (speed_lo, a_lo) = (c.fixed(5), c.fixed(7));
        c.world().request(k::SPAWN_RANDOM, point, 1, [speed_lo, a_lo, at[1]]);
    } else if op == object::SPAWN_OBJECT_FROM_GLOBAL_POOL {
        // SpawnObjectFromGlobalPool [gameObject, spawnPoint, position, rotation, store]: pool object `prefab` at the spawn
        // point plus the offset (a vector at its first component).
        let (prefab, point) = (c.obj(0), c.obj(1));
        let at = super::position_of(c, point);
        let (dx, dy) = (c.comp(2, 0), c.comp(2, 1));
        c.world().request(k::SPAWN, NONE8, 0, [prefab as i32, at[0] + dx, at[1] + dy]);
        let h = c.world().spawned;
        c.set_obj(4, h);
    } else {
        return R_CONT;
    }
    r_stateless(c.flags)
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Set object `t` (and, recursive, every descendant) active or not, and switch the FSMs of everything whose hierarchy
/// state changed: a re-activated object's FSMs start over, a deactivated one's stop.
#[inline(never)]
pub fn set_active(c: &mut Ctx<'_, '_>, t: u8, on: bool, recursive: bool) {
    if c.world().get(t).is_none() {
        return;
    }
    #[cfg(feature = "trace")]
    crate::fsm3::trace::emit(crate::fsm3::trace::Ev::Activate { fsm: c.f, object: t, on });
    let n = c.world().n;
    if let Some(e) = c.world().get_mut(t) {
        e.set(flag::ACTIVE, on);
    }
    if recursive {
        for i in 0..n {
            if i != t && descends(c.world(), i, t) {
                c.world().ent[i as usize].set(flag::ACTIVE, on);
            }
        }
    }
    sync_activity(c);
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// Bring every FSM's on/off state in line with its object's hierarchy state.
#[inline(never)]
pub fn sync_activity(c: &mut Ctx<'_, '_>) {
    let fsms = c.t.bank.fsm_count();
    for g in 0..fsms {
        let owner = c.fsm_owner(g);
        let on = c.world().active_in_hierarchy(owner);
        c.t.set_active(g, on);
    }
}

#[cfg_attr(target_arch = "mips", optimize(size))]
#[inline(never)]
fn descends(w: &World, h: u8, top: u8) -> bool {
    let mut at = h;
    for _ in 0..6 {
        match w.get(at) {
            Some(e) => {
                if e.parent == top {
                    return true;
                }
                at = e.parent;
            }
            None => return false,
        }
    }
    false
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// SetParent [gameObject, parent, resetLocalPosition, resetLocalRotation]: the object keeps its place in the world unless
/// a reset says to take the parent's origin or rotation; a missing parent is the scene root.
#[inline(never)]
fn set_parent(c: &mut Ctx<'_, '_>) {
    let b = c.skip();
    let t = c.otarget();
    let parent = c.obj(b);
    let (reset_pos, reset_rot) = (c.bit(b + 1), c.bit(b + 2));
    let w = c.world();
    let (pos, scale, rot) = w.world_of(t);
    let new_parent = if parent == ENT_HERO || parent == ENT_CAMERA { NONE8 } else { parent };
    if let Some(e) = w.get_mut(t) {
        e.parent = new_parent;
    }
    let (ppos, pscale, prot) = if w.get(new_parent).is_some() { w.world_of(new_parent) } else { ([0; 3], [crate::ONE; 2], 0) };
    let local = w.local_for(t, pos);
    if let Some(e) = w.get_mut(t) {
        e.pos = if reset_pos { [0, 0, e.pos[2]] } else { local };
        e.scale = [crate::fsm3_world::div(scale[0], pscale[0]), crate::fsm3_world::div(scale[1], pscale[1])];
        e.rot = if reset_rot { 0 } else { rot.wrapping_sub(prot) };
    }
    let _ = (ppos, mul(0, 0));
    sync_activity(c);
}

#[cfg_attr(target_arch = "mips", optimize(size))]
/// `sync_activity` for a tree no pass is running: switch the FSMs of the objects that start inactive off.
#[inline(never)]
pub fn sync_tree(t: &mut crate::fsm3::Tree<'_>) {
    let fsms = t.bank.fsm_count();
    // SAFETY: the adapter that attached the tree set `env.world` to its own World for the length of the pass.
    let w = unsafe { &mut *t.env.world };
    for g in 0..fsms {
        let on = w.active_in_hierarchy(t.bank.owner(g));
        t.set_active(g, on);
    }
}
