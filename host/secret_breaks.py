"""Hidden walls and cracked floors as multi-hit secret breakables.

host/breakables.py recognizes the two FSM-authored families and pins their
definitions by digest; this module turns a recognized instance into what the
port runs. Every number below is a literal of one of those pinned definitions,
so the digest is what keeps it true; the reference for each is the state that
authors it.

A secret is a breakable with a hit counter. It shares the Breakable state space
(`scene * 128 + state`, `docs/STATE_IDENTITY.md`) so the world's broken bitmap,
its terrain exclusion and the `Kind::Breakable` save item carry it unchanged;
its state indices are taken from the top of the scene's 128 downwards, clear of
the C# Breakable ordinals, which count up from zero.

Families:

  WALL        `breakable_wall_v2`. Nail hits (attackType 0) count down `Hits`
              in `Idle` only; a hit locks the wall out for `Hit X` + `Return X`
              (0.1 s + 0.1 s) while it recoils 0.1 units along its `Facing` and
              back. A spell (attackType 2) goes to `Spell Destroy` at once.
              `Break` hides the sprite, drops the collider, shakes the camera
              (`AverageShake`) and broadcasts UNCOVER to the wall's subtree.
  WALL_TK2D   `Break Wall 2`. The same counter and lockout, drawn by a
              tk2dSprite; `Crumble` is eight fully transparent frames, so the
              art is gone on the break frame. UNCOVER goes to the one object
              `Mask Name` names.
  FLOOR       `break_floor` b74c4cdb. `ReceivedDamage` from a nail swing
              (attackType 0) while the hero body is inside the `Hero Range`
              child's trigger; three hits, 0.25 s apart. Hits 1 and 2 sag the two
              plank sprites (Translate and Rotate in Self space) and send
              `EnemyKillShake`; the third breaks and sends `AverageShake`.
  FLOOR_OPEN  `break_floor` 72a2d729 (Crossroads_09, the Mawlek wall): any
              `Nail Attack` trigger enter, no range and no attack-type gate.
"""
import math
from pathlib import Path
import breakables
from breakables import _components, _descendants, collider_polygons

FAMILY_WALL = 1
FAMILY_WALL_TK2D = 2
FAMILY_FLOOR = 3
FAMILY_FLOOR_OPEN = 4

# State indices come from the top of the scene's Breakable range.
TOP_STATE = breakables.MAX_SCENE_BREAKABLES - 1

# breakable_wall_v2 / Break Wall 2 `Hit X` + `Return X`: two `Wait 0.1`.
WALL_LOCKOUT_TICKS = 12
# `SetVelocity2d` 1.0 units/s for each `Wait 0.1`: 0.1 units out and back.
WALL_RECOIL_TICKS = 6
WALL_RECOIL_UNITS = 0.1
# `IntSwitch Facing`: 0 RIGHT, 1 UP, 2 LEFT, 3 DOWN, and the direction the
# wall's kinematic body first moves in each (`Recoil Speed Neg` is -1). `Hit Up`
# only waits; Break Wall 2's `Return Down` repeats the push (Idle snaps back).
WALL_RECOIL = {0: (-1, 0), 1: (0, 0), 2: (1, 0), 3: (0, 1)}
# break_floor `Hit 1` / `Hit 2` end in `Wait 0.25`.
FLOOR_LOCKOUT_TICKS = 15
FLOOR_HITS = breakables.CRACKED_FLOOR_NAIL_HITS
# The sag, per hit, as the actions run: (child, Translate y, Rotate z degrees).
# `Hit 2` translates Floor 2 once more after its audio.
FLOOR_SAG = (
    (('floor 1', -0.05, 0.0), ('floor 2', -0.10, 0.0), ('floor 1', 0.0, -2.5), ('floor 2', 0.0, 3.5)),
    (('floor 1', -0.10, 0.0), ('floor 2', -0.20, 0.0), ('floor 1', 0.0, -2.5), ('floor 2', 0.0, 2.5),
     ('floor 2', -0.15, 0.0)),
)
# `Initiate`'s FindChild names (Transform.Find: direct children only).
FLOOR_PARTS = ('floor 1', 'floor 2', 'wood small', 'wood large', 'Solid')
FLOOR_MASK_CHILD = 'msk_generic'
# `Strike Nail R` spawns at owner + (0, -2, 0) in world axes.
FLOOR_STRIKE_OFFSET = (0.0, -2.0)
# The `fade` FSM on Crossroads_09's `msk_generic`: HIT -> iTweenFadeTo 0.4 s.
FLOOR_MASK_FADE_SECONDS = 0.4

