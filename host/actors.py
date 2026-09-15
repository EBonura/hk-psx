"""Source HealthManager/DamageHero metadata and verified no-charm vital values.

Enemy movement remains unsupported when controlled by PlayMaker. Metadata never
turns an inactive enemy on or replaces its controller with a generic patrol.
"""
import hashlib
import math
import struct
from pathlib import Path
from combat import ticks


def _literal(instruction):
    name = instruction.opcode.name
    if name in ('ldc.r4', 'ldc.r8', 'ldc.i4', 'ldc.i4.s'):
        return instruction.operand
    if name == 'ldc.i4.m1':
        return -1
    if name.startswith('ldc.i4.') and name[-1].isdigit():
        return int(name[-1])
    return None


def source_vital_values(source, hero_constants):
    """Read the installed CIL initializers, never execute managed game code."""
    import dnfile
    from dncil.cil.body.reader import read_method_body_from_bytes
    assembly = source.directory / 'Managed/Assembly-CSharp.dll'
    pe = dnfile.dnPE(str(assembly))
    methods = {}
    wanted = {('PlayerData', 'SetupNewPlayerData'), ('HeroController', '.ctor'),
              ('HeroController', 'SoulGain'), ('HealthManager', 'NonFatalHit')}
    for typ in pe.net.mdtables.TypeDef.rows:
        for method_ref in typ.MethodList:
            method = method_ref.row
            key = (str(typ.TypeName), str(method.Name))
            if key in wanted:
                methods[key] = read_method_body_from_bytes(pe.get_data(method.Rva, 100000)).instructions
    if set(methods) != wanted:
        raise ValueError('installed CIL vital methods missing')

    def assignment(key, field):
        instructions = methods[key]
        found = []
        for i, instruction in enumerate(instructions):
            if instruction.opcode.name != 'stfld' or not i:
                continue
            token = instruction.operand
            row = pe.net.mdtables.tables[token.table].rows[token.rid - 1]
            if str(row.Name) == field:
                value = _literal(instructions[i - 1])
                if value is not None:
                    found.append(value)
        if len(found) != 1:
            raise ValueError(f'expected one literal assignment for {key}/{field}, got {found}')
        return found[0]

    setup = ('PlayerData', 'SetupNewPlayerData')
    values = {key: assignment(setup, field) for key, field in [
        ('max_health', 'maxHealth'), ('initial_health', 'health'),
        ('nail_damage', 'nailDamage'), ('max_soul', 'maxMP'), ('focus_cost', 'focusMP_amount')]}
    soul_literals = [_literal(instruction) for instruction in methods[('HeroController', 'SoulGain')]
                     if _literal(instruction) is not None]
    if not soul_literals or soul_literals[0] != 11:
        raise ValueError('unvalidated no-charm SoulGain control flow')
    values['soul_per_hit'] = soul_literals[0]
    values['death_wait_seconds'] = assignment(('HeroController', '.ctor'), 'DEATH_WAIT')
    values['enemy_hit_evasion_seconds'] = assignment(('HealthManager', 'NonFatalHit'), 'evasionByHitRemaining')
    for field in ['INVUL_TIME', 'RECOIL_DURATION', 'RECOIL_VELOCITY',
                  'DAMAGE_FREEZE_DOWN', 'DAMAGE_FREEZE_WAIT', 'DAMAGE_FREEZE_UP']:
        value = hero_constants[field]
        if not math.isfinite(value) or value < 0:
            raise ValueError(f'invalid HeroController scalar {field}')
        values[field] = value
    values['assembly_sha256'] = hashlib.sha256(assembly.read_bytes()).hexdigest()
    values['source_methods'] = ['.'.join(pair) for pair in sorted(wanted)] + [
        'HeroController.TakeDamage', 'HeroController.StartInvulnerable',
        'HeroController.CanTakeDamage', 'HeroController.StartRecoil coroutine',
        'HeroController.FixedUpdate', 'HealthManager.Hit', 'HealthManager.Die']
    return values


