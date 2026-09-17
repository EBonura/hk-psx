"""Strict original Crawler/Runner corpse art, separate from particle emulation."""
from pathlib import Path
from breakables import _components

def corpse_source(source, scene, actor):
    """Admit only the verified Crawler, Zombie Runner, Climber or Buzzer corpse.

    Runner source evidence: .hkpsx/runner71/corpse-source.json. Its zero spawn
    offset, 0.2 bounce and Single/Once clips differ from the original Crawler
    contract; never broaden the Crawler validator to accept arbitrary variants.
    Crawler admission remains structural: source IDs may differ between bundles.
    The Buzzer corpse (.hkpsx/vengefly/CONTRACT.md) is a breaker without
    ObjectBounce: it leaves on landing.
    """
    kind=actor.get('movement_control',{}).get('kind')
    if kind=='Vengefly':return _breaker_corpse_source(source,scene,actor)
    if kind not in ('WalkLeftRight','ZombieSwipeWalker','Climber'):
        raise ValueError('unvalidated corpse controller family')
    runner=kind=='ZombieSwipeWalker';climber=kind=='Climber'
    expected_offset={'x':0.,'y':0. if (runner or climber) else .5,'z':0.}
    expected_bounce=.2 if runner else .45 if climber else .3
    # Climber corpse (.hkpsx/climber70/references.json): Death Air loops eight
    # frames at 30 fps, Death Land holds three frames at 15 fps.
    expected_timing=[(30,6,1),(12,2,8)] if runner else [(30,0,8),(15,2,3)] if climber else [(12,2,3),(12,2,2)]
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1:raise ValueError('actor needs exactly one EnemyDeathEffects')
    death=deaths[0]
    if any(death[k] for k in ('isCorpseRecyclable','corpseFacesRight','lowCorpseArc','rotateCorpse')) or death['corpseFlingSpeed']!=(20 if climber else 15):
        raise ValueError('unsupported corpse launch')
    offset=death['corpseSpawnPoint']
    if offset!=expected_offset:raise ValueError('unsupported corpse spawn offset')
    obj=source.ref(scene.file,death['corpsePrefab']);go=source.read(obj)
    if runner and source.sid(obj)!='sharedassets37.assets:61':raise ValueError('unvalidated corpse prefab for controller')
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);typ=source.typename(component)
        if typ in parts:raise ValueError('duplicate corpse component')
        parts[typ]=(component,source.read(component))
    required=('Corpse','tk2dSprite','tk2dSpriteAnimator','Rigidbody2D','BoxCollider2D','ObjectBounce','Transform')
    if any(k not in parts for k in required):raise ValueError('incomplete corpse prefab')
    corpse=parts['Corpse'][1];body=parts['Rigidbody2D'][1];bounce=parts['ObjectBounce'][1]
    # resetRotation only matters for a rotated owner; the guest corpse is drawn
    # upright, which is what the reset produces for the Climber.
    special=('breaker','bigBreaker','chunker','deathStun','fungusExplode','goopExplode','hatcher','instantChunker','massless','spineBurst','zomHive')+(() if climber else ('resetRotation',))
    if any(corpse[k]for k in special):
        raise ValueError('unsupported special corpse')
    if corpse['landEffects']['m_PathID']:raise ValueError('unsupported additional corpse land effects')
    if body['m_BodyType']!=0 or body['m_LinearDamping']!=0 or abs(body['m_GravityScale']-.8)>1e-6 or body['m_Constraints']!=4:
        raise ValueError('unsupported corpse body')
    if abs(bounce['bounceFactor']-expected_bounce)>1e-6 or bounce['speedThreshold']!=1 or any(bounce[k]for k in ('playSound','playAnimationOnBounce','sendFSMEvent')):
        raise ValueError('unsupported corpse bounce')
    sprite=parts['tk2dSprite'][1];anim=parts['tk2dSpriteAnimator'][1];transform=parts['Transform'][1]
    if sprite['_color']!={'r':1.0,'g':1.0,'b':1.0,'a':1.0} or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0} or transform['m_LocalScale']!={'x':1.0,'y':1.0,'z':1.0}:
        raise ValueError('unsupported corpse scale/color')
    library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    selected=[next(c for c in library['clips']if c['name']==name)for name in ('Death Air','Death Land')]
    if runner and source.sid(library_o)!='sharedassets37.assets:149':raise ValueError('unvalidated corpse animation library')
    if [(c['fps'],c['wrapMode'],len(c['frames']))for c in selected]!=expected_timing:raise ValueError('unvalidated corpse clip timing')
    if any(c.get('loopStart',0)!=0 or any(f.get('triggerEvent',False)for f in c['frames'])for c in selected):
        raise ValueError('unsupported corpse animation events/loop start')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    box=parts['BoxCollider2D'][1];off=box['m_Offset'];size=box['m_Size']
    if runner:
        if go['m_Name']!='Corpse Zombie Basic One' or not death['m_Enabled'] or death['playerDataName']!='ZombieRunner':
            raise ValueError('unvalidated Runner corpse identity')
        if any(abs(matrix[i][j]-(1 if i==j else 0))>1e-6 for i in range(2)for j in range(2)):
            raise ValueError('unsupported Runner corpse actor transform')
        if not box['m_Enabled'] or box['m_IsTrigger'] or box['m_EdgeRadius']!=0 or off!={'x':.03125,'y':-.7265625} or size!={'x':1.21875,'y':1.265625}:
            raise ValueError('unsupported Runner corpse collider')
        if not body['m_Simulated'] or body['m_CollisionDetection']!=0 or not corpse['m_Enabled'] or not bounce['m_Enabled']:
            raise ValueError('unsupported Runner corpse component activation')
        if not anim['m_Enabled'] or not anim['playAutomatically'] or anim['isRealtime'] or library['clips'][anim['defaultClipId']]['name']!='Death Air':
            raise ValueError('unsupported Runner corpse animator startup')
        material_o=source.ref(obj.assets_file,box['m_Material']);material=source.read(material_o)
        if source.sid(material_o)!='resources.assets:1073' or abs(material['friction']-.2)>1e-6 or material['bounciness']!=0:
            raise ValueError('unsupported Runner corpse physics material')
    bounds=[(off['x']-size['x']/2)*sx,(off['y']-size['y']/2)*sy,(off['x']+size['x']/2)*sx,(off['y']+size['y']/2)*sy]
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':selected,'scale':[sx,sy],'bounds':[round(v*65536)for v in bounds],
            'spawn_offset':[round(offset['x']*65536),round(offset['y']*65536)], 'gravity':48*65536,'breaker':False,'fling_speed':round(death['corpseFlingSpeed']*65536),'bounce_factor':round(expected_bounce*65536),'land_delay_ticks':60,
            'limitations':['Corpse Steam/Flame and infected wave/spatter remain separate particle/effect work',
                            'Fixed-point terrain solver is not complete Box2D; bounce RNG is deterministic per source, not Unity global RNG']}