# Scene sound events (host/scene_sfx.py rows) each family plays.
SOUNDS = {
    FAMILY_WALL: {'hit': ('breakable_wall_hit_1', 'breakable_wall_hit_2'), 'break': ('breakable_wall_death', 'secret_discovered')},
    FAMILY_WALL_TK2D: {'hit': ('breakable_wall_hit_1', 'breakable_wall_hit_2'), 'break': ('breakable_wall_death', 'secret_discovered')},
    FAMILY_FLOOR: {'hit': ('barrel_death_1',), 'break': ('breakable_wall_death', 'barrel_death_1')},
    FAMILY_FLOOR_OPEN: {'hit': ('barrel_death_1',), 'break': ('breakable_wall_death', 'barrel_death_1')},
}


def _children(sc, gid):
    """Direct children by name, as Transform.Find sees them (first match)."""
    out = {}
    for child in sc.transforms[sc.go_transform[gid]]['m_Children']:
        kid = sc.transforms[child['m_PathID']]['m_GameObject']['m_PathID']
        out.setdefault(sc.gos[kid]['m_Name'], kid)
    return out


def _renderers(sc, gids):
    """Enabled SpriteRenderers with a sprite on active objects, as drawn."""
    out = []
    for gid in sorted(gids):
        if not sc.active(gid):
            continue
        for index, kind, tree in _components(sc, gid):
            if kind == 'SpriteRenderer' and tree['m_Enabled'] and tree['m_Sprite']['m_PathID']:
                out.append(sc.sid(index))
    return out


def _solid_colliders(sc, gids):
    """Enabled solid terrain colliders (layer 8), keyed as the cook keys edges."""
    out = []
    for gid in sorted(gids):
        if not sc.active(gid) or sc.gos[gid]['m_Layer'] != breakables.TERRAIN_LAYER:
            continue
        for index, kind, tree in _components(sc, gid):
            if kind.endswith('Collider2D') and tree['m_Enabled'] and not tree['m_IsTrigger']:
                out.append(sc.sid(index))
    return out


def _camera_locks(sc, gids):
    return [sc.sid(index) for gid in sorted(gids) if sc.active(gid)
            for index, kind, _ in _components(sc, gid) if kind == 'CameraLockArea']


def _box(points):
    return [min(p[0] for p in points), min(p[1] for p in points),
            max(p[0] for p in points), max(p[1] for p in points)]


# ------------------------------------------------------------------ transforms

def _quat_mul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (aw*bx + ax*bw + ay*bz - az*by, aw*by - ax*bz + ay*bw + az*bx,
            aw*bz + ax*by - ay*bx + az*bw, aw*bw - ax*bx - ay*by - az*bz)


def _quat_rotate(q, v):
    x, y, z, w = q
    vx, vy, vz = v
    # v' = q v q^-1, expanded.
    tx, ty, tz = 2*(y*vz - z*vy), 2*(z*vx - x*vz), 2*(x*vy - y*vx)
    return (vx + w*tx + y*tz - z*ty, vy + w*ty + z*tx - x*tz, vz + w*tz + x*ty - y*tx)


def _euler_z(degrees):
    a = math.radians(degrees) / 2
    return (0.0, 0.0, math.sin(a), math.cos(a))


def _quat(t):
    q = t['m_LocalRotation']
    return (q['x'], q['y'], q['z'], q['w'])


def _world_rotation(sc, tid):
    """The rotation-only chain Transform.rotation returns (scale ignored)."""
    q = (0.0, 0.0, 0.0, 1.0)
    chain = []
    while tid:
        chain.append(tid)
        tid = sc.transforms[tid]['m_Father']['m_PathID']
    for t in reversed(chain):
        q = _quat_mul(q, _quat(sc.transforms[t]))
    return q


