"""Source HealthManager/DamageHero metadata and verified no-charm vital values.

Enemy movement remains unsupported when controlled by PlayMaker. Metadata never
turns an inactive enemy on or replaces its controller with a generic patrol.
"""
import hashlib
import math
import struct
from pathlib import Path
from polygons import bounded_polygons
from combat import ticks
from scene import ADDITIVE_ID_BASE


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


# Serialized shape of the one placed Egg Sac (docs/EGG_SAC.md, level81:4050).
# The family has a single placement, so the recognizer matches it exactly rather
# than structurally: anything that differs stays an unadmitted record.
EGG_SAC_COMPONENTS = sorted([
    'AudioSource', 'BoxCollider2D', 'EnemyDeathEffects', 'EnemyDreamnailReaction',
    'ExtraDamageable', 'HealthManager', 'InfectedEnemyEffects', 'MeshFilter', 'MeshRenderer',
    'PersistentBoolItem', 'SetZ', 'SpriteFlash', 'Transform', 'tk2dSprite', 'tk2dSpriteAnimator'])
EGG_SAC_BODY_SIZE = (1.7711232900619507, 1.8317594528198242)
EGG_SAC_BODY_OFFSET = (-0.04640769958496094, -0.37363290786743164)
# name: (frames, fps, wrapMode, loopStart)
EGG_SAC_CLIPS = {'Idle': (4, 12., 0, 0), 'Death': (4, 12., 1, 1), 'Burst': (4, 18., 2, 0)}


