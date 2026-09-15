#!/usr/bin/env python3
"""Build an exhaustive source-role, content and PlayerData catalog."""
import argparse
from collections import Counter, defaultdict
import gzip
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'host'))
from focus import action_fields

FORMAT = 'HKWORLDCATALOG01'
HELPER_SCENES = {
    'Pre_Menu_Intro': 'startup/bootstrap UI',
    'Quit_To_Menu': 'explicit QuitToMenu transition helper',
    'BetaEnd': 'BetaEndPrompt terminal helper',
    'Crossroads_10_preload': 'source-named preload helper',
    'PermaDeath': 'permadeath transition screen',
    'PermaDeath_Unlock': 'permadeath unlock screen',
    'GG_Unlock': 'Godhome unlock screen',
}
CONTENT_TOKENS = {
    'actors': ('Enemy','HealthManager','DamageHero','Walker','Crawler','Turret','NPC'),
    'bosses': ('Boss',),
    'interactables': ('Inspect','Dialogue','Conversation','Bench','Lever','Toll','Breakable','Door'),
    'items': ('Pickup','Charm','Geo','Relic','Item','Collectable'),
    'shops': ('Shop','Stock'),
    'quests': ('Quest','Mushroom','Flower'),
    'audio': ('Audio','Music','Sound','Ambience'),
    'animation': ('Anim','tk2d'),
    'particles': ('Particle',),
}
INFRASTRUCTURE_TYPES = {'GameObject','Transform','RectTransform','Camera','Canvas',
    'CanvasRenderer','MeshRenderer','MeshFilter','SpriteRenderer','Renderer',
    'Rigidbody2D','BoxCollider2D','CircleCollider2D','PolygonCollider2D',
    'EdgeCollider2D','CompositeCollider2D','MeshCollider','SceneManager',
    'RenderSettings','LightmapSettings','NavMeshSettings','Light','AudioListener'}
CONTENT_OWNERS = {'actors':'P19-P21 actor runtime','bosses':'P22 boss runtime',
    'interactables':'P12/P17 interactions and NPCs','items':'P15/P16 items and inventory',
    'shops':'P17 economy and shops','quests':'P18 quests and transport',
    'audio':'P25 audio closure','animation':'P08/P26 rendering and effects',
    'particles':'P08/P26 rendering and effects','infrastructure':'P05-P09 scene runtime',
    'unclassified':'P12-P26 source-system implementation triage'}


def classify_component_type(name):
    categories = [category for category,tokens in CONTENT_TOKENS.items()
                  if any(token.casefold() in name.casefold() for token in tokens)]
    if name in INFRASTRUCTURE_TYPES:
        categories.append('infrastructure')
    if not categories:
        categories.append('unclassified')
    return sorted(set(categories))