def _matrix(position, rotation, scale):
    x, y, z, w = rotation
    r = [[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)], [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
         [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]]
    return [[r[i][0]*scale[0], r[i][1]*scale[1], r[i][2]*scale[2], position[i]] for i in range(3)] + [[0, 0, 0, 1]]


def _mul(a, b):
    return [[sum(a[i][k]*b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def _inverse_point(m, p):
    """Solve m * (x, y, z, 1) = p for the 3x3-invertible affine m."""
    a = [row[:3] for row in m[:3]]
    b = [p[i] - m[i][3] for i in range(3)]
    det = (a[0][0]*(a[1][1]*a[2][2]-a[1][2]*a[2][1]) - a[0][1]*(a[1][0]*a[2][2]-a[1][2]*a[2][0])
           + a[0][2]*(a[1][0]*a[2][1]-a[1][1]*a[2][0]))
    if abs(det) < 1e-12:
        raise ValueError('singular floor part transform')
    def col(j):
        c = [row[:] for row in a]
        for i in range(3):
            c[i][j] = b[i]
        return (c[0][0]*(c[1][1]*c[2][2]-c[1][2]*c[2][1]) - c[0][1]*(c[1][0]*c[2][2]-c[1][2]*c[2][0])
                + c[0][2]*(c[1][0]*c[2][1]-c[1][1]*c[2][0]))
    return [col(j)/det for j in range(3)]


def _sprite_box(source, sc, renderer):
    from cook import native_sprite_geometry
    o = source.ref(sc.file, renderer['m_Sprite'])
    _, (x0, y0, x1, y1) = native_sprite_geometry(o)
    if renderer['m_FlipX']:
        x0, x1 = -x0, -x1
    if renderer['m_FlipY']:
        y0, y1 = -y0, -y1
    return x0, y0, x1, y1


def floor_sag(source, sc, parts):
    """World quads of each sagging plank after hit 1 and after hit 2.

    Replays `Hit 1` and `Hit 2` on a copy of each plank's local transform:
    `Translate(v, Space.Self)` moves the world position by the rotation-only
    chain applied to v (scale is ignored, which is why the doubly negative
    Fungus1_08 floor sags away from its Hero Range), and `Rotate(e, Self)` is
    `localRotation *= Euler(e)`. Returns {child: [stage1 quad, stage2 quad]},
    each quad in the cook's corner order (x0,y1), (x1,y1), (x0,y0), (x1,y0)
    with only x and y, and the renderer source it moves.
    """
    state = {}
    for name in ('floor 1', 'floor 2'):
        gid = parts.get(name)
        if gid is None:
            raise ValueError(f'cracked floor has no {name!r} child')
        renderer = [(i, t) for i, k, t in _components(sc, gid) if k == 'SpriteRenderer' and t['m_Enabled']]
        if len(renderer) != 1:
            raise ValueError(f'{name!r} carries {len(renderer)} sprite renderers, not one')
        tid = sc.go_transform[gid]
        t = sc.transforms[tid]
        father = t['m_Father']['m_PathID']
        p = t['m_LocalPosition']
        s = t['m_LocalScale']
        state[name] = {'tid': tid, 'parent': sc.world(father) if father else _matrix((0, 0, 0), (0, 0, 0, 1), (1, 1, 1)),
                       'parent_rotation': _world_rotation(sc, father) if father else (0.0, 0.0, 0.0, 1.0),
                       'position': [p['x'], p['y'], p['z']], 'rotation': _quat(t), 'scale': (s['x'], s['y'], s['z']),
                       'renderer': sc.sid(renderer[0][0]), 'box': _sprite_box(source, sc, renderer[0][1])}
    stages = {name: [] for name in state}
    for ops in FLOOR_SAG:
        for name, dy, angle in ops:
            part = state[name]
            if dy:
                world_q = _quat_mul(part['parent_rotation'], part['rotation'])
                delta = _quat_rotate(world_q, (0.0, dy, 0.0))
                world = [sum(part['parent'][i][j]*v for j, v in enumerate(part['position'] + [1])) for i in range(3)]
                world = [world[i] + delta[i] for i in range(3)]
                part['position'] = _inverse_point(part['parent'], world)
            if angle:
                part['rotation'] = _quat_mul(part['rotation'], _euler_z(angle))
        for name, part in state.items():
            m = _mul(part['parent'], _matrix(part['position'], part['rotation'], part['scale']))
            x0, y0, x1, y1 = part['box']
            quad = [[m[0][0]*x + m[0][1]*y + m[0][3], m[1][0]*x + m[1][1]*y + m[1][3]]
                    for x, y in ((x0, y1), (x1, y1), (x0, y0), (x1, y0))]
            stages[name].append(quad)
    return {name: {'renderer': state[name]['renderer'], 'stages': stages[name]} for name in state}


# --------------------------------------------------------------------- records

def _wall(source, sc, record):
    gid = record['gid']
    family = FAMILY_WALL if record['definition'] == 'breakable_wall_v2' else FAMILY_WALL_TK2D
    subtree = _descendants(sc, gid)
    colliders = [record['collider_source']]
    locks = []
    if family == FAMILY_WALL:
        # `Pause Frame` and `Destroy` take `Camera Locks` away: Crossroads_21's
        # holds a solid terrain collider over the top of the wall.
        camera = _children(sc, gid).get('Camera Locks')
        if camera is not None:
            under = _descendants(sc, camera)
            colliders += _solid_colliders(sc, under)
            locks += _camera_locks(sc, under)
    return {'family': family, 'hits': record['nail_hits'], 'facing': record['facing'],
            'spell': True, 'hero_range': None, 'lockout_ticks': WALL_LOCKOUT_TICKS,
            'off_renderer_sources': [record['renderer_source']],
            'moving': [{'renderer': record['renderer_source'], 'recoil': list(WALL_RECOIL[record['facing']])}],
            'recoil_ticks': WALL_RECOIL_TICKS, 'recoil_units': WALL_RECOIL_UNITS,
            'collider_sources': colliders, 'camera_lock_sources': locks,
            'strike_origin': list(sc.point(gid)[:2]),
            'uncovers': [u['game_object'] for u in record['uncovers']],
            'renderer_type': record['renderer_type'], 'renderer_source': record['renderer_source'],
            'subtree': sorted(sc.sid(g) for g in subtree)}


def _floor(source, sc, record):
    gid = record['gid']
    family = FAMILY_FLOOR if record['hit_gate'] == 'Hero Range and attack type' else FAMILY_FLOOR_OPEN
    parts = _children(sc, gid)
    missing = [name for name in FLOOR_PARTS if name not in parts]
    if missing:
        raise ValueError(f'cracked floor lacks {missing}')
    # Everything `Break` switches off, children included (Crossroads_13's wall
    # sprites under Solid, Fungus1_08's shaft masks under floor 2), goes with it.
    hidden = set()
    for name in FLOOR_PARTS:
        hidden |= _descendants(sc, parts[name])
    hero_range = None
    if family == FAMILY_FLOOR:
        detector = _children(sc, gid).get('Hero Range')
        if detector is None:
            raise ValueError('cracked floor has no Hero Range child')
        boxes = [(i, k, t) for i, k, t in _components(sc, detector) if k == 'BoxCollider2D' and t['m_IsTrigger']]
        if len(boxes) != 1:
            raise ValueError('Hero Range needs exactly one trigger box')
        points = [p for poly in collider_polygons(sc, detector, 'BoxCollider2D', boxes[0][2]) for p in poly]
        hero_range = _box(points)
    sag = floor_sag(source, sc, parts)
    masks = []
    mask = parts.get(FLOOR_MASK_CHILD)
    if mask is not None and sc.active(mask):
        masks.append(sc.sid(mask))
    origin = sc.point(gid)
    return {'family': family, 'hits': FLOOR_HITS, 'facing': 0, 'spell': False, 'hero_range': hero_range,
            'lockout_ticks': FLOOR_LOCKOUT_TICKS,
            'off_renderer_sources': _renderers(sc, hidden),
            'moving': [{'renderer': sag[name]['renderer'], 'stages': sag[name]['stages']} for name in ('floor 1', 'floor 2')],
            'collider_sources': list(record['solid_collider_sources']),
            'camera_lock_sources': _camera_locks(sc, hidden),
            'strike_origin': [origin[0] + FLOOR_STRIKE_OFFSET[0], origin[1] + FLOOR_STRIKE_OFFSET[1]],
            'uncovers': masks, 'mask_fade_seconds': FLOOR_MASK_FADE_SECONDS if masks else None,
            'subtree': sorted(sc.sid(g) for g in _descendants(sc, gid))}


def secret_states(sc):
    """FSM source -> state index for every hidden wall and cracked floor.

    Decided by structure alone (the pinned shape, an enabled FSM on an active
    object), in source id order from the top of the scene's range down, so a
    secret's state and so its save item do not move when another one is
    refused, and host/reveal_masks.py can name a driver without recognizing
    the whole secret.
    """
    if hasattr(sc, '_secret_states'):
        return sc._secret_states
    found = []
    for index, (typ, tree) in sorted(sc.objects.items()):
        if typ != 'PlayMakerFSM' or not tree['m_Enabled']:
            continue
        gid = tree['m_GameObject']['m_PathID']
        if gid not in sc.gos or not sc.active(gid):
            continue
        try:
            if breakables.hidden_wall_shape(tree['fsm']) or breakables.cracked_floor_shape(tree['fsm']):
                found.append(sc.sid(index))
        except ValueError:
            continue
    breakable_count = len([o for o in sc.file.objects.values()
                           if o.type.name == 'MonoBehaviour' and sc.source.typename(o) == 'Breakable'])
    if breakable_count + len(found) > breakables.MAX_SCENE_BREAKABLES:
        raise ValueError(f'secret states collide with Breakable ordinals: {breakable_count} + {len(found)}')
    sc._secret_states = {sid: TOP_STATE - k for k, sid in enumerate(found)}
    return sc._secret_states


def secret_drivers(sc):
    """Mask GameObject -> the state of the secret whose break uncovers it.

    A hidden wall's UNCOVER reaches its subtree or its named mask
    (breakables.uncover_drivers); a cracked floor's `Break` sends HIT to its
    direct `msk_generic` child, which only Crossroads_09's has.
    """
    states = secret_states(sc)
    out = {}
    for target, driver in breakables.uncover_drivers(sc).items():
        if driver['source'] in states:
            out[target] = states[driver['source']]
    for index, (typ, tree) in sorted(sc.objects.items()):
        if typ != 'PlayMakerFSM' or sc.sid(index) not in states:
            continue
        try:
            if not breakables.cracked_floor_shape(tree['fsm']):
                continue
        except ValueError:
            continue
        mask = _children(sc, tree['m_GameObject']['m_PathID']).get(FLOOR_MASK_CHILD)
        if mask is not None and sc.active(mask):
            out[mask] = states[sc.sid(index)]
    return out


def secret_sources(source, sc, bounds=None, errors=None):
    """Every hidden wall and cracked floor of the scene, as a secret record.

    `bounds` keeps the ones whose hit box or hero range touches a region, the
    way breakable_sources filters. State indices are assigned over the whole
    scene first so every region numbers an instance the same.
    """
    found = []
    for recognize, build in ((breakables.hidden_walls, _wall), (breakables.cracked_floors, _floor)):
        refused = []
        for record in recognize(sc, errors=refused, contract=False):
            try:
                secret = build(source, sc, record)
            except ValueError as error:
                refused.append({'id': record['source'], 'type': 'secret', 'error': str(error)})
                continue
            persist = [{'source': sc.sid(i), 'semi_persistent': bool(t['semiPersistent']), 'dont_save': bool(t['dontSave'])}
                       for i, k, t in _components(sc, record['gid']) if k == 'PersistentBoolItem']
            secret.update(source=record['source'], game_object=record['game_object'], name=record['name'],
                          definition=record['definition'], fsm_sha256=record['fsm_sha256'],
                          hit_polygons=record['hit_polygons'], box=record['box'], persistence=persist)
            found.append(secret)
        if errors is not None:
            errors.extend(refused)
    states = secret_states(sc)
    for secret in found:
        secret['state_index'] = states[secret['source']]
    found.sort(key=lambda s: -s['state_index'])
    if bounds is None:
        return found
    def touches(box):
        return box and not (box[2] < bounds[0] or box[0] > bounds[2] or box[3] < bounds[1] or box[1] > bounds[3])
    return [s for s in found if touches(s['box']) or touches(s['hero_range'])]


def bind_secrets(records, draws, edges):
    """Region draw and edge indices for each secret, like bind_breakables."""
    draw_ids = {}
    for index, draw in enumerate(draws):
        draw_ids.setdefault(draw['source'], index)
    out = []
    for record in records:
        bound = dict(record)
        bound['off_draws'] = [draw_ids[s] for s in record['off_renderer_sources'] if s in draw_ids]
        bound['edge_indices'] = [i for i, edge in enumerate(edges) if edge['source'] in record['collider_sources']]
        bound['moving_draws'] = [dict(m, draw=draw_ids.get(m['renderer'])) for m in record['moving']]
        out.append(bound)
    return out


# ------------------------------------------------------------------ particles
#
# What each family plays, per stage of its FSM: 0 the break, 1 and 2 a floor's
# `Hit 1` and `Hit 2`, 3 a wall's every surviving hit. Entries are
# ('child', FindChild name, Emit count or None for Play), ('create', state)
# for the state's CreateObject prefabs and ('pool', state) for its
# SpawnObjectFromGlobalPool ones; only prefabs carrying a ParticleSystem
# count. A child a placement does not have is a null FsmGameObject in the
# source, so nothing plays (Crossroads_07's and _18's walls have no rocks).
FACING_STATES = ('Hit Right', 'Hit Up', 'Hit Left', 'Hit Down')


def emitter_plan(family, facing):
    hit = FACING_STATES[facing]
    if family == FAMILY_WALL:
        return {3: [('child', 'Particle_rocks_small', 5), ('pool', hit)],
                0: [('child', 'Particle_rocks_large', None), ('create', 'Break')]}
    if family == FAMILY_WALL_TK2D:
        return {3: [('child', 'Particle_rocks_small', 5), ('create', hit)],
                0: [('child', 'Dust Break 1', None), ('child', 'Dust Break 2', None),
                    ('child', 'Particle_rocks_large', None), ('create', 'Break')]}
    if family == FAMILY_FLOOR:
        return {stage: [('child', f'{name} {stage or 3}', None) for name in ('Dust Hit', 'Pt Bits', 'Pt Wood')]
                for stage in (1, 2, 0)}
    return {1: [('child', 'Dust Hit 1', None)], 2: [('child', 'Dust Hit 2', None)],
            0: [('child', 'Dust Hit 3', None), ('child', 'Dust Break 1', None), ('child', 'Dust Break 2', None)]}


def pool_spawn_prefabs(sc, gid, fsm, state_name):
    """SpawnObjectFromGlobalPool prefabs of one state that carry particles,
    with where they land, the way breakables._create_object_prefabs reads
    CreateObject."""
    out = []
    for state in fsm['states']:
        if state['name'] != state_name:
            continue
        data = state['actionData']
        for index, raw in enumerate(data['actionNames']):
            if not data['actionEnabled'][index] or not raw.endswith('.SpawnObjectFromGlobalPool'):
                continue
            slots = breakables._action_slots(data, index)
            if not {'gameObject', 'spawnPoint', 'position', 'rotation'} <= set(slots):
                raise ValueError('unsupported serialized SpawnObjectFromGlobalPool parameters')
            reference = data['fsmGameObjectParams'][data['paramDataPos'][slots['gameObject']]]
            if reference.get('useVariable'):
                continue
            found = breakables._prefab_particles(sc.source, sc.file, reference['value'])
            if found is None or not found['particles']:
                continue
            found['reference'] = reference['value']
            found['origin'], found['rotation'] = breakables._spawn_transform(sc, gid, fsm, data, slots)
            out.append(found)
    return out