def _breaker_corpse_source(source,scene,actor):
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1:raise ValueError('actor needs exactly one EnemyDeathEffects')
    death=deaths[0]
    if any(death[k] for k in ('isCorpseRecyclable','corpseFacesRight','lowCorpseArc','rotateCorpse')) or death['corpseFlingSpeed']!=15 \
            or death['corpseSpawnPoint']!={'x':0.,'y':0.,'z':0.}:
        raise ValueError('unsupported corpse launch')
    obj=source.ref(scene.file,death['corpsePrefab']);go=source.read(obj)
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);typ=source.typename(component)
        if typ in parts:raise ValueError('duplicate corpse component')
        parts[typ]=(component,source.read(component))
    if any(k not in parts for k in ('Corpse','tk2dSprite','tk2dSpriteAnimator','Rigidbody2D','BoxCollider2D','Transform')) or 'ObjectBounce' in parts:
        raise ValueError('unsupported breaker corpse prefab')
    corpse=parts['Corpse'][1];body=parts['Rigidbody2D'][1]
    if not corpse['breaker'] or any(corpse[k] for k in ('bigBreaker','chunker','deathStun','fungusExplode','goopExplode','hatcher','instantChunker','massless','spineBurst','zomHive','resetRotation')):
        raise ValueError('unsupported special corpse')
    if corpse['landEffects']['m_PathID']:raise ValueError('unsupported additional corpse land effects')
    if body['m_BodyType']!=0 or body['m_LinearDamping']!=0 or abs(body['m_GravityScale']-.7)>1e-6 or body['m_Constraints']!=4:
        raise ValueError('unsupported corpse body')
    sprite=parts['tk2dSprite'][1];transform=parts['Transform'][1]
    if sprite['_color']!={'r':1.0,'g':1.0,'b':1.0,'a':1.0} or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0} or transform['m_LocalScale']!={'x':1.0,'y':1.0,'z':1.0}:
        raise ValueError('unsupported corpse scale/color')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    box=parts['BoxCollider2D'][1];off=box['m_Offset'];size=box['m_Size']
    if not box['m_Enabled'] or box['m_IsTrigger'] or box['m_EdgeRadius']!=0:raise ValueError('unsupported breaker corpse collider')
    bounds=[(off['x']-size['x']/2)*sx,(off['y']-size['y']/2)*sy,(off['x']+size['x']/2)*sx,(off['y']+size['y']/2)*sy]
    anim=parts['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    selected=[next(c for c in library['clips']if c['name']==name)for name in ('Death Air','Death Land')]
    if [(c['fps'],c['wrapMode'],len(c['frames']))for c in selected]!=[(12,2,3),(12,2,3)]:raise ValueError('unvalidated corpse clip timing')
    if any(c.get('loopStart',0)!=0 or any(f.get('triggerEvent',False)for f in c['frames'])for c in selected):
        raise ValueError('unsupported corpse animation events/loop start')
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':selected,'scale':[sx,sy],'bounds':[round(v*65536)for v in bounds],
            'spawn_offset':[0,0],'gravity':42*65536,'breaker':True,'fling_speed':15*65536,'bounce_factor':0,'land_delay_ticks':0,
            'limitations':['Break pieces, spatter and infected wave on landing are not presented; the corpse is removed',
                           'Fixed-point terrain solver is not complete Box2D']}

