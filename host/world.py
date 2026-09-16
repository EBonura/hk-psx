"""Generate bounded guest metadata from cooked region indices, never raw Unity."""
import json
import math
from source import ROOT
from actors import generated_actor_specs
from flat_support import generate as flat_floor_catalog, rust_catalog

from quality import SCENE_COUNT as SCENES
BREAKABLES_PER_SCENE = 128
GRASS_PER_SCENE = 1024


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


def persistent_object(record):
    # Full persistent flags survive the session's death/reset operation. Objects
    # without PersistentBoolItem, dontSave, and semiPersistent objects reset.
    return any(not p['dont_save'] and not p['semi_persistent'] for p in record.get('persistence', []))


def damagehero_respawns(hit_type):
    """DamageHero's serialized integer is NOT GlobalEnums.HazardType.

    HeroBox copies the integer unchanged into HeroController.TakeDamage's fourth
    argument. Installed TakeDamage calls StartRecoil for0/1, DieFromHazard for
    2(spikes)/3(acid), logs4(lava), and calls pit recovery for5 only in its normal
    damage branch. The latter two require separate handling, not this bool ABI.
    """
    if type(hit_type) is not int or hit_type not in (0, 1, 2, 3):
        raise ValueError(f'unsupported DamageHero hit type {hit_type}; lava/pit semantics require explicit handling')
    return hit_type in (2, 3)


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