def generated_nail_response_params(hero_constants, fixed_dt):
    """Hero FixedUpdate uses recoilSteps 0..RECOIL_HOR_STEPS inclusive.

    NailSlash OnTriggerEnter/Stay selects this normal bounce on layers 11/19/17;
    BigBouncer and BounceShroom require their own behavior and are excluded.
    """
    fields = {
        'recoil_ticks': ticks((hero_constants['RECOIL_HOR_STEPS'] + 1) * fixed_dt),
        'recoil_speed': round(hero_constants['RECOIL_HOR_VELOCITY'] * 65536),
        'bounce_ticks': ticks(hero_constants['BOUNCE_TIME']),
        'bounce_speed': round(hero_constants['BOUNCE_VELOCITY'] * 65536),
        'down_speed': round(hero_constants['RECOIL_DOWN_VELOCITY'] * 65536),
    }
    if any(type(v) is not int or not 0 <= v <= (65535 if k.endswith('ticks') else 0x7fffffff)
           for k, v in fields.items()):
        raise ValueError('nail response exceeds bounded representation')
    return ('pub const NAIL_RESPONSE_PARAMS: hk_sim::NailResponseParams = hk_sim::NailResponseParams {' +
            ','.join(f'{k}:{v}' for k,v in fields.items()) + '};\n')


def generated_vital_params(values):
    fields = {
        'max_health': values['max_health'], 'max_soul': values['max_soul'],
        'nail_damage': values['nail_damage'], 'soul_per_hit': values['soul_per_hit'],
        'invulnerable_ticks': ticks(values['INVUL_TIME'] + values['DAMAGE_FREEZE_DOWN']),
        'hazard_invulnerable_ticks': ticks(values['INVUL_TIME'] / 2 + values['DAMAGE_FREEZE_DOWN']),
        'recoil_ticks': ticks(values['RECOIL_DURATION']),
        'freeze_ticks': ticks(sum(values[k] for k in ['DAMAGE_FREEZE_DOWN', 'DAMAGE_FREEZE_WAIT', 'DAMAGE_FREEZE_UP'])),
        'death_ticks': ticks(values['death_wait_seconds']),
        'recoil_speed': round(values['RECOIL_VELOCITY'] * 65536),
    }
    if any(type(value) is not int or not 0 <= value <= (0x7fffffff if key == 'recoil_speed' else 65535)
           for key, value in fields.items()):
        raise ValueError('vital parameter exceeds guest fixed-width representation')
    return ('pub const VITAL_PARAMS: hk_sim::VitalParams = hk_sim::VitalParams {' +
            ','.join(f'{key}:{value}' for key, value in fields.items()) + '};\n' +
            f'pub const ENEMY_HIT_EVASION_TICKS: u16 = {ticks(values["enemy_hit_evasion_seconds"])};\n')


def _component_records(sc, gid):
    return [(i, typ, tree) for i, (typ, tree) in sc.objects.items()
            if tree.get('m_GameObject', {}).get('m_PathID') == gid]


def action_fields(data, action_index):
    """Decode the narrow typed action parameters used by WalkLeftRight."""
    start = data['actionStartIndex'][action_index]
    end = (data['actionStartIndex'][action_index + 1]
           if action_index + 1 < len(data['actionStartIndex']) else len(data['paramName']))
    fields = {}
    for index in range(start, end):
        name = data['paramName'][index]
        kind = data['paramDataType'][index]
        position = data['paramDataPos'][index]
        size = data['paramByteDataSize'][index]
        if kind == 1 and size == 1:
            value = bool(data['byteData'][position])
        elif kind == 2 and size == 4:
            value = struct.unpack('<f', bytes(data['byteData'][position:position + 4]))[0]
        elif kind in (15, 17, 18):
            value = data[{15: 'fsmFloatParams', 17: 'fsmBoolParams', 18: 'fsmStringParams'}[kind]][position]
            if value['useVariable']:
                raise ValueError(f'dynamic action parameter unsupported: {name}')
            value = value['value']
        elif kind == 20:
            owner = data['fsmOwnerDefaultParams'][position]
            if owner['ownerOption'] != 0:
                raise ValueError('external action owner unsupported')
            value = 'owner'
        else:
            raise ValueError(f'unsupported action parameter {name}/{kind}/{size}')
        fields[name] = value
    return fields