def append_corpse_art(source,scene,actors,atlas,frames,clips):
    from cook import tk_sprite,FOCAL,CAM_Z
    textures={};collections={};images={};cooked={}
    for actor in actors:
        if not actor['movement_supported']:continue
        record=corpse_source(source,scene,actor);library_o=record.pop('library_object');selected=record.pop('clips');sx,sy=record['scale']
        for kind,clip in zip(('air','land'),selected):
            key=(record['library'],clip['name'],sx,sy)
            if key not in cooked:
                start=len(frames)
                for frame in clip['frames']:
                    co=source.ref(library_o.assets_file,frame['spriteCollection']);sid=source.sid(co)
                    if sid not in collections:collections[sid]=source.read(co)
                    image_key=(sid,frame['spriteId'],sx,sy)
                    if image_key not in images:
                        image,box=tk_sprite(source,co.assets_file,collections[sid],frame['spriteId'],textures)
                        box=(box[0]*sx,box[1]*sy,box[2]*sx,box[3]*sy)
                        texture=atlas.add(image,(box[2]-box[0])*FOCAL/-CAM_Z,(box[3]-box[1])*FOCAL/-CAM_Z,streamed=True)
                        images[image_key]=(texture,box)
                    texture,box=images[image_key]
                    frames.append({'texture':texture,'box':box,'sprite':f'{sid}:{frame["spriteId"]}','event':frame})
                cooked[key]=len(clips)
                from cook import guest_wrap
                clips.append({'name':f'{record["library"]}/{clip["name"]}','start':start,'count':len(clip['frames']),'fps':clip['fps'],'wrap':guest_wrap(clip),'loopStart':clip.get('loopStart',0)})
            record[kind+'_clip']=cooked[key]
        actor['corpse']=record

def generated_corpse(record):
    if record is None:return 'None'
    if any(k not in record for k in ('air_clip','land_clip','bounds','spawn_offset','bounce_factor')):raise ValueError('corpse source missing cooked art/geometry')
    if type(record['bounce_factor']) is not int or not 0<=record['bounce_factor']<=65536:
        raise ValueError('corpse bounce factor outside Q16 range')
    fields={k:record[k]for k in ('air_clip','land_clip','bounce_factor')}
    fields['fling_speed']=record.get('fling_speed',15*65536)
    fields['gravity']=record.get('gravity',48*65536)
    fields['breaker']=str(bool(record.get('breaker',False))).lower()
    fields.update({k:'['+','.join(map(str,record[k]))+']'for k in ('bounds','spawn_offset')})
    return 'Some(hk_sim::CorpseSpec {'+','.join(f'{k}:{v}'for k,v in fields.items())+'})'

