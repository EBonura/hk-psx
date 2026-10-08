"""Generate bounded guest metadata from cooked region indices, never raw Unity."""
import json
import math
from pathlib import Path
from source import ROOT
from actors import scene_actor_bank
import sys
sys.path.insert(0, str(ROOT / 'tools'))
from world_metadata import damagehero_respawns, persistent_object
from flat_support import generate as flat_floor_catalog, rust_catalog

from quality import SCENE_COUNT as SCENES
BREAKABLES_PER_SCENE = 128
GRASS_PER_SCENE = 1024

# Catalogue slots the disc format can address, and the reason it is this many.
# A scene's gates carry their destination inside one u32 of that scene's world
# metadata bank: tools/world_metadata.py packs `region | target_scene << 16 |
# side << 24`, and game/src/world.rs reads the slot back out as `destination &
# 0xFFFF`. Sixteen bits is all a slot gets there, so 65536 is the catalogue's
# real ceiling. The two linked tables that name slots directly agree at that
# width: data/great_door.rs and data/battle_gates.rs are both `(u16, ...)` and
# both refuse above 65535 in their own generators.
#
# This replaces a 1024 that was called a sanity bound and was never measured.
# It bit at 943 regions with 99,716 bytes of linked headroom still free, and a
# slot costs seven linked bytes, read out of build 161's link map rather than
# estimated: REGION_SCENES 1 byte (data/regions.rs), REGION_SCENE_LOCAL 4
# (data/scene_manifest.rs), SCENERY_PACKET_BUDGETS 2 (data/scenery_budgets.rs).
# Those three are the whole per-slot cost; nothing else in the guest is sized
# per slot, and the scene arena and the metadata bank are per scene, so neither
# moves when the catalogue grows. RAM needs no bound of its own here either:
# the linker's RAM region is LOAD_ADDR..STACK_INIT less STACK_RESERVE (see
# host/stack_budget.py), so a catalogue that does not fit fails the link.
#
# What this does not raise: a scene may still hold only 256 regions, because
# REGION_SCENE_LOCAL stores the scene-local index in a u8. host/pack_scenes.py
# refuses that one. The busiest scene at build 161 is Tutorial_01 with 86.
MAX_REGIONS = 65536


def q(value):
    if not math.isfinite(value) or abs(value) > 512:
        raise ValueError('world coordinate outside Q16 bounds')
    return round(value * 65536)


def arr(values):
    return '[' + ','.join(map(str, values)) + ']'


def ids(values):
    return '&' + arr(values)


def integer(value, bound, name):
    if type(value) is not int or not 0 <= value < bound:
        raise ValueError(f'{name} outside bounded range 0..{bound-1}')
    return value


def rect(values):
    if len(values) != 4 or values[0] > values[2] or values[1] > values[3]:
        raise ValueError('invalid world rectangle')
    return arr(map(q, values))