def classify_scene(scene):
    name = scene['scene_name']; path = scene.get('path', '')
    components = scene.get('component_types', {}); counts = scene.get('counts', {})
    evidence = []
    if name == 'Menu_Title' or components.get('MainMenuOptions'):
        role = 'menu'
        evidence.append({'kind':'component', 'value':'MainMenuOptions'})
    elif components.get('GameCompletionScreen'):
        role = 'completion_screen'
        evidence.append({'kind':'component', 'value':'GameCompletionScreen'})
    elif components.get('CreditsHelper') or name in ('End_Credits', 'Menu_Credits'):
        role = 'credits'
        evidence.append({'kind':'component_or_exact_name',
                         'value':'CreditsHelper/End_Credits/Menu_Credits'})
    elif name in HELPER_SCENES:
        role = 'transition_or_bootstrap_helper'
        evidence.append({'kind':'exact_scene_contract', 'value':HELPER_SCENES[name]})
    elif name.endswith('_boss_defeated') and not components.get('SceneManager'):
        role = 'additive_boss_defeated_variant'
        evidence.append({'kind':'exact_suffix_and_component_absence',
                         'value':'_boss_defeated without SceneManager'})
    elif (name.startswith('Cinematic_') or '/Cinematics/' in path or
          (components.get('InGameCutsceneInfo') and not counts.get('gates'))):
        role = 'cinematic'
        evidence.append({'kind':'source_path_or_component',
                         'value':'Cinematic_ path/name or InGameCutsceneInfo'})
    elif components.get('BossSceneController'):
        role = 'boss_arena'
        evidence.append({'kind':'component', 'value':'BossSceneController'})
    elif '/Dream/' in path or name.startswith('Dream_'):
        role = 'dream_gameplay_or_variant'
        evidence.append({'kind':'source_path_or_exact_prefix', 'value':'Dream'})
    elif name == 'Knight_Pickup' and components.get('HeroController'):
        role = 'shared_player_prefab_scene'
        evidence.append({'kind':'component_and_exact_name', 'value':'HeroController/Knight_Pickup'})
    elif (counts.get('terrain_edges') or counts.get('gates') or counts.get('tilemaps') or
          components.get('HealthManager') or components.get('SceneManager')):
        role = 'gameplay'
        evidence.append({'kind':'source_system_counts', 'value':{
            key:value for key,value in (
                ('terrain_edges', counts.get('terrain_edges', 0)),
                ('gates', counts.get('gates', 0)), ('tilemaps', counts.get('tilemaps', 0)),
                ('HealthManager', components.get('HealthManager', 0)),
                ('SceneManager', components.get('SceneManager', 0))) if value}})
    else:
        role = 'support_scene_unresolved'
        evidence.append({'kind':'explicit_unresolved',
                         'value':'no known role contract; retained for P02 review'})
    features = []
    for feature, types in {
        'boss_content': ('BossSceneController','BossSequenceDoor','BossStatue'),
        'cinematic_content': ('CinematicPlayer','CinematicSequence','InGameCutsceneInfo'),
        'bench': ('RestBench',), 'shop': ('ShopMenuStock',),
        'stag_transport': ('SpawnStagMenu','StagTravel'),
        'dialogue': ('DialogueBox','DialogueManager'),
    }.items():
        found = {kind:components[kind] for kind in types if components.get(kind)}
        if found:
            features.append({'feature':feature, 'evidence':found})
    return {'primary_role': role, 'evidence': evidence, 'features': features,
            'source_included': True, 'exclusion': None,
            'runtime_admission': 'not_evaluated'}


def player_data_direction(action):
    short = action.rsplit('.', 1)[-1]
    if short.startswith(('Set','Increment','Add','Take','Remove','Clear')) or short.endswith('Add'):
        return 'producer'
    if (short.startswith(('Get','Check','Test')) or 'Test' in short or
            'Compare' in short or short.startswith('PlayerDataBool')):
        return 'consumer'
    return 'unknown'


def field_literal(field):
    if isinstance(field, dict):
        if field.get('useVariable'):
            return None
        return field.get('value')
    return field


def fsm_string_values(fsm, variable):
    values = []
    for rows in (fsm.get('variables') or {}).values():
        if not isinstance(rows, list):
            continue
        for row in rows:
            if isinstance(row, dict) and row.get('name') == variable:
                value = row.get('value')
                if isinstance(value, str) and value:
                    values.append(value)
    return sorted(set(values))


def player_data_keys(action, fields, fsm=None):
    result = []
    for name, value in fields.items():
        folded = name.casefold()
        source_bool = action.startswith('PlayerDataBool') and (
            folded.endswith('bool') or name.isdigit())
        if ('name' not in folded and 'playerdata' not in folded and not source_bool) or folded in {
                'fsmname','methodname','eventname'}:
            continue
        literal = field_literal(value)
        if isinstance(literal, str) and literal:
            result.append({'field':name, 'key':literal, 'binding':'literal'})
        elif isinstance(value, dict) and value.get('useVariable') and fsm is not None:
            variable = value.get('name') or value.get('variableName')
            result.extend({'field':name, 'key':candidate, 'binding':'fsm_variable_initial',
                           'variable':variable}
                          for candidate in fsm_string_values(fsm, variable))
    return result


