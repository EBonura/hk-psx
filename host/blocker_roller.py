"""The Elder Baldur's spat Roller, which `Attack Choose` can only pick once the
Knight has Vengeful Spirit.

`Blocker Control`'s `Attack Choose` is a 50/50 between GOOP and ROLLER, and
`Can Roller?` sends ROLLER back to GOOP while PlayerData `fireballLevel` is 0
or a `Spawn Roller v2(Clone)` is still alive. So the rollers exist for the
player who has the spell: 15 HP each, three nail hits, 33 SOUL, one more
cast. The Blocker's own 60 HP is four casts against a 99-SOUL vessel, and its
shell is invincible to the nail, so without them the Greenpath exit cannot be
opened without cheats.

The prefab is the `Roller` FSM of the Ancestral Mound's Baldurs
(shared/hk-sim/src/baldur.rs) with two differences this cook asserts: `Max
Speed` 14 rather than 11, and it starts in `Moving Right?` -> `In Air`, flung
by the Blocker's `Fire` with the goop's own launch and rolling the way the
Blocker faces on landing. Its clips are packed into the quick map's page by
host/game_map.py (`art`), so they cost no linked RAM and no animation slot.
"""
import math
from PIL import Image
from source import Source
from scene import Scene
from shops import Catalog
from cook import tk_sprite, FOCAL, CAM_Z
from focus import action_parameters, fsm_variables

SCENE = 'Crossroads_11_alt'
PREFAB = 'Spawn Roller v2'
CLIPS = ('Idle', 'Start', 'Roll', 'Stop')


def _prefab(source, sc):
    """The object `Roller` assigns to `Projectile` in the Blocker's FSM."""
    fsm = next(tree['fsm'] for _cid, (typ, tree) in sc.objects.items()
               if typ == 'PlayMakerFSM' and tree['fsm']['name'] == 'Blocker Control')
    state = next(st for st in fsm['states'] if st['name'] == 'Roller')
    data = state['actionData']
    index = next(i for i, n in enumerate(data['actionNames']) if n.endswith('SetGameObject') and data['actionEnabled'][i])
    start = data['actionStartIndex'][index]
    end = data['actionStartIndex'][index + 1] if index + 1 < len(data['actionNames']) else len(data['paramName'])
    refs = [data['fsmGameObjectParams'][data['paramDataPos'][k]] for k in range(start, end) if data['paramDataType'][k] == 19]
    literal = [r['value'] for r in refs if not r['useVariable'] and r['value']['m_PathID']]
    if len(literal) != 1:
        raise ValueError('the Blocker Roller state no longer assigns one prefab')
    obj = source.ref(sc.file, literal[0])
    go = source.read(obj)
    if go['m_Name'] != PREFAB:
        raise ValueError(f'the Blocker spits {go["m_Name"]}, not {PREFAB}')
    parts = {}
    for ref in go['m_Component']:
        component = source.ref(obj.assets_file, ref['component'])
        parts.setdefault(source.typename(component), (component, source.read(component)))
    roller = next(source.read(c) for c in (source.ref(obj.assets_file, r['component']) for r in go['m_Component'])
                  if source.typename(c) == 'PlayMakerFSM' and source.read(c)['fsm']['name'] == 'Roller')['fsm']
    variables = fsm_variables(roller)
    checks = {'Max Speed': 14.0, 'Acceleration': 0.45, 'Roll time Min': 2.0, 'Roll time Max': 3.0, 'Stop Time': 0.5}
    for name, want in checks.items():
        if abs(variables[name] - want) > 1e-4:
            raise ValueError(f'spawned Roller {name} is {variables[name]}, not {want}')
    if roller['startState'] != 'Initiate':
        raise ValueError('spawned Roller no longer starts in Initiate')
    hp = parts['HealthManager'][1]['hp']
    if hp != 15 or abs(parts['Rigidbody2D'][1]['m_GravityScale'] - 0.8) > 1e-6 or parts['DamageHero'][1]['damageDealt'] != 1:
        raise ValueError('spawned Roller body differs from the Mound Baldur contract')
    return obj, parts, hp


def art(source):
    """(images, boxes, clips, sources) for host/game_map.py to pack, in CLIPS order."""
    catalog = Catalog(source)
    sc = Scene(source, catalog.files[SCENE])
    obj, parts, hp = _prefab(source, sc)
    library_o = source.ref(obj.assets_file, parts['tk2dSpriteAnimator'][1]['library'])
    library = source.read(library_o)
    scale = FOCAL / -CAM_Z
    textures, images, boxes, clips = {}, [], [], []
    for name in CLIPS:
        clip = next(c for c in library['clips'] if c['name'] == name)
        start = len(images)
        for frame in clip['frames']:
            col = source.ref(library_o.assets_file, frame['spriteCollection'])
            image, box = tk_sprite(source, col.assets_file, source.read(col), frame['spriteId'], textures)
            dims = (max(1, math.ceil((box[2] - box[0]) * scale)), max(1, math.ceil((box[3] - box[1]) * scale)))
            images.append(image.resize(dims, Image.Resampling.LANCZOS))
            boxes.append(box)
        clips.append(dict(name=name, start=start, count=len(clip['frames']), fps=int(clip['fps']),
                          wrap=0 if clip['wrapMode'] == 0 else 2))
    return images, boxes, clips, {'prefab': source.sid(obj), 'library': source.sid(library_o), 'hp': hp}