def checkpoint_sources(sc, unsupported=None):
    """Compile direct HazardRespawnTrigger -> Marker links and source ground rays.

    HazardRespawnTrigger.OnTriggerEnter2D accepts the hero layer9 and calls
    PlayerData.SetHazardRespawn(marker). HeroController's hazard coroutine then
    FindGroundPoint(marker,true): downward50, terrainLayer8, body extents minus
    body offset plus0.01. The reported projection is the initial static terrain;
    scripted/dynamic terrain changes are not silently treated as resolved.
    """
    from breakables import _components, _local_id, collider_polygons
    from pathlib import Path
    source = sc.source; file = Path(sc.file.name).name
    terrain = []
    for index, (kind, tree) in sc.objects.items():
        if kind not in ('BoxCollider2D','PolygonCollider2D','EdgeCollider2D'):
            continue
        gid = tree['m_GameObject']['m_PathID']
        if not tree['m_Enabled'] or tree['m_IsTrigger'] or not sc.active(gid) or sc.gos[gid]['m_Layer']!=8:
            continue
        if kind=='EdgeCollider2D':
            offset=tree['m_Offset']
            paths=[[sc.point(gid,p['x']+offset['x'],p['y']+offset['y'])[:2] for p in tree['m_Points']]]
        else:
            # Terrain may legitimately have more than16 vertices; retain the
            # original boundary segments for the offline ground-ray calculation.
            offset=tree['m_Offset']
            if kind=='BoxCollider2D':
                x,y=tree['m_Size']['x']/2,tree['m_Size']['y']/2
                raw=[[{'x':a,'y':b}for a,b in [(-x,-y),(x,-y),(x,y),(-x,y)]]]
            else:raw=tree['m_Points']['m_Paths']
            paths=[[sc.point(gid,p['x']+offset['x'],p['y']+offset['y'])[:2] for p in path]for path in raw]
            paths=[path+path[:1]for path in paths]
        for path in paths:
            for a,b in zip(path,path[1:]):terrain.append((a,b,f'{file}:{index}'))
    resource=source.file('resources.assets')
    hero=next(o for o in resource.objects.values()if o.type.name=='MonoBehaviour'and source.typename(o)=='HeroController')
    fields=source.read(hero);game_object=source.ref(resource,fields['m_GameObject'])
    body=next(source.read(o) for o in (source.ref(resource,c['component']) for c in source.read(game_object)['m_Component'])if o.type.name=='BoxCollider2D')
    elevation=body['m_Size']['y']/2-body['m_Offset']['y']+0.01
    result=[]
    for index,(kind,tree)in sc.objects.items():
        if kind!='HazardRespawnTrigger' or not tree['m_Enabled']:continue
        gid=_local_id(tree['m_GameObject'])
        if not sc.active(gid):continue
        try:_checkpoint(sc,file,index,tree,gid,terrain,elevation,result)
        except (ValueError,KeyError) as error:
            # Whole-world cooks record a trigger the runtime cannot honour yet
            # instead of failing the scene; canonical callers still raise.
            if unsupported is None:raise
            unsupported.append({'source':f'{file}:{index}','error':str(error)})
    if len(result)>128:raise ValueError('checkpoint scene exceeds128 bounded trigger shapes')
    return result


def _checkpoint(sc,file,index,tree,gid,terrain,elevation,result):
    from breakables import _components, _local_id, collider_polygons
    if tree['fireOnce']:raise ValueError('one-shot HazardRespawnTrigger state not yet implemented')
    marker_id=_local_id(tree['respawnMarker'])
    if marker_id not in sc.objects:raise ValueError('hazard checkpoint references no marker')
    marker_type,marker=sc.objects[marker_id]
    if marker_type!='HazardRespawnMarker':raise ValueError('hazard checkpoint does not reference a marker')
    marker_gid=_local_id(marker['m_GameObject']);position=sc.point(marker_gid);hits=[]
    for a,b,collider_id in terrain:
        if a[0]==b[0] or not min(a[0],b[0])<=position[0]<=max(a[0],b[0]):continue
        y=a[1]+(position[0]-a[0])*(b[1]-a[1])/(b[0]-a[0])
        if 0<=position[1]-y<=50:hits.append((y,collider_id))
    if not hits:raise ValueError(f'checkpoint {file}:{index} has no source terrain within downward50 ray')
    ground,ground_source=max(hits)
    for collider_id,collider_type,collider in _components(sc,gid):
        if not collider_type.endswith('Collider2D') or not collider['m_Enabled']:continue
        if not collider['m_IsTrigger']:raise ValueError('HazardRespawnTrigger body is not a trigger')
        polygons=collider_polygons(sc,gid,collider_type,collider)
        points=[p for poly in polygons for p in poly]
        result.append({'source':f'{file}:{index}','marker_source':f'{file}:{marker_id}',
            'collider_source':f'{file}:{collider_id}','world_polygons':polygons,
            'bounds':[min(p[0]for p in points),min(p[1]for p in points),max(p[0]for p in points),max(p[1]for p in points)],
            'marker_position':position,'spawn':[position[0],ground+elevation],
            'ground_source':ground_source,'ground_y':ground,'ground_ray_distance':50,
            'respawn_facing_right':bool(marker['respawnFacingRight']),'fire_once':False,
            'projection':'initial source terrain; dynamic scripted terrain not evaluated'})