def scan_player_data(document):
    scene = document['scene']; rows = []
    for obj in document['objects']:
        if obj['type'] != 'PlayMakerFSM':
            continue
        fsm = obj['data'].get('fsm') or {}
        for state in fsm.get('states') or []:
            data = state.get('actionData') or {}
            for index, full_name in enumerate(data.get('actionNames') or []):
                action = full_name.rsplit('.', 1)[-1]
                if 'PlayerData' not in action:
                    continue
                try:
                    fields = action_fields(data, index)
                except Exception as error:
                    rows.append({'source_scene':scene['scene_name'],
                        'source_file':scene['file'], 'fsm_source':obj['source'],
                        'fsm_name':fsm.get('name',''), 'state':state.get('name',''),
                        'action_index':index, 'action':action, 'direction':'unknown',
                        'keys':[], 'parse_error':f'{type(error).__name__}: {error}'})
                    continue
                keys = player_data_keys(action, fields, fsm)
                assigned = []
                if player_data_direction(action) == 'producer':
                    for field_name in ('value','amount','add'):
                        value = field_literal(fields.get(field_name))
                        if isinstance(value, (bool, int, float, str)) and value != '':
                            assigned.append({'field':field_name, 'value':value})
                rows.append({'source_scene':scene['scene_name'], 'source_file':scene['file'],
                    'fsm_source':obj['source'], 'fsm_name':fsm.get('name',''),
                    'state':state.get('name',''), 'action_index':index, 'action':action,
                    'direction':player_data_direction(action),
                    'keys':keys,
                    'assigned_literals':assigned,
                    'key_resolution':('serialized' if keys else 'runtime_variable_unresolved'),
                    'owner_task':(None if keys else 'P13/P14 persistence and progression')})
    return rows


def aggregate_player_data(rows):
    keys = {}
    unresolved = []
    for row in rows:
        if not row['keys']:
            unresolved.append(row)
        for item in row['keys']:
            entry = keys.setdefault(item['key'], {'key':item['key'], 'producers':0,
                'consumers':0, 'unknown':0, 'scene_indices':set(), 'examples':[],
                'assigned_literals':[]})
            direction = row['direction']
            entry[direction + 's' if direction in ('producer','consumer') else 'unknown'] += 1
            entry['scene_indices'].add(row['source_file'])
            if len(entry['examples']) < 8:
                entry['examples'].append({key:row[key] for key in
                    ('source_scene','source_file','fsm_source','fsm_name','state',
                     'action_index','action','direction')})
            for assignment in row.get('assigned_literals', []):
                if assignment not in entry['assigned_literals']:
                    entry['assigned_literals'].append(assignment)
    result = []
    for entry in keys.values():
        entry['source_files'] = sorted(entry.pop('scene_indices'))
        entry['scene_count'] = len(entry['source_files'])
        result.append(entry)
    return sorted(result, key=lambda row:(-row['scene_count'],row['key'])), unresolved


def content_summary(scene):
    components = scene.get('component_types', {})
    groups = {
        'actors': ('HealthManager','DamageHero','EnemyDreamnailReaction'),
        'bosses': ('BossSceneController','BossStatue','BossSequenceDoor'),
        'interactions': ('RestBench','InspectRegion','DialogueBox','BridgeLever','TollGate'),
        'shops': ('ShopMenuStock',),
        'transport': ('TransitionPoint','SpawnStagMenu','StagTravel'),
        'audio': ('AudioSource','AudioManager','MusicCue'),
        'animation': ('Animator','tk2dSpriteAnimator'),
        'particles': ('ParticleSystem','ParticleSystemRenderer'),
    }
    return {group:{kind:components[kind] for kind in kinds if components.get(kind)}
            for group,kinds in groups.items()
            if any(components.get(kind) for kind in kinds)}


def transition_points(geometry):
    colliders = {row['source']:row for row in geometry.get('colliders', [])}
    result = []
    for gate in geometry.get('gates', []):
        shapes = []
        for source in gate.get('colliders', []):
            collider = colliders.get(source)
            shapes.append({'source':source, 'resolved':collider is not None,
                'type':collider.get('type') if collider else None,
                'trigger':collider.get('trigger') if collider else None,
                'world_shape':(collider.get('world_shape') if collider else None),
                'world_paths':(collider.get('world_paths') if collider else None)})
        result.append({'source_scene':geometry['scene']['scene_name'],
            'source_file':geometry['scene']['file'], 'source':gate['source'],
            'game_object':gate['game_object'], 'gate_name':gate['name'],
            'position':gate['position'], 'enabled':gate['enabled'],
            'active_self':gate['active_self'],
            'active_hierarchy':gate['active_hierarchy'],
            'target_scene':gate.get('target_scene'),
            'entry_point':gate.get('entry_point'),
            'entry_offset':gate.get('entry_offset'),
            'serialized':gate.get('serialized', {}), 'trigger_shapes':shapes})
    return result