def egg_sac_control(sc, actor):
    """Recognize the Egg Sac, a destructible with no FSM and no rigid body.

    There is no behaviour to port: `playAutomatically` loops the default clip
    and nothing else on the object moves it, damages the hero or recoils. The
    controller is therefore "play Idle where you stand", and the only checks
    that matter are the ones proving nothing else is attached.
    """
    source = sc.source
    gid = actor['game_object']
    records = _component_records(sc, gid)
    if sc.gos[gid]['m_Layer'] != 11:
        raise ValueError('Egg Sac outside the enemy layer')
    if sorted(kind for _, kind, _ in records) != EGG_SAC_COMPONENTS:
        raise ValueError('unsupported Egg Sac component set')
    matrix = sc.world(sc.go_transform[gid])
    if any(abs(matrix[row][column] - (1 if row == column else 0)) > 1e-6 for row in range(2) for column in range(2)):
        raise ValueError('unsupported Egg Sac rotation or scale')
    body = next(tree for _, kind, tree in records if kind == 'BoxCollider2D')
    if not body['m_Enabled'] or body['m_IsTrigger'] or body['m_EdgeRadius'] != 0 \
            or any(abs(body['m_Size'][axis] - value) > 1e-6 for axis, value in zip('xy', EGG_SAC_BODY_SIZE)) \
            or any(abs(body['m_Offset'][axis] - value) > 1e-6 for axis, value in zip('xy', EGG_SAC_BODY_OFFSET)):
        raise ValueError('unsupported Egg Sac body collider')
    health = actor['health_manager']
    if health['hp'] <= 0 or any(health[key] for key in (
            'invincible', 'invincibleFromDirection', 'hasSpecialDeath',
            'hasAlternateHitAnimation', 'damageOverride', 'megaFlingGeo')):
        raise ValueError('unsupported Egg Sac HealthManager variant')
    sprite = actor['tk2dSprite']
    if sprite['_color'] != {'r': 1., 'g': 1., 'b': 1., 'a': 1.} or sprite['_scale'] != {'x': 1., 'y': 1., 'z': 1.}:
        raise ValueError('unsupported Egg Sac sprite scale/color')
    animator = actor['tk2dSpriteAnimator']
    if not animator['m_Enabled'] or not animator['playAutomatically'] or animator['isRealtime']:
        raise ValueError('unsupported Egg Sac animator startup')
    library_object = source.ref(sc.file, animator['library'])
    library = source.read(library_object)
    clips = {clip['name']: clip for clip in library['clips'] if clip['name']}
    if library['clips'][animator['defaultClipId']]['name'] != 'Idle':
        raise ValueError('Egg Sac does not start on Idle')
    for name, expected in EGG_SAC_CLIPS.items():
        clip = clips.get(name)
        if clip is None or (len(clip['frames']), clip['fps'], clip['wrapMode'], clip.get('loopStart', 0)) != expected:
            raise ValueError('unsupported Egg Sac animation: ' + name)
        if any(frame.get('triggerEvent') for frame in clip['frames']):
            raise ValueError('unsupported Egg Sac animation events: ' + name)
    return {'kind': 'EggSac', 'guest_enabled': True, 'idle_clip_name': 'Idle',
            'library_source': source.sid(library_object),
            'limitations': [
                'No FSM, rigid body, Recoil or DamageHero on the source object: the guest loops Idle at the authored transform and answers only the nail.',
                'PersistentBoolItem is recorded, not honoured: a killed Egg Sac returns on a scene reload.',
                'The looping idle AudioSource, the SetZ depth and the death particles are not presented.']}


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
            result.append({'source': sc.sid(sid), 'type': typ,
                           'unsupported': 'exact circle contact not yet cooked'})
            continue
        polygons = [[sc.point(gid, x + offset['x'], y + offset['y'])[:2] for x, y in path]
                    for path in paths]
        try:  # guest records hold at most 16 vertices; pieces tile the source shape exactly
            polygons = [piece for polygon in polygons for piece in bounded_polygons(polygon)]
        except ValueError as error:
            result.append({'source': sc.sid(sid), 'type': typ, 'unsupported': f'collider polygon: {error}'})
            continue
        points = [point for path in polygons for point in path]
        if not points or any(not math.isfinite(value) or abs(value) > 512 for point in points for value in point):
            raise ValueError('actor collider exceeds Q16 world coordinate bounds')
        result.append({'source': sc.sid(sid), 'type': typ,
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
        actor = {'source': sc.sid(sid), 'spec_source_id': sid, 'game_object': gid,
                 'name': sc.gos[gid]['m_Name'], 'position': position,
                 'health': tree['hp'], 'health_manager': tree,
                 'colliders': _colliders(sc, gid, records), 'components': {str(i): kind for i, kind, _ in records},
                 'movement_supported': False, 'limitations': ['Enemy PlayMaker movement and state transitions remain unsupported.']}
        for component_id, kind, component in records:
            if kind in ('tk2dSprite', 'tk2dSpriteAnimator', 'Recoil', 'DamageHero', 'Rigidbody2D',
                        'EnemyDreamnailReaction'):
                actor[kind] = component
                actor[kind + '_source'] = sc.sid(component_id)
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
        if not actor['movement_supported'] and any(kind == 'Climber' for _, kind, _ in records):
            from climber import recognize as recognize_climber
            try:
                control = recognize_climber(sc, actor)
                actor['movement_control'] = control
                actor['movement_supported'] = True
                actor['limitations'] = list(control['limitations'])
            except (ValueError, KeyError, StopIteration) as error:
                actor['movement_error'] = str(error)
        if not actor['movement_supported'] and any(kind == 'LineOfSightDetector' for _, kind, _ in records) \
                and sc.gos[gid]['m_Name'].startswith('Buzzer'):
            from vengefly import recognize as recognize_vengefly
            try:
                control = recognize_vengefly(sc, actor)
                actor['movement_control'] = control
                actor['movement_supported'] = True
                actor['limitations'] = list(control['limitations'])
            except (ValueError, KeyError, StopIteration) as error:
                actor['movement_error'] = str(error)
        if not actor['movement_supported'] and sc.gos[gid]['m_Name'].startswith('Fly') \
                and any(kind == 'PlayMakerCollisionStay2D' for _, kind, _ in records):
            from gruzzer import recognize as recognize_gruzzer
            try:
                control = recognize_gruzzer(sc, actor)
                actor['movement_control'] = control
                actor['movement_supported'] = True
                actor['limitations'] = list(control['limitations'])
            except (ValueError, KeyError, StopIteration) as error:
                actor['movement_error'] = str(error)
        if not actor['movement_supported'] and sc.gos[gid]['m_Name'].startswith('Roller') \
                and any(kind == 'LineOfSightDetector' for _, kind, _ in records):
            from baldur import recognize as recognize_baldur
            try:
                control = recognize_baldur(sc, actor)
                actor['movement_control'] = control
                actor['movement_supported'] = True
                actor['limitations'] = list(control['limitations'])
            except (ValueError, KeyError, StopIteration) as error:
                actor['movement_error'] = str(error)
        if not actor['movement_supported'] and sc.gos[gid]['m_Name'].startswith('Spitter') \
                and any(kind == 'PersonalObjectPool' for _, kind, _ in records):
            from aspid import recognize as recognize_aspid
            try:
                control = recognize_aspid(sc, actor)
                actor['movement_control'] = control
                actor['movement_supported'] = True
                actor['limitations'] = list(control['limitations'])
            except (ValueError, KeyError, StopIteration) as error:
                actor['movement_error'] = str(error)
        if not actor['movement_supported'] and sc.gos[gid]['m_Name'].startswith('Egg Sac') and not fsms:
            try:
                control = egg_sac_control(sc, actor)
                actor['movement_control'] = control
                actor['movement_supported'] = True
                actor['limitations'] = list(control['limitations'])
            except (ValueError, KeyError, StopIteration) as error:
                actor['movement_error'] = str(error)
        if not actor['movement_supported'] and sc.gos[gid]['m_Name'].startswith('False Knight') \
                and any(kind == 'EnemyHitEffectsArmoured' for _, kind, _ in records):
            from false_knight import recognize_placement
            try:
                control = recognize_placement(sc, actor)
                actor['movement_control'] = control
                actor['movement_supported'] = True
                actor['limitations'] = list(control['limitations'])
            except (ValueError, KeyError, StopIteration) as error:
                actor['movement_error'] = str(error)
        if not actor['movement_supported'] and any(kind == 'Walker' for _, kind, _ in records):
            from runner import recognize
            try:
                # A strictly recognized Zombie Runner is admitted: the guest binds
                # its controller, clips, corpse, senses and audio bank.
                control = recognize(sc, actor)
                actor['movement_control'] = dict(control, guest_enabled=True)
                actor['movement_supported'] = True
                actor['limitations'] = [limit for limit in control['limitations']
                                        if not limit.startswith('Guest actor/clip/sensing/audio bindings')]
                actor['limitations'].append('Charge Dust and source death effects are not presented; audio uses the resident Runner bank.')
            except (ValueError, KeyError, StopIteration) as error:
                actor['pending_movement_error'] = str(error)
        # Content merged in from an additive scene is admitted one controller at
        # a time rather than wholesale, because the merge exists for one of them.
        # Crossroads_10_boss brings the False Knight and the three pre-battle
        # Zombies that `Battle Control` kills with KILL ALL ENEMIES the moment
        # the boss lands. Both halves of the reason this comment used to give
        # have expired, so read them as history rather than as the rule:
        #
        # The arena does run now. The boss is fought and killed on the disc,
        # `Action::KillAllEnemies` is handled, and the `boss-fight` route takes
        # the arena to Open.
        #
        # And the CLUT figure does not reproduce. It said the two banks reach
        # 438 of 416 after the room dedup. Re-measuring by replaying
        # `regions.postpack_actor_bank` and `deduplicate_room` in memory puts
        # the tightest view at 379 of 416 with the Zombies lifted, before
        # `postpack_similarity` runs, with no resident page. The +99 the comment
        # attributed to them is right; the baseline it was measured against was
        # not. That leaves the NPC bank, appended after this one, as the only
        # part of the figure still unaccounted for.
        #
        # What is holding them out today is neither: the `boss-fight` tape is
        # 239 events tuned to the exact fight, and standing three more enemies
        # in that arena invalidates it on the first frame. Admitting them is a
        # re-authoring job, not a budget one.
        if actor['movement_supported'] and sid >= ADDITIVE_ID_BASE \
                and not actor['movement_control'].get('admit_from_additive_scene'):
            actor['movement_supported'] = False
            actor['movement_error'] = (
                f'{actor["movement_control"]["kind"]} merged in from an additive scene; only a'
                ' controller that opts in is admitted from one')
        result.append(actor)
    # Only supported actors enter the guest pool; unsupported ones stay records.
    if sum(1 for actor in result if actor['movement_supported']) > 32:
        raise ValueError('actor region exceeds bounded 32-slot guest pool')
    return result


def generated_actor_region(region):
    """Inline `&[ActorSpec,...]` form of a region's supported actors (fixtures/tests)."""
    return '&[' + ','.join(generated_actor_specs(region)) + ']'


def generated_actor_specs(region):
    """Generate bounded ActorSpec expressions only after their source clips are cooked."""
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
        control = actor['movement_control']
        # A controller may answer the nail differently from its HealthManager,
        # and the False Knight does: `Check Health` restores the body instead of
        # letting it die, and `hasSpecialDeath` is the death sequence its own
        # controller runs. A recognizer has to claim that explicitly, because
        # for everything else a special death is behaviour nothing reproduces.
        invincible = control.get('invincible', bool(health['invincible']))
        if (health['hasSpecialDeath'] and not (control.get('invincible') or control.get('special_death'))) \
                or health['hasAlternateHitAnimation'] or health['invincibleFromDirection']:
            raise ValueError('actor requires unsupported HealthManager variant')
        x, y = actor['position'][:2]
        box = colliders[0]['bounds']
        bounds = [round((box[0] - x) * 65536), round((box[1] - y) * 65536),
                  round((box[2] - x) * 65536), round((box[3] - y) * 65536)]
        if control['kind'] == 'ZombieSwipeWalker':
            runner_clips = ('idle_clip', 'anticipate_clip', 'lunge_clip', 'cooldown_clip')
            if any(key not in actor for key in runner_clips):
                raise ValueError(f'Runner is missing cooked clips: {actor["source"]}')
            if not actor.get('corpse'):
                raise ValueError(f'Runner is missing validated corpse: {actor["source"]}')
            p = control['parameters']
            attack = p.get('attack', {'kind': 'Swipe'})
            if attack['kind'] == 'Leap':
                attack_text = (f'hk_sim::runner::Attack::Leap {{trigger_ticks:{int(attack["trigger_ticks"])},jump_speed_y:{round(attack["jump_speed_y"] * 65536)},'
                               f'jump_x_factor:{round(attack["jump_x_factor"] * 65536)},idle_ticks:{ticks(attack["idle_time"])}}}')
            else:
                attack_text = 'hk_sim::runner::Attack::Swipe'
            params = (f'hk_sim::runner::Params {{walk_speed:{p["walk_velocity_q16"][1]},lunge_speed:{p["lunge_velocity_q16"][1]},'
                      f'walking_wait:[{p["walking_wait_endpoints_ticks"][0]},{p["walking_wait_endpoints_ticks"][1]}],'
                      f'paused_wait:[{p["pause_endpoints_ticks"][0]},{p["pause_endpoints_ticks"][1]}],attack:{attack_text},'
                      f'gravity:{round(p.get("gravity_scale", 1.) * 60 * 65536)}}}')
            alert = '[' + ','.join(map(str, control['alert_bounds_q16'])) + ']'
            controller = ('hk_sim::ActorController::Runner {' + ','.join(f'{key}:{actor[key]}' for key in runner_clips)
                          + f',params:{params},alert:{alert}' + '}')
            # Legacy Walker fields remain populated for the common ActorSpec;
            # the Runner runtime uses its separately verified native controller.
            speed, turn_ticks, turn_cooldown_ticks = p['walk_speed'], 10, 60
            # Taken from the placement's transform mirror, not assumed.
            initial_direction, random_start_direction = p['initial_direction'], False
        elif control['kind'] == 'Climber':
            if 'stun_clip' not in actor or not actor.get('corpse'):
                raise ValueError(f'Climber is missing cooked clips or corpse: {actor["source"]}')
            controller = ('hk_sim::ActorController::Climber {'
                          f'stun_clip:{actor["stun_clip"]},start_right:{str(control["start_right"]).lower()},'
                          f'rotation_q16:{control["rotation_q16"]}' + '}')
            # The controller owns speed and turns; the walker fields are unused.
            speed, turn_ticks, turn_cooldown_ticks = 2.0, 15, 0
            initial_direction, random_start_direction = 1, False
        elif control['kind'] == 'Vengefly':
            vengefly_clips = ('startle_clip', 'chase_clip', 'turn_fly_clip')
            if any(key not in actor for key in vengefly_clips) or not actor.get('corpse'):
                raise ValueError(f'Vengefly is missing cooked clips or corpse: {actor["source"]}')
            controller = 'hk_sim::ActorController::Vengefly {' + ','.join(f'{key}:{actor[key]}' for key in vengefly_clips) + '}'
            # The controller owns velocity; the walker fields are unused.
            speed, turn_ticks, turn_cooldown_ticks = 0, 0, 0
            initial_direction, random_start_direction = -1, False
        elif control['kind'] == 'Baldur':
            baldur_clips = ('start_clip', 'roll_clip', 'stop_clip')
            if any(key not in actor for key in baldur_clips) or not actor.get('corpse'):
                raise ValueError(f'Baldur is missing cooked clips or corpse: {actor["source"]}')
            controller = 'hk_sim::ActorController::Baldur {' + ','.join(f'{key}:{actor[key]}' for key in baldur_clips) + '}'
            speed, turn_ticks, turn_cooldown_ticks = 0, 0, 0
            initial_direction, random_start_direction = -1, False
        elif control['kind'] == 'Aspid':
            aspid_clips = ('fire_clip', 'shot_clip', 'impact_clip')
            if any(key not in actor for key in aspid_clips) or not actor.get('corpse'):
                raise ValueError(f'Aspid is missing cooked clips or corpse: {actor["source"]}')
            controller = ('hk_sim::ActorController::Aspid {' + ','.join(f'{key}:{actor[key]}' for key in aspid_clips)
                          + f',start_alert:{str(bool(control.get("start_alert"))).lower()}' + '}')
            speed, turn_ticks, turn_cooldown_ticks = 0, 0, 0
            initial_direction, random_start_direction = -1, False
        elif control['kind'] == 'Gruzzer':
            if not actor.get('corpse'):
                raise ValueError(f'Gruzzer is missing validated corpse: {actor["source"]}')
            controller = 'hk_sim::ActorController::Gruzzer'
            speed, turn_ticks, turn_cooldown_ticks = 0, 0, 0
            initial_direction, random_start_direction = -1, False
        elif control['kind'] == 'EggSac':
            if 'idle_clip' not in actor or not actor.get('corpse'):
                raise ValueError(f'Egg Sac is missing cooked clips or corpse: {actor["source"]}')
            controller = f'hk_sim::ActorController::Static {{idle_clip:{actor["idle_clip"]}}}'
            # Nothing on the source object moves it, and its world basis is the
            # identity, so the guest draws the sprite in its native orientation.
            speed, turn_ticks, turn_cooldown_ticks = 0, 0, 0
            initial_direction, random_start_direction = -1, False
        elif control['kind'] == 'FalseKnight':
            # `walk_clip` is Idle and `turn_clip` is Turn; these four are the
            # rest of what fits the room budget, and the trigger box is what the
            # arena watches for BATTLE START. The walk fields are unused: the
            # controller owns every velocity the body ever has.
            false_knight_clips = ('jump_antic_clip', 'land_clip', 'stun_opened_clip', 'attack_clip',
                                  'barrel_clip')
            if any(key not in actor for key in false_knight_clips):
                raise ValueError(f'False Knight is missing cooked clips: {actor["source"]}')
            trigger = '[' + ','.join(str(round(value * 65536)) for value in control['arena_trigger_world']) + ']'
            # `summon`'s `Spawn` reads the summoner's own transform for the drop
            # height, so it is a placement number rather than a constant.
            spawn_y = round(control['barrel']['spawn_world'][1] * 65536)
            controller = ('hk_sim::ActorController::FalseKnight {'
                          + ','.join(f'{key}:{actor[key]}' for key in false_knight_clips)
                          + f',trigger:{trigger},barrel_spawn_y:{spawn_y}' + '}')
            speed, turn_ticks, turn_cooldown_ticks = 0, control['turn_ticks'], 0
            initial_direction, random_start_direction = control['initial_direction'], False
        elif control['kind'] == 'WalkLeftRight':
            controller = 'hk_sim::ActorController::Crawler'
            speed, turn_ticks, turn_cooldown_ticks = control['speed'], control['turn_ticks'], control['turn_cooldown_ticks']
            initial_direction, random_start_direction = control['initial_direction'], control['random_start_direction']
        else:
            raise ValueError('unsupported generated actor controller: ' + control['kind'])
        clip_fields = ('walk_clip', 'turn_clip') + (runner_clips if control['kind'] == 'ZombieSwipeWalker' else ()) \
            + (vengefly_clips if control['kind'] == 'Vengefly' else ()) + (baldur_clips if control['kind'] == 'Baldur' else ()) \
            + (aspid_clips if control['kind'] == 'Aspid' else ()) + (('idle_clip',) if control['kind'] == 'EggSac' else ()) \
            + (false_knight_clips if control['kind'] == 'FalseKnight' else ())
        if any(type(actor[key]) is not int or not 0 <= actor[key] <= 65535 for key in clip_fields):
            raise ValueError('actor clip binding exceeds u16')
        recoil = actor.get('Recoil', {})
        damage = actor.get('DamageHero', {}).get('damageDealt', 0)
        # EnemyDreamnailReaction::RecieveDreamImpact pays SOUL once, unless the
        # component sets noSoul or starts suppressed. 33 without Dream Wielder.
        dream = actor.get('EnemyDreamnailReaction')
        dream_soul = 33 if dream and not dream['noSoul'] and not dream['startSuppressed'] else 0
        fields = {
            # Scene-unique, which for an object merged in from an additive scene
            # is its shifted id rather than the one in its own file.
            'source_id': actor.get('spec_source_id', int(actor['source'].rsplit(':', 1)[1])),
            'x': round(x * 65536), 'y': round(y * 65536),
            'bounds': '[' + ','.join(map(str, bounds)) + ']',
            'health': ('hk_sim::EnemyParams {' + f'health:{actor["health"]},contact_damage:{damage},evasion_ticks:12,' +
                       f'invincible:{str(invincible).lower()},damage_override:{str(bool(health["damageOverride"])).lower()}' + '}'),
            'controller': controller,
            'walk': ('hk_sim::WalkParams {' + f'speed:{round(speed * 65536)},turn_ticks:{turn_ticks},' +
                     f'turn_cooldown_ticks:{turn_cooldown_ticks}' + '}'),
            'initial_direction': initial_direction,
            'random_start_direction': str(random_start_direction).lower(),
            'walk_clip': actor['walk_clip'], 'turn_clip': actor['turn_clip'],
            'corpse': generated_corpse(actor.get('corpse')),
            'recoil_speed': round(recoil.get('recoilSpeedBase', 0) * 65536),
            'recoil_ticks': ticks(recoil.get('recoilDuration', 0)),
            'dream_soul': dream_soul,
        }
        output.append('hk_sim::ActorSpec {' + ','.join(f'{key}:{value}' for key, value in fields.items()) + '}')
    return output


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
            result.append({**collider, 'source': sc.sid(sid), 'collider_source': collider['source'], 'name': sc.gos[gid]['m_Name'],
                           'damage': tree['damageDealt'], 'hazard_type': tree['hazardType'],
                           'position': sc.point(gid)})
    if len(result) > 64:
        raise ValueError('hazard region exceeds bounded 64-shape pool')
    return result


def shroom_sources(sc, bounds=None):
    """Enabled BounceShroom triggers, which pogo_sources deliberately refuses.

    NailSlash.OnTriggerEnter2D answers a down slash on one of these with
    HeroController.ShroomBounce rather than the ordinary Bounce, so the guest
    needs the trigger box and nothing else: the response is a constant.
    """
    result = []
    for sid, (typ, tree) in sc.objects.items():
        if typ != 'BounceShroom' or not tree['m_Enabled']:
            continue
        gid = tree['m_GameObject']['m_PathID']
        if not sc.active(gid):
            continue
        # The owning object's FSMs are recorded, not cooked: several of them
        # deactivate their object from PlayerData, which the guest cannot see.
        fsms = sorted(d['fsm']['name'] for _, (kind, d) in sc.objects.items()
                      if kind == 'PlayMakerFSM' and d['m_GameObject']['m_PathID'] == gid)
        for collider in _colliders(sc, gid, _component_records(sc, gid)):
            if 'bounds' not in collider or not collider['trigger']:
                continue
            box = collider['bounds']
            if bounds and (box[2] < bounds[0] or box[0] > bounds[2] or box[3] < bounds[1] or box[1] > bounds[3]):
                continue
            # The guest carries the box alone, so a shape whose bounds are
            # wider than the collider the source tests would silently grow the
            # target. Refuse it instead.
            if any(len(polygon) != 4 or {p[0] for p in polygon} != {box[0], box[2]}
                   or {p[1] for p in polygon} != {box[1], box[3]}
                   for polygon in collider['world_polygons']):
                raise ValueError(f'BounceShroom collider is not an axis-aligned box: {collider["source"]}')
            result.append({'source': sc.sid(sid), 'collider_source': collider['source'],
                           'name': sc.gos[gid]['m_Name'], 'bounds': box, 'owner_fsms': fsms,
                           'limitations': ['Down slash response only: no shroom bob, bounce animation or particles',
                                           'The owner FSMs are not run, so a PlayerData gate that would deactivate this object is ignored']})
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
            unsupported.append({'game_object': sc.sid(gid), 'name': go['m_Name'],
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
            result.append({**collider, 'game_object': sc.sid(gid),
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
    infos = sorted(report['scenes'], key=lambda row: row['scene_id'])
    from quality import SCENE_COUNT
    if [r['scene_id'] for r in infos] != list(range(len(infos))) or len(infos) > SCENE_COUNT:
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
        for target in records['targets']:
            target['breakable_state_id'] = owned.get(target['source'])
        # Targets ride in the world bank as objects of every region whose
        # activation envelope they touch (tools/world_metadata.py, kind 10).
        for region in report['regions']:
            if region['scene_id'] != scene_id:
                continue
            b = region['activation_bounds']
            region['pogo_targets'] = [target for target in records['targets']
                                      if target['bounds'][0] <= b[2] and target['bounds'][2] >= b[0]
                                      and target['bounds'][1] <= b[3] and target['bounds'][3] >= b[1]]
        info['static_pogo'] = records