def walker_control(sc, actor):
    """Recognize the observed Crawler Walk state and original C# action."""
    source = sc.source
    records = _component_records(sc, actor['game_object'])
    if sc.gos[actor['game_object']]['m_Layer'] != 11 or any(
            typ in ('BigBouncer', 'BounceShroom') or (typ == 'NonBouncer' and tree['active'])
            for _, typ, tree in records):
        raise ValueError('unsupported Crawler nail response variant')
    matching = []
    for sid in actor['fsm_ids']:
        fsm = source.read(sc.file.objects[int(sid.rsplit(':', 1)[1])])['fsm']
        if fsm['name'] == 'Crawler':
            matching.append((sid, fsm))
    if len(matching) != 1:
        raise ValueError('no single supported Crawler FSM')
    sid, fsm = matching[0]
    if fsm['startState'] != 'Walk':
        raise ValueError('Crawler begins in unsupported state')
    first = [v['value'] for v in fsm['variables']['boolVariables'] if v['name'] == 'First Crawler']
    if first != [0]:
        raise ValueError('First Crawler wait/event behavior unsupported')
    state = next(st for st in fsm['states'] if st['name'] == 'Walk')
    data = state['actionData']
    if data['actionNames'] != ['HutongGames.PlayMaker.Actions.BoolTest', 'HutongGames.PlayMaker.Actions.WalkLeftRight'] or data['actionEnabled'] != [1, 1]:
        raise ValueError('Crawler Walk action sequence changed')
    fields = action_fields(data, 1)
    if fields['groundLayer'] != 'Terrain' or fields['walkSpeed'] <= 0 or fields['walkSpeed'] > 16:
        raise ValueError('unsupported walker terrain/speed')
    if not 0 <= fields['turnDelay'] <= 10:
        raise ValueError('walker cooldown exceeds tick bound')
    matrix = sc.world(sc.go_transform[actor['game_object']])
    if any(abs(matrix[row][column]) > .00001 for row, column in [(0, 1), (1, 0), (0, 2), (1, 2)]):
        raise ValueError('rotated crawler movement unsupported')
    if abs(matrix[0][0]) < .001 or matrix[1][1] <= 0:
        raise ValueError('unsupported crawler transform')
    library_object = source.ref(sc.file, actor['tk2dSpriteAnimator']['library'])
    library = source.read(library_object)
    clips = {clip['name']: clip for clip in library['clips']}
    walk = clips[fields['walkAnimName']]
    turn = clips[fields['turnAnimName']]
    if not turn['frames'] or turn['fps'] <= 0 or not walk['frames'] or walk['fps'] <= 0:
        raise ValueError('invalid crawler animation duration')
    direction = (1 if matrix[0][0] > 0 else -1) * (-1 if fields['spriteFacesLeft'] else 1)
    return {'kind': 'WalkLeftRight', 'fsm': sid, 'action_state': 'Walk',
            'source_action_fields': fields, 'speed': fields['walkSpeed'],
            'turn_cooldown_ticks': ticks(fields['turnDelay']),
            'turn_ticks': ticks(len(turn['frames']) / turn['fps']),
            'initial_direction': direction,
            'random_start_direction': not (fields['startLeft'] or fields['startRight'] or fields['keepDirection']),
            'library_source': source.sid(library_object),
            'walk_clip_name': fields['walkAnimName'], 'turn_clip_name': fields['turnAnimName'],
            'ray_parameters': {'ahead_margin': .1, 'height_above_bottom': .5, 'down_length': 1.0},
            'source_methods': ['WalkLeftRight.SetupStartingDirection', 'WalkLeftRight.Walk coroutine',
                               'WalkLeftRight.Turn coroutine', 'WalkLeftRight.CheckWall',
                               'WalkLeftRight.CheckFloor', 'WalkLeftRight.CheckIsGrounded']}