def postpack_checkpoints(report, source, scenes=None):
    """Attach authored trigger shapes without modifying cooked texture packs."""
    from scene import Scene
    scenes = {} if scenes is None else scenes
    by_scene = {}
    for info in report['scenes']:
        scene_id = info['scene_id']
        if scene_id not in scenes:
            scenes[scene_id] = Scene(source, info.get('file', info.get('scene_file')))
        unsupported = []
        by_scene[scene_id] = checkpoint_sources(scenes[scene_id], unsupported=unsupported)
        info['hazard_checkpoints'] = by_scene[scene_id]
        if unsupported:
            info['hazard_checkpoints_unsupported'] = unsupported
    for region in report['regions']:
        bounds = region.get('interaction_bounds', region['activation_bounds'])
        region['checkpoints'] = [record for record in by_scene[region['scene_id']]
            if bounds[0] <= record['bounds'][2] and record['bounds'][0] <= bounds[2]
            and bounds[1] <= record['bounds'][3] and record['bounds'][1] <= bounds[3]]
    report['checkpoint_policy'] = {
        'scope': 'Direct authored HazardRespawnTrigger links; hazard recovery only, not bench saves',
        'ground_projection': 'Initial source layer8 terrain, downward50, original HeroController body offset',
        'unsupported': ['FSM-assigned checkpoints', 'scripted terrain changes', 'one-shot triggers'],
        'unique_triggers': sum(len(records) for records in by_scene.values()),
    }


def postpack_masks(report, draw_sources=None):
    """Bind source masks wherever rendered, independently of owner collider bounds.

    Existing per-region Breakable metadata retains local bindings. Remote bindings
    reference the same scene-wide state bit and fade clock, with no hit/collision
    records copied into regions that do not contain the owner.
    """
    owners = {}
    for region in report['regions']:
        for record in region['breakables']:
            if not record.get('mask_fades'):continue
            key = (region['scene_id'], record['source'])
            canonical = {k:v for k,v in record.items() if k not in ('off_draws','on_draws','edge_indices','mask_fades')}
            fades = [{k:v for k,v in f.items() if k != 'draw_indices'} for f in record['mask_fades']]
            value = (canonical, fades)
            if key in owners and owners[key] != value:
                raise ValueError('inconsistent source mask owner metadata')
            owners[key] = value
    for region in report['regions']:
        chunk = region['chunk_id']
        if draw_sources is None:
            path = ROOT / f'data/regions/region-{chunk:03}/scene.json'
            draws = json.loads(path.read_text())['draws']
            sources = [d['source'] for d in draws]
        else:sources = draw_sources[chunk]
        if len(sources) != region['draws']:
            raise ValueError('mask provenance draw count differs from cooked region')
        local = {b['source'] for b in region['breakables']}
        bindings = []
        for (scene, source), (owner, fades) in sorted(owners.items()):
            if scene != region['scene_id'] or source in local:continue
            total_ticks = max(f['ticks_60hz'] for f in fades)
            for fade in fades:
                indices = [i for i,sid in enumerate(sources) if sid in fade['renderer_sources']]
                if indices:
                    bindings.append({'owner_source':source,'state_index':owner['state_index'],
                        'owner_fade_ticks':total_ticks,'fade':dict(fade,draw_indices=indices)})
        region['remote_mask_bindings'] = bindings
    report['mask_binding_policy'] = {
        'source_owners':len(owners),
        'remote_bindings':sum(len(r['remote_mask_bindings']) for r in report['regions']),
        'scope':'Authored Breakable HIT fades follow renderer residency; source hit shapes and collider ownership remain local',
    }