def generate(report):
    regions = report['regions']
    if [r['chunk_id'] for r in regions] != list(range(1, len(regions) + 1)):
        raise ValueError('region chunks must be contiguous and start at one')
    if len(regions) > 1024:
        raise ValueError('region count exceeds the 1024-slot sanity bound')
    stable_breakables, stable_grass = {}, {}
    floors = flat_floor_catalog(report, root=ROOT)
    out = [rust_catalog(floors)]
    reveal_scenes=report.get('reveal_mask_scenes',{})
    for scene in range(SCENES):
        records=reveal_scenes.get(str(scene),{}).get('controllers',[])
        if len(records)>16:raise ValueError('reveal scene pool exceeds16')
        seen=set();specs=[]
        for index,record in enumerate(records):
            source=integer(record['source_id'],1<<32,'reveal source ID')
            if source in seen or record['controller']!=index:raise ValueError('reveal source aliases or noncanonical index')
            seen.add(source)
            polygon=record['trigger']
            if not 3<=len(polygon)<=16 or any(len(point)!=2 for point in polygon):raise ValueError('reveal polygon bound')
            ticks=integer(record['fade_ticks'],601,'reveal fade ticks')
            if not ticks:raise ValueError('reveal duration must be positive')
            opacity=record['initial_opacity']
            if type(opacity)is not int or opacity not in (0,128):raise ValueError('invalid reveal initial opacity')
            specs.append('crate::reveal_masks::RevealMaskSpec {source_id:'+str(source)+',trigger:&['+
                ','.join(arr(map(q,point))for point in polygon)+f'],fade_ticks:{ticks},initial_opacity:{opacity}'+'}')
        out.append(f'static REVEAL_MASKS_SCENE_{scene}: &[crate::reveal_masks::RevealMaskSpec] = &['+','.join(specs)+'];')
    from effects import generated_debris
    debris_catalog={}
    for region in regions:
        key=generated_debris(region.get('door_debris',[]),region['scene_id'])
        if key not in debris_catalog:
            name=f'DOOR_DEBRIS_{len(debris_catalog)}'
            debris_catalog[key]=name
            out.append(f'static {name}: &[debris::Spec] = {key};')
    if len(debris_catalog)>8:raise ValueError('door debris metadata variant budget')
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
    # One static per distinct actor spec; regions reference them, since the
    # scene actor bank gives every region of a scene the same clip indices.
    actor_statics = {}
    region_actor_refs = []
    for region in regions:
        refs = []
        for text in generated_actor_specs(region):
            key = (region['scene_id'], text)
            if key not in actor_statics:
                actor_statics[key] = f"ACTOR_S{region['scene_id']}_{len(actor_statics)}"
                out.append(f'static {actor_statics[key]}: hk_sim::ActorSpec = {text};')
            refs.append('&' + actor_statics[key])
        region_actor_refs.append('&[' + ','.join(refs) + ']')
    out.append('pub static REGIONS: &[Region] = &[')
    for region_index, region in enumerate(regions):
        scene = integer(region['scene_id'], SCENES, 'scene index')
        draw_count = region['draws']; edge_count = region['edges']
        def draw_indices(values):
            return ids(integer(v, draw_count, 'draw index') for v in values)
        patches = []
        for patch in region['grass']:
            state = integer(patch['state_index'], GRASS_PER_SCENE, 'grass state')
            stable = (scene, state)
            if stable_grass.setdefault(stable, patch['source']) != patch['source']:
                raise ValueError('grass state aliases different source objects')
            off = integer(patch['off_draw'], draw_count, 'grass off draw')
            on = integer(patch['on_draw'], draw_count, 'grass on draw')
            patches.append(f'Patch {{state:{scene*GRASS_PER_SCENE+state},grass:Grass {{bounds:' +
                           rect(patch['bounds']) + f',off_draw:{off},on_draw:{on}}}}}')
        props = []
        for record in region['breakables']:
            state = integer(record['state_index'], BREAKABLES_PER_SCENE, 'breakable state')
            if record.get('hit_points') != 1:
                raise ValueError('Breakable C# subset requires observed one-hit behavior')
            if record.get('scene_state_count', 0) > BREAKABLES_PER_SCENE:
                raise ValueError('authored Breakable scene state exceeds budget')
            stable = (scene, state)
            if stable_breakables.setdefault(stable, record['source']) != record['source']:
                raise ValueError('breakable state aliases different source objects')
            polygons = []
            for polygon in record['hit_polygons']:
                if not 3 <= len(polygon) <= 16 or any(len(point) != 2 for point in polygon):
                    raise ValueError('Breakable polygon exceeds 3..16 vertex bound')
                polygons.append('&[' + ','.join(arr(map(q, point)) for point in polygon) + ']')
            if not 1 <= len(polygons) <= 8:
                raise ValueError('Breakable polygon count exceeds 1..8 bound')
            fades = []; fade_ticks = 0
            for fade in record.get('mask_fades', []):
                ticks = integer(fade['ticks_60hz'], 601, 'mask fade ticks')
                if not ticks or fade['target_alpha'] != 0 or fade['ease'] != 'linear':
                    raise ValueError('mask fade requires positive-duration linear fade to zero')
                if any(renderer['initial_alpha'] != 1 for renderer in fade['renderers']):
                    raise ValueError('mask fade requires observed full initial alpha')
                fade_ticks = max(fade_ticks, ticks)
                fades.append('MaskFade {draws:' + draw_indices(fade.get('draw_indices', [])) + f',ticks:{ticks}' + '}')
            edge_indices = ids(integer(v, edge_count, 'breakable edge index') for v in record['edge_indices'])
            # Play the resident source sample only when the authored table points
            # to that clip. Several poles use the same clip as the tutorial doors.
            options = record.get('audio', {}).get('options', [])
            sound = len(options) == 1 and options[0]['name'] == 'breakable_wall_hit_1'
            props.append(f'Breakable {{id:{scene*BREAKABLES_PER_SCENE+state},bounds:' + rect(record['box']) +
                         ',polygons:&[' + ','.join(polygons) + '],off:' + draw_indices(record['off_draws']) +
                         ',on:' + draw_indices(record['on_draws']) + ',edges:' + edge_indices +
                         ',fades:&[' + ','.join(fades) + f'],fade_ticks:{fade_ticks},persistent:' +
                         str(persistent_object(record)).lower() + ',door_sound:' + str(sound).lower() + '}')
        remote_masks = []
        for binding in region.get('remote_mask_bindings', []):
            state = integer(binding['state_index'], BREAKABLES_PER_SCENE, 'mask owner state')
            stable = (scene, state)
            if stable_breakables.setdefault(stable, binding['owner_source']) != binding['owner_source']:
                raise ValueError('mask state aliases different source objects')
            fade = binding['fade']
            ticks = integer(fade['ticks_60hz'], 601, 'mask fade ticks')
            total = integer(binding['owner_fade_ticks'], 601, 'mask owner fade ticks')
            if not ticks or total < ticks or fade['target_alpha'] != 0 or fade['ease'] != 'linear':
                raise ValueError('invalid remote mask fade')
            if any(r['initial_alpha'] != 1 for r in fade['renderers']):
                raise ValueError('remote mask requires observed full initial alpha')
            remote_masks.append(f'MaskBinding {{owner:{scene*BREAKABLES_PER_SCENE+state},fade_ticks:{total},fade:MaskFade {{draws:' +
                draw_indices(fade['draw_indices']) + f',ticks:{ticks}' + '}}')
        hazards = []
        for hazard in region['hazards']:
            damage = integer(hazard['damage'], 65536, 'hazard damage')
            polygons = []
            for polygon in hazard['world_polygons']:
                if not 3 <= len(polygon) <= 16 or any(len(point) != 2 for point in polygon):
                    raise ValueError('hazard polygon exceeds 3..16 vertex bound')
                polygons.append('&[' + ','.join(arr(map(q, point)) for point in polygon) + ']')
            if not 1 <= len(polygons) <= 8:
                raise ValueError('hazard polygon count exceeds 1..8 bound')
            hazards.append('Hazard {bounds:' + rect(hazard['bounds']) + ',polygons:&[' +
                           ','.join(polygons) + f'],origin_x:{q(hazard["position"][0])},damage:{damage},hazard:' +
                           str(damagehero_respawns(hazard['hazard_type'])).lower() + '}')
        neighbours = [integer(n-1, len(regions), 'neighbour region') for n in region.get('neighbour_chunks', [])]
        checkpoints=[]
        for checkpoint in region.get('checkpoints',[]):
            if checkpoint['fire_once']:raise ValueError('one-shot checkpoint unsupported')
            polygons=[]
            for polygon in checkpoint['world_polygons']:
                if not 3<=len(polygon)<=16:raise ValueError('checkpoint polygon bound')
                polygons.append('&['+','.join(arr(map(q,point)) for point in polygon)+']')
            if not 1<=len(polygons)<=8:raise ValueError('checkpoint polygon count')
            checkpoints.append('Checkpoint {bounds:'+rect(checkpoint['bounds'])+',polygons:&['+
                ','.join(polygons)+'],spawn:'+arr(map(q,checkpoint['spawn']))+
                ',facing:'+('1'if checkpoint['respawn_facing_right']else'-1')+'}')
        reveal_bindings=[];reveal_owned=set()
        for binding in region.get('reveal_mask_bindings',[]):
            controller=integer(binding['controller'],len(reveal_scenes.get(str(scene),{}).get('controllers',[])),'reveal controller')
            draw=integer(binding['draw'],draw_count,'reveal draw')
            if draw in reveal_owned:raise ValueError('multiple reveal owners for draw')
            reveal_owned.add(draw)
            reveal_bindings.append(f'crate::reveal_masks::RevealMaskBinding {{controller:{controller},draw:{draw}}}')
        from effects import generated_debris
        debris_spec=debris_catalog[generated_debris(region.get("door_debris",[]),scene)]
        particle_data=region.get('particle_effects')
        particle_bank='None' if not particle_data else 'Some('+particle_banks[(scene,repr(particle_data['frames']))]+')'
        particle_emitters='&[]' if not particle_data else f'PARTICLE_EMITTERS_{scene}'
        actors = region_actor_refs[region_index]
        impact=region.get('grass_impact')
        impact_spec='None' if not impact else 'Some(crate::impact::Spec {clips:['+','.join(str(integer(i,128,'impact clip'))for i in impact['clips'])+'],ticks:'+str(integer(impact['ticks'],65536,'impact duration'))+'})'
        out.append(f'Region {{global_id:{region["chunk_id"]},scene:{scene},flat_floor_heights:FLAT_FLOOR_HEIGHTS[{region["chunk_id"]-1}],bounds:' + rect(region['activation_bounds']) +
                   ',collision_bounds:' + rect(region.get('collision_bounds',region['activation_bounds'])) +
                   ',particle_bank:'+particle_bank+',grass_emitters:'+particle_emitters+',door_debris:'+debris_spec+',camera:' + rect(region['camera_bounds']) + ',grass_impact:'+impact_spec+',grass:&[' + ','.join(patches) +
                   '],breakables:&[' + ','.join(props) + '],remote_masks:&[' + ','.join(remote_masks) + '],hazards:&[' + ','.join(hazards) +
                   '],reveal_masks:REVEAL_MASKS_SCENE_'+str(scene)+',reveal_bindings:&['+','.join(reveal_bindings)+'],checkpoints:&[' + ','.join(checkpoints) + '],actors:' + actors + ',neighbours:' + ids(neighbours) + '},')
    out.append('];\npub static GATES: &[Gate] = &[')
    by_name = {scene['scene_name']: scene for scene in report['scenes']}
    for scene in report['scenes']:
        integer(scene['scene_id'], SCENES, 'gate scene')
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
            if not any(r['scene_id'] == target['scene_id'] and
                       r['activation_bounds'][0] <= xy[0] <= r['activation_bounds'][2] and
                       r['activation_bounds'][1] <= xy[1] <= r['activation_bounds'][3] for r in regions):
                continue
            # Source HeroController top entries (the Dirtmouth well is the verified
            # case) start the drop at -12 units per second; other sides keep rest.
            entry_vy = -12 * 65536 if destination['name'].startswith('top') else 0
            out.append(f'Gate {{scene:{scene["scene_id"]},target_scene:{target["scene_id"]},bounds:' +
                       rect(gate['trigger_bounds']) + ',spawn:' + arr(map(q, xy)) + f',entry_vy:{entry_vy}' + '},')
    out.append('];')
    result = '\n'.join(out) + '\n'
    (ROOT/'data/regions.rs').write_text(result)
    return result


if __name__ == '__main__':
    report = json.loads((ROOT/'data/regions.json').read_text())
    text = generate(report)
    print(f'Generated {len(report["regions"])} regions, {len(text)} bytes of guest source metadata')