def _colliders(sc, gid, records):
    result = []
    for sid, typ, tree in records:
        if typ not in ('BoxCollider2D', 'PolygonCollider2D', 'CircleCollider2D') or not tree.get('m_Enabled'):
            continue
        offset = tree['m_Offset']
        if typ == 'BoxCollider2D':
            half = (tree['m_Size']['x'] / 2, tree['m_Size']['y'] / 2)
            paths = [[(-half[0], -half[1]), (half[0], -half[1]),
                      (half[0], half[1]), (-half[0], half[1])]]
        elif typ == 'PolygonCollider2D':
            paths = [[(point['x'], point['y']) for point in path]
                     for path in tree['m_Points']['m_Paths']]
        else:
            result.append({'source': f'{sc.file.name}:{sid}', 'type': typ,
                           'unsupported': 'exact circle contact not yet cooked'})
            continue
        polygons = [[sc.point(gid, x + offset['x'], y + offset['y'])[:2] for x, y in path]
                    for path in paths]
        points = [point for path in polygons for point in path]
        if not points or any(not math.isfinite(value) or abs(value) > 512 for point in points for value in point):
            raise ValueError('actor collider exceeds Q16 world coordinate bounds')
        result.append({'source': f'{Path(sc.file.name).name}:{sid}', 'type': typ,
                       'trigger': bool(tree['m_IsTrigger']), 'world_polygons': polygons,
                       'bounds': [min(p[0] for p in points), min(p[1] for p in points),
                                  max(p[0] for p in points), max(p[1] for p in points)]})
    return result


def actor_sources(sc, bounds=None):
    result = []
    source = sc.source
    for sid, (typ, tree) in sc.objects.items():
        if typ != 'HealthManager' or not tree['m_Enabled']:
            continue
        gid = tree['m_GameObject']['m_PathID']
        if not sc.active(gid):
            continue
        position = sc.point(gid)
        if bounds and not (bounds[0] <= position[0] <= bounds[2] and bounds[1] <= position[1] <= bounds[3]):
            continue
        records = _component_records(sc, gid)
        actor = {'source': f'{Path(sc.file.name).name}:{sid}', 'game_object': gid,
                 'name': sc.gos[gid]['m_Name'], 'position': position,
                 'health': tree['hp'], 'health_manager': tree,
                 'colliders': _colliders(sc, gid, records), 'components': {str(i): kind for i, kind, _ in records},
                 'movement_supported': False, 'limitations': ['Enemy PlayMaker movement and state transitions remain unsupported.']}
        for component_id, kind, component in records:
            if kind in ('tk2dSprite', 'tk2dSpriteAnimator', 'Recoil', 'DamageHero', 'Rigidbody2D'):
                actor[kind] = component
                actor[kind + '_source'] = f'{Path(sc.file.name).name}:{component_id}'
        fsms = []
        for ref in sc.gos[gid]['m_Component']:
            obj = source.ref(sc.file, ref['component'])
            if obj.type.name == 'MonoBehaviour' and source.typename(obj) == 'PlayMakerFSM':
                fsms.append(source.sid(obj))
        actor['fsm_ids'] = fsms
        try:
            actor['movement_control'] = walker_control(sc, actor)
            actor['movement_supported'] = True
            actor['limitations'] = ['WalkLeftRight subset: global GO LEFT/RIGHT events and first-crawler activation are not executed.',
                                    'Initial random direction requires deterministic guest sampling; Unity RNG sequence is not reproduced.']
        except (ValueError, KeyError, StopIteration) as error:
            actor['movement_error'] = str(error)
        if not actor['movement_supported'] and any(kind == 'Walker' for _, kind, _ in records):
            from runner import recognize
            try:
                actor['pending_movement_control'] = recognize(sc, actor)
                actor['limitations'] = actor['pending_movement_control']['limitations']
            except (ValueError, KeyError, StopIteration) as error:
                actor['pending_movement_error'] = str(error)
        result.append(actor)
    if len(result) > 32:
        raise ValueError('actor region exceeds bounded 32-slot guest pool')
    return result