def locate_region(regions, scene_id, xy):
    """Catalogue slot (chunk_id - 1) of the region of `scene_id` containing `xy`, in cooked order."""
    for region in regions:
        b = region.get('activation_bounds')
        if b and region.get('scene_id') == scene_id and b[0] <= xy[0] <= b[2] and b[1] <= xy[1] <= b[3]:
            return region['chunk_id'] - 1
    return None


def resolve_gates(report, regions, scene):
    """Admitted TransitionPoints of one scene, resolved against the whole world.

    A gate names its destination by scene name and entry point, and its spawn
    lands in some region of that other scene, so this cannot be done inside the
    per-scene bank encoder. `generate` runs it once per scene and hangs the
    result on the scene, the way it already hands the encoder `statics`. The
    actor spec index went the other way: the encoder derives it from the same
    `scene_actor_bank` pass this module links, rather than reading back a
    number the report happened to be carrying.
    """
    by_name = {other['scene_name']: other for other in report['scenes']}
    integer(scene['scene_id'], SCENES, 'gate scene')
    resolved = []
    for gate in scene['gates']:
        if not gate['enabled'] or gate['target_scene'] not in by_name or 'trigger_bounds' not in gate:
            continue
        target = by_name[gate['target_scene']]
        matches = [g for g in target['gates'] if g['name'] == gate['entry_point']]
        if len(matches) != 1:
            continue
        destination = matches[0]
        xy = list(destination['position'][:2]); offset = destination.get('entry_offset', {})
        xy[0] += offset.get('x', 0); xy[1] += offset.get('y', 0)
        # Development placement inside the authored destination entry side;
        # the complete retail walk-in state machine is not yet implemented.
        if destination['name'].startswith('left'):
            xy[0] += 1
        elif destination['name'].startswith('right'):
            xy[0] -= 1
        target_region = locate_region(regions, target['scene_id'], xy)
        if target_region is None:
            continue
        # Source HeroController top entries (the Dirtmouth well is the verified
        # case) start the drop at -12 units per second; other sides keep rest.
        entry_vy = -12 * 65536 if destination['name'].startswith('top') else 0
        # TransitionPoint.TryDoTransition: side gates need the Knight facing
        # into them, a recoiling Knight is pushed back out instead, and the
        # collider of a delayed top gate turns on `collider_delay` seconds
        # after the scene starts. Source GatePosition: right 1, left 2.
        side = {'left': 1, 'right': 2, 'top': 3, 'bot': 4}.get(''.join(c for c in gate['name'].split(' ')[0] if c.isalpha()), 0)
        # A door is 5, and the guest enters it on UP with the hero body
        # rather than by walking a point through it, because that is what
        # its `Door Control` FSM does. Its collider is a strip of floor
        # about a quarter of a unit tall, so a point test taken at the
        # Knight's own origin passes over it and never enters.
        if gate.get('door_control') and side == 0:
            side = 5
        delay = gate.get('collider_delay', 0.)
        if not gate.get('collider_initially_enabled', True) and delay <= 0:
            raise ValueError('gate collider starts disabled without a Delay Collider FSM: ' + gate['source'])
        delay_ticks = integer(round(delay * 60), 65536, 'gate collider delay')
        resolved.append({'source': gate['source'], 'target_scene': target['scene_id'],
                         'target_region': target_region, 'bounds': gate['trigger_bounds'],
                         'spawn': xy, 'entry_vy': entry_vy, 'side': side, 'delay_ticks': delay_ticks})
    return resolved