def append_grass_impact_art(source,scene,atlas,frames,clips):
    """Original shared Slash Impact clip family; static so bursts do not pin LRU slots."""
    from cook import tk_sprite,FOCAL,CAM_Z
    # Resolve through this scene's GrassCut, not an unrelated replacement prefab.
    grasses=[d for typ,d in scene.objects.values()if typ=='GrassCut'and d['m_Enabled']]
    if not grasses:return None
    refs={source.sid(source.ref(scene.file,d['cutEffectPrefab']))for d in grasses if d['cutEffectPrefab']['m_PathID']}
    if len(refs)!=1:raise ValueError('unvalidated mixed GrassCut impact prefabs')
    obj=source.ref(scene.file,next(d['cutEffectPrefab']for d in grasses if d['cutEffectPrefab']['m_PathID']))
    go=source.read(obj);components={}
    for ref in go['m_Component']:
        co=source.ref(obj.assets_file,ref['component']);components[source.typename(co)]=(co,source.read(co))
    if source.sid(obj)!='resources.assets:4006':raise ValueError('unvalidated grass impact source')
    animator=components['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,animator['library']);library=source.read(library_o)
    color=components['tk2dSprite'][1]['_color']
    if color!={'r':1.,'g':1.,'b':1.,'a':1.}:raise ValueError('unsupported impact tint')
    textures={};collections={};images={};result=[]
    for name in ('Slash Impact 2','Slash Impact 1'):
        clip=next(c for c in library['clips']if c['name']==name)
        if (clip['fps'],clip['wrapMode'],len(clip['frames']))!=(30,2,5):raise ValueError('unvalidated impact clip duration')
        start=len(frames)
        for frame in clip['frames']:
            co=source.ref(library_o.assets_file,frame['spriteCollection']);sid=source.sid(co)
            if sid not in collections:collections[sid]=source.read(co)
            key=(sid,frame['spriteId'])
            if key not in images:
                image,box=tk_sprite(source,co.assets_file,collections[sid],frame['spriteId'],textures)
                # GrassCut source prefab scaleX0.6, scaleY1; sign applied on spawn.
                box=(box[0]*.6,box[1],box[2]*.6,box[3])
                tex=atlas.add(image,(box[2]-box[0])*FOCAL/-CAM_Z,(box[3]-box[1])*FOCAL/-CAM_Z,streamed=False)
                images[key]=(tex,box)
            tex,box=images[key];frames.append({'texture':tex,'box':box,'sprite':f'{sid}:{frame["spriteId"]}','event':frame})
        result.append(len(clips));clips.append({'name':f'{source.sid(library_o)}/{name}','start':start,'count':5,'fps':30,'wrap':2,'loopStart':0})
    return {'source':source.sid(obj),'clips':result,'ticks':10,'timeout_ticks':11,
            'limitations':['Variant choice uses deterministic source-object seed, not pooled Unity Chooser RNG',
                            'Serialized Anim1 name contains trailing newline; original clip family is preserved without claiming PlayMaker scheduling parity',
                            'GrassCut particles and bending remain separate work']}

# Exact observed Tutorial door family. Other Breakable debris stays unsupported.
DOOR_SPRITES={'resources.assets:3261','resources.assets:3639','resources.assets:3745','resources.assets:2451'}
def polygon_moment(points):
    """Mass-one uniform polygon centroid/moment; not native Rigidbody2D inertia."""
    area=cx=cy=moment=0.
    if not 3<=len(points)<=16:raise ValueError('door collider vertex budget')
    for (x,y),(u,v) in zip(points,points[1:]+points[:1]):
        cross=x*v-u*y;area+=cross/2;cx+=(x+u)*cross/6;cy+=(y+v)*cross/6
        moment+=cross*(x*x+x*u+u*u+y*y+y*v+v*v)/12
    if abs(area)<1e-8:raise ValueError('degenerate door polygon')
    center=[cx/area,cy/area];inertia=moment/area-sum(x*x for x in center)
    if inertia<=0:raise ValueError('invalid door polygon inertia')
    return center,inertia

def append_door_debris_art(source,scene,atlas,frames):
    """Original four rigid sprites, present in every same-scene residency bank."""
    from cook import native_sprite,FOCAL,CAM_Z
    from breakables import breakable_sources
    import math
    if Path(scene.file.name).name!='level6':return []
    textures={};records=[];errors=[]
    for door in breakable_sources(scene,errors=errors):
        selected=[]
        for part in door['debris']:
            gid=int(part['game_object'].split(':')[-1]);components={typ:(i,d)for i,typ,d in _components(scene,gid)}
            if 'SpinSelf'not in components:continue
            required=('SpriteRenderer','Rigidbody2D','PolygonCollider2D','ObjectBounce','Transform')
            if any(k not in components for k in required):raise ValueError('incomplete physical door fragment')
            rid,renderer=components['SpriteRenderer'];obj=source.ref(scene.file,renderer['m_Sprite']);sid=source.sid(obj)
            if sid not in DOOR_SPRITES:raise ValueError('unrecognized spinning Breakable sprite')
            body=components['Rigidbody2D'][1];bounce=components['ObjectBounce'][1];spin=components['SpinSelf'][1]
            if body['m_BodyType']!=0 or body['m_Mass']!=1 or body['m_UseAutoMass'] or body['m_LinearDamping']!=0 or abs(body['m_AngularDamping']-.05)>1e-6 or body['m_GravityScale']!=1 or body['m_Constraints']!=0 or spin['spinFactor']!=4:
                raise ValueError('unsupported door rigid body')
            if bounce['bounceFactor']!=.5 or bounce['speedThreshold']!=1:raise ValueError('unsupported door bounce')
            if renderer['m_Color']!={'r':1.,'g':1.,'b':1.,'a':1.} or renderer['m_FlipX'] or renderer['m_FlipY']:
                raise ValueError('unsupported door sprite color/reflection')
            matrix=scene.world(scene.go_transform[gid])
            if abs(matrix[0][1])>1e-6 or abs(matrix[1][0])>1e-6 or not all(.5<=abs(matrix[i][i])<=2 for i in range(2)):
                raise ValueError('unsupported door world scale/rotation')
            collider=components['PolygonCollider2D'][1];paths=collider['m_Points']['m_Paths'];offset=collider['m_Offset']
            if len(paths)!=1:raise ValueError('unsupported composite door collider')
            points=[[p['x']+offset['x'],p['y']+offset['y']]for p in paths[0]]
            center,_=polygon_moment(points)
            sx,sy=matrix[0][0],matrix[1][1]
            _,inertia=polygon_moment([[x*sx,y*sy]for x,y in points])
            key=(sid,sx,sy)
            if key not in textures:
                image,box=native_sprite(obj)
                tex=atlas.add(image,(box[2]-box[0])*abs(sx)*FOCAL/-CAM_Z,(box[3]-box[1])*abs(sy)*FOCAL/-CAM_Z,streamed=False)
                textures[key]=len(frames)
                frames.append({'texture':tex,'box':box,'sprite':sid,'event':{'source':'Breakable debris/SpinSelf'}})
            selected.append({'source':f'level6:{rid}','game_object':part['game_object'],'sprite':sid,'door_source':door['source'],
                'door_state':door['state_index'],'frame':textures[key],'scale':[round(sx*65536),round(sy*65536)],'origin':[round(v*65536)for v in part['position'][:2]],
                'centroid':[round(v*65536)for v in center],'polygon':[[round(v*65536)for v in p]for p in points],
                'inertia_estimate':inertia,'torque':round((4/50)*(180/math.pi)/inertia*65536),'angle_offset':round(door['angle_offset']),
                'z':part['position'][2],'order':renderer['m_SortingOrder'],'layer':renderer['m_SortingLayer'],
                'limitations':['Uniform-polygon inertia and 50Hz approximation are not verified Unity native physics',
                    'Source ObjectBounce cache/threshold/reflection and sleep are implemented; contact manifold/inertia/friction remain fixed-point native-physics approximations',
                    'No authored lifetime; persistent per-scene pool, deterministic RNG differs from Unity']})
        if selected:
            if len(selected)!=4 or {p['sprite']for p in selected}!=DOOR_SPRITES or door['fling_speed']!=[10.,17.] or door['angle_offset']!=-60:
                raise ValueError('unvalidated door fragment family/launch')
            records.extend(selected)
    if len(records)!=28:raise ValueError(f'expected 28 exact Tutorial door fragments, got {len(records)}')
    # Stable identity independent of texture/region ordering; draw original Z order.
    indices={p['source']:i for i,p in enumerate(sorted(records,key=lambda p:int(p['source'].split(':')[-1])))}
    for p in records:p['id']=indices[p['source']]
    return sorted(records,key=lambda p:(-p['z'],p['layer'],p['order'],p['source']))

def generated_debris(records,scene):
    if len(records)>28:raise ValueError('door fragment pool budget')
    result=[];seen=set()
    for p in records:
        index=p['id']
        if not isinstance(index,int) or not 0<=index<28 or index in seen:raise ValueError('door fragment identity')
        seen.add(index)
        if not 0<=p['door_state']<128 or not 0<=p['frame']<65536:raise ValueError('door fragment owner/frame')
        if not 3<=len(p['polygon'])<=16 or not 0<p['torque']<2**31:raise ValueError('door fragment geometry')
        def array(values):return '['+','.join(map(str,values))+']'
        fields={'id':index,'door':scene*128+p['door_state'],'frame':p['frame'],'source':int(p['source'].split(':')[-1]),
                'origin':array(p['origin']),'centroid':array(p['centroid']),'scale':array(p['scale']),'polygon':'&['+','.join(array(v)for v in p['polygon'])+']',
                'torque':p['torque'],'angle_offset':p['angle_offset']}
        result.append('debris::Spec {'+','.join(f'{k}:{v}'for k,v in fields.items())+'}')
    return '&['+','.join(result)+']'