def generated_actor_region(region):
    """Generate bounded ActorSpec entries only after their source clips are cooked."""
    from effects import generated_corpse
    output = []
    for actor in region.get('actors', []):
        if not actor['movement_supported']:
            continue
        if 'walk_clip' not in actor or 'turn_clip' not in actor:
            raise ValueError(f'supported actor is missing cooked clips: {actor["source"]}')
        if len(output) == 32:
            raise ValueError('guest actor pool exceeds 32')
        colliders = [collider for collider in actor['colliders'] if 'bounds' in collider and not collider.get('trigger')]
        if not colliders or any(collider['bounds'] != colliders[0]['bounds'] for collider in colliders):
            raise ValueError('actor requires unsupported distinct body colliders')
        health = actor['health_manager']
        if health['hasSpecialDeath'] or health['hasAlternateHitAnimation'] or health['invincibleFromDirection']:
            raise ValueError('actor requires unsupported HealthManager variant')
        x, y = actor['position'][:2]
        box = colliders[0]['bounds']
        bounds = [round((box[0] - x) * 65536), round((box[1] - y) * 65536),
                  round((box[2] - x) * 65536), round((box[3] - y) * 65536)]
        control = actor['movement_control']
        recoil = actor.get('Recoil', {})
        damage = actor.get('DamageHero', {}).get('damageDealt', 0)
        fields = {
            'source_id': int(actor['source'].rsplit(':', 1)[1]), 'x': round(x * 65536), 'y': round(y * 65536),
            'bounds': '[' + ','.join(map(str, bounds)) + ']',
            'health': ('hk_sim::EnemyParams {' + f'health:{actor["health"]},contact_damage:{damage},evasion_ticks:12,' +
                       f'invincible:{str(bool(health["invincible"])).lower()},damage_override:{str(bool(health["damageOverride"])).lower()}' + '}'),
            'walk': ('hk_sim::WalkParams {' + f'speed:{round(control["speed"] * 65536)},turn_ticks:{control["turn_ticks"]},' +
                     f'turn_cooldown_ticks:{control["turn_cooldown_ticks"]}' + '}'),
            'initial_direction': control['initial_direction'],
            'random_start_direction': str(control['random_start_direction']).lower(),
            'walk_clip': actor['walk_clip'], 'turn_clip': actor['turn_clip'],
            'corpse': generated_corpse(actor.get('corpse')),
            'recoil_speed': round(recoil.get('recoilSpeedBase', 0) * 65536),
            'recoil_ticks': ticks(recoil.get('recoilDuration', 0)),
        }
        output.append('hk_sim::ActorSpec {' + ','.join(f'{key}:{value}' for key, value in fields.items()) + '}')
    return '&[' + ','.join(output) + ']'


def hazard_sources(sc, bounds=None):
    """Static enabled DamageHero shapes; enemy bodies are tracked separately."""
    health_gos = {tree['m_GameObject']['m_PathID'] for typ, tree in sc.objects.values() if typ == 'HealthManager'}
    result = []
    for sid, (typ, tree) in sc.objects.items():
        if typ != 'DamageHero' or not tree['m_Enabled'] or tree['damageDealt'] <= 0:
            continue
        gid = tree['m_GameObject']['m_PathID']
        if gid in health_gos or not sc.active(gid):
            continue
        for collider in _colliders(sc, gid, _component_records(sc, gid)):
            if 'bounds' not in collider:
                continue
            box = collider['bounds']
            if bounds and (box[2] < bounds[0] or box[0] > bounds[2] or box[3] < bounds[1] or box[1] > bounds[3]):
                continue
            result.append({**collider, 'source': f'{Path(sc.file.name).name}:{sid}', 'collider_source': collider['source'], 'name': sc.gos[gid]['m_Name'],
                           'damage': tree['damageDealt'], 'hazard_type': tree['hazardType'],
                           'position': sc.point(gid)})
    if len(result) > 64:
        raise ValueError('hazard region exceeds bounded 64-shape pool')
    return result