def build(world, component_root):
    coverage = world.get('coverage', {})
    if (world.get('run', {}).get('status') != 'verified' or
            not world.get('inputs_unchanged') or coverage.get('failed_scenes') or
            not coverage.get('all_scenes_processed')):
        raise ValueError('World import must be complete and source-verified')
    scenes = []; player_rows = []; transitions = []
    for source in world['scenes']:
        classification = classify_scene(source)
        scenes.append({'index':source['index'], 'file':source['file'],
            'scene_name':source['scene_name'], 'path':source.get('path'),
            **classification, 'content':content_summary(source),
            'component_type_count':len(source.get('component_types', {})),
            'fsm_action_type_count':len(source.get('fsm_action_types', {}))})
        with gzip.open(component_root/source['file']/'components.json.gz', 'rt') as stream:
            player_rows.extend(scan_player_data(json.load(stream)))
        with gzip.open(component_root/source['file']/'geometry.json.gz', 'rt') as stream:
            transitions.extend(transition_points(json.load(stream)))
    inventory, unresolved = aggregate_player_data(player_rows)
    roles = Counter(scene['primary_role'] for scene in scenes)
    component_catalog = []
    category_counts = Counter()
    for name, source in sorted(coverage.get('systems', {}).items()):
        categories = classify_component_type(name); category_counts.update(categories)
        component_catalog.append({'type':name, 'instances':source['instances'],
            'scene_count':len(set(source['scene_indices'])), 'categories':categories,
            'owner_tasks':sorted({CONTENT_OWNERS[category] for category in categories})})
    return {'format':FORMAT,
        'scope':'All source scenes and serialized content; no PS1 runtime support claim',
        'source_world_fingerprint':world['fingerprint'], 'scene_count':len(scenes),
        'all_scenes_included':all(scene['source_included'] for scene in scenes),
        'role_counts':dict(sorted(roles.items())), 'scenes':scenes,
        'component_type_catalog':component_catalog,
        'component_category_counts':dict(sorted(category_counts.items())),
        'transition_points':transitions,
        'transition_point_count':len(transitions),
        'transition_points_with_unresolved_trigger_shapes':sum(
            any(not shape['resolved'] for shape in row['trigger_shapes']) for row in transitions),
        'player_data':{'action_count':len(player_rows), 'key_count':len(inventory),
            'keys':inventory, 'unresolved_key_actions':unresolved},
        'limitations':[
            'Primary roles are source-evidenced packaging categories, not proof of reachability or guest admission.',
            'Scenes with mixed gameplay/cinematic content keep secondary feature evidence.',
            'PlayerData records cover serialized PlayMaker actions; managed-code reads and writes require IL analysis.',
            'Unclassified component types remain included with a P02 owner instead of being treated as unsupported or unused.',
            'Every BuildSettings scene remains included even when its role is unresolved.',
            'TransitionPoint activation is serialized initial state; runtime FSM overrides remain in the scripted transition dataflow catalog.',
        ]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--world', type=Path, default=ROOT/'.hkpsx/world-import/report.json')
    parser.add_argument('--components', type=Path, default=ROOT/'.hkpsx/world-import')
    parser.add_argument('--output', type=Path, default=ROOT/'.hkpsx/world-import/world-catalog.json')
    args = parser.parse_args(); output = args.output.resolve()
    if not output.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Output must be inside .hkpsx')
    result = build(json.loads(args.world.read_text()), args.components)
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key:result[key] for key in
        ('scene_count','all_scenes_included','role_counts')}, indent=2))
    print(json.dumps({key:result['player_data'][key] for key in
        ('action_count','key_count')}, indent=2))


if __name__ == '__main__':
    main()