def generate(report, out_path=None):
    """Write the guest region metadata. `out_path` diverts it for a dry run,
    so a catalogue change can be checked without touching the shared table."""
    regions = report['regions']
    if [r['chunk_id'] for r in regions] != list(range(1, len(regions) + 1)):
        raise ValueError('region chunks must be contiguous and start at one')
    if len(regions) > MAX_REGIONS:
        raise ValueError(f'region count exceeds the {MAX_REGIONS}-slot gate destination field')
    floors = flat_floor_catalog(report, root=ROOT)
    # The guest sizes its per-scene persistent state tables from the catalog.
    out = [f'pub const SCENES: usize = {SCENES};\n', rust_catalog(floors)]
    # Reveal-mask controllers are not linked: each scene's ride in its own
    # metadata bank, where the trigger polygon the guest tests against is one
    # of the bank's own polygons. Only the per-scene pool bound is checked
    # here; tools/world_metadata.py owns the rest of the contract now.
    reveal_scenes=report.get('reveal_mask_scenes',{})
    for scene in range(SCENES):
        records=reveal_scenes.get(str(scene),{}).get('controllers',[])
        if len(records)>16:raise ValueError('reveal scene pool exceeds16')
        for record in records:
            # A driven controller has no trigger of its own; the rest may have several.
            for polygon in record.get('triggers') or []:
                if not 3<=len(polygon)<=16 or any(len(point)!=2 for point in polygon):raise ValueError('reveal polygon bound')
    from effects import generated_debris
    debris_catalog={}
    for region in regions:
        key=generated_debris(region.get('door_debris',[]),region['scene_id'])
        if key not in debris_catalog:
            name=f'DOOR_DEBRIS_{len(debris_catalog)}'
            debris_catalog[key]=name
            out.append(f'static {name}: &[debris::Spec] = {key};')
    from effects import DEBRIS_VARIANT_LIMIT
    if len(debris_catalog)>DEBRIS_VARIANT_LIMIT:raise ValueError('door debris metadata variant budget')
    from particles import generated_style,generated_emitter
    particle_scenes={};particle_banks={}
    for region in regions:
        data=region.get('particle_effects')
        if not data:continue
        scene=region['scene_id']
        identity=(repr(data['styles']),repr(data['emitters']),repr(data['death_offset']))
        if scene not in particle_scenes:
            particle_scenes[scene]=identity
            out.append(f'static PARTICLE_STYLES_{scene}: &[particles::Style] = &['+','.join(generated_style(v)for v in data['styles'])+'];')
            out.append(f'static PARTICLE_EMITTERS_{scene}: &[particles::EmitterSpec] = &['+','.join(generated_emitter(v)for v in data['emitters'])+'];')
        elif particle_scenes[scene]!=identity:raise ValueError('particle scene identity changes across residency')
        frames=data['frames']
        if len(frames)!=2 or list(map(len,frames))!=[3,9]or any(len(v)!=4 or any(not 0<=f<2048 for f in v)for group in frames for v in group):raise ValueError('particle frame bank shape')
        key=(scene,repr(frames))
        if key not in particle_banks:
            name=f'PARTICLE_BANK_{len(particle_banks)}';particle_banks[key]=name
            arrays=['&['+','.join('['+','.join(map(str,v))+']'for v in group)+']'for group in frames]
            out.append(f'static {name}: particles::Bank = particles::Bank '+'{frames:['+','.join(arrays)+f'],styles:PARTICLE_STYLES_{scene},death_offset:'+str(data['death_offset'])+'};')
    if len(particle_banks)>8:raise ValueError('particle metadata variant budget')
    # One static per distinct actor *type* per scene. Where a placement stands
    # and how it was authored is not in here: that rides in the scene's own
    # metadata bank, which already carries an object per placement, and
    # tools/world_metadata.py stamps the index into this list from the same
    # `scene_actor_bank` pass rather than from anything recorded here.
    for scene in range(SCENES):
        specs, _ = scene_actor_bank([r for r in regions if r['scene_id'] == scene])
        out.append(f'static SCENE_ACTORS_{scene}: &[hk_sim::ActorSpec] = &[' + ','.join(specs) + '];')
    out.append('pub static SCENE_ACTORS: &[&[hk_sim::ActorSpec]] = &[' + ','.join(f'SCENE_ACTORS_{scene}' for scene in range(SCENES)) + '];')
    # Per-scene statics the runtime Region value borrows, and small variant
    # catalogues the bank's KIND_REGION_STATICS object indexes per region.
    scene_emitters = {}
    for region in regions:
        if region.get('particle_effects'):
            scene_emitters[region['scene_id']] = f'PARTICLE_EMITTERS_{region["scene_id"]}'
    out.append('pub static SCENE_EMITTERS: &[&[particles::EmitterSpec]] = &[' + ','.join(scene_emitters.get(scene, '&[]') for scene in range(SCENES)) + '];')
    out.append('pub static DEBRIS_CATALOG: &[&[debris::Spec]] = &[' + ','.join(debris_catalog.values()) + '];')
    out.append('pub static PARTICLE_BANKS: &[particles::Bank] = &[' + ','.join(particle_banks.values()) + '];')
    debris_index = {name: i for i, name in enumerate(debris_catalog.values())}
    bank_index = {name: i for i, name in enumerate(particle_banks.values())}
    impact_catalog = []
    from effects import generated_debris
    for region in regions:
        scene = integer(region['scene_id'], SCENES, 'scene index')
        impact = region.get('grass_impact')
        impact_spec = None if not impact else 'crate::impact::Spec {clips:[' + ','.join(str(integer(i, 128, 'impact clip')) for i in impact['clips']) + '],ticks:' + str(integer(impact['ticks'], 65536, 'impact duration')) + '}'
        if impact_spec is not None and impact_spec not in impact_catalog:
            impact_catalog.append(impact_spec)
        particle_data = region.get('particle_effects')
        region['statics'] = {
            'door_debris': debris_index[debris_catalog[generated_debris(region.get('door_debris', []), scene)]],
            'particle_bank': -1 if not particle_data else bank_index[particle_banks[(scene, repr(particle_data['frames']))]],
            'grass_impact': -1 if impact_spec is None else impact_catalog.index(impact_spec)}
    if len(impact_catalog) > 32:
        raise ValueError('grass impact metadata variant budget')
    out.append('pub static IMPACT_CATALOG: &[crate::impact::Spec] = &[' + ','.join(impact_catalog) + '];')
    # Each scene's camera tilemap size in tiles, which are units (GameManager.RefreshTilemapInfo):
    # the guest's camera clamps its centre to 14.6 .. width - 14.6 and 8.3 ..
    # height - 8.3, as CameraController does (host/regions.py postpack_camera).
    by_id={scene['scene_id']:scene for scene in report['scenes']}
    sizes=[]
    for scene in range(SCENES):
        tilemap=by_id[scene].get('camera_tilemap')
        if not tilemap:raise ValueError('scene without a cooked camera tilemap: '+str(scene))
        sizes.append(f"[{integer(tilemap['width'],512,'tilemap width')},{integer(tilemap['height'],512,'tilemap height')}]")
    out.append('pub static SCENE_CAMERA: &[[u16; 2]] = &[' + ','.join(sizes) + '];')
    # Scene owner per catalogue slot: the only per-view table left in the guest.
    out.append('pub static REGION_SCENES: &[u8] = &[' + ','.join(str(integer(r['scene_id'], min(SCENES, 256), 'scene index')) for r in regions) + '];')
    # Gates ride in each scene's metadata bank rather than a linked table; the
    # resolution is attached to the scene here because it needs every scene and
    # every region at once, which the per-scene encoder never sees.
    for scene in report['scenes']:
        scene['resolved_gates'] = resolve_gates(report, regions, scene)
    result = '\n'.join(out) + '\n'
    Path(out_path or ROOT/'data/regions.rs').write_text(result)
    return result


if __name__ == '__main__':
    report = json.loads((ROOT/'data/regions.json').read_text())
    text = generate(report)
    print(f'Generated {len(report["regions"])} regions, {len(text)} bytes of guest source metadata')