def pogo_sources(sc):
    """Static, enabled source NailSlash targets; special/dynamic bouncers explicit."""
    by_go = {}
    for sid, (typ, tree) in sc.objects.items():
        gid = tree.get('m_GameObject', {}).get('m_PathID')
        if gid:
            by_go.setdefault(gid, []).append((sid, typ, tree))
    result, unsupported = [], []
    for gid, go in sc.gos.items():
        layer = go['m_Layer']
        if layer not in (11, 17, 19) or not sc.active(gid):
            continue
        records = by_go.get(gid, [])
        types = {typ for _, typ, _ in records}
        if 'HealthManager' in types:
            continue  # Separate moving source-ID actor state, never spawn-position copies.
        if any(typ == 'NonBouncer' and tree['active'] for _, typ, tree in records):
            continue
        colliders = _colliders(sc, gid, records)
        if not colliders:
            continue
        blockers = types & {'BigBouncer', 'BounceShroom', 'PlayMakerFSM'}
        if any(typ == 'Rigidbody2D' and tree['m_BodyType'] != 2 for _, typ, tree in records):
            blockers.add('moving Rigidbody2D')
        if blockers:
            unsupported.append({'game_object': f'{Path(sc.file.name).name}:{gid}', 'name': go['m_Name'],
                                'reason': 'special/dynamic pogo: ' + ', '.join(sorted(blockers))})
            continue
        for collider in colliders:
            if 'bounds' not in collider:
                unsupported.append(collider)
                continue
            polygons = collider['world_polygons']
            if not 1 <= len(polygons) <= 8 or any(not 3 <= len(p) <= 16 for p in polygons):
                unsupported.append({'source': collider['source'], 'reason': 'pogo polygon bound'})
                continue
            result.append({**collider, 'game_object': f'{Path(sc.file.name).name}:{gid}',
                           'name': go['m_Name'], 'layer': layer, 'horizontal_and_up': layer == 11})
    if len(result) > 128:
        raise ValueError('static pogo pool exceeds 128 targets per scene')
    return {'targets': result, 'unsupported': unsupported,
            'source_methods': ['NailSlash.OnTriggerEnter2D', 'NailSlash.OnTriggerStay2D'],
            'policy': 'Static layer11/17/19 normal bouncers only; no active NonBouncer, HealthManager or special/dynamic bouncer'}


def postpack_pogo(report, source, scenes=None):
    """Reproducible metadata-only pass; no texture pack or source mutation."""
    from scene import Scene
    from source import ROOT
    scenes = {} if scenes is None else scenes
    output = ['// Generated static NailSlash-compatible targets, source IDs in regions.json.',
              'static POGO_SCENES: &[&[PogoTarget]] = &[']
    infos = sorted(report['scenes'], key=lambda row: row['scene_id'])
    if [r['scene_id'] for r in infos] != list(range(len(infos))) or len(infos) > 2:
        raise ValueError('pogo scene IDs must match bounded world scene table')
    for info in infos:
        scene_id = info['scene_id']
        if scene_id not in scenes:
            scenes[scene_id] = Scene(source, info.get('file', info.get('scene_file')))
        records = pogo_sources(scenes[scene_id])
        owned = {}
        for region in report['regions']:
            if region['scene_id'] != scene_id:
                continue
            for prop in region['breakables']:
                for collider in prop['disabled_collider_sources']:
                    owned[collider] = scene_id * 128 + prop['state_index']
        output.append('&[')
        for target in records['targets']:
            target['breakable_state_id'] = owned.get(target['source'])
            q = lambda v: str(round(v * 65536))
            polygon = lambda points: '&[' + ','.join('[' + ','.join(map(q, point)) + ']' for point in points) + ']'
            bounds = '[' + ','.join(map(q, target['bounds'])) + ']'
            broken = 'None' if target['breakable_state_id'] is None else f'Some({target["breakable_state_id"]})'
            output.append('PogoTarget {bounds:' + bounds + ',polygons:&[' +
                          ','.join(map(polygon, target['world_polygons'])) + '],breakable:' + broken +
                          ',horizontal_and_up:' + str(target['horizontal_and_up']).lower() + '},')
        output.append('],')
        info['static_pogo'] = records
    output.append('];\n')
    (ROOT/'data/pogo.rs').write_text('\n'.join(output))
