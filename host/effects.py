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
    if kind in ('Vengefly','Gruzzer'):return _breaker_corpse_source(source,scene,actor,kind)
    if kind=='Baldur':return _roller_corpse_source(source,scene,actor)
    if kind=='Aspid':return _aspid_corpse_source(source,scene,actor)
    if kind=='EggSac':return _egg_sac_corpse_source(source,scene,actor)
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
    # Runner, Barger and Hornhead corpses (Corpse Zombie Basic One/Three/Five)
    # share one structure; identity is checked below by name and player data.
    if runner and not (go['m_Name'].startswith('Corpse Zombie Basic ') or go['m_Name']=='Corpse Zombie Leaper'):raise ValueError('unvalidated corpse prefab for controller')
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
    if runner and source.sid(library_o)!=actor['movement_control'].get('library_source'):raise ValueError('corpse animation library differs from the actor library')
    if [(c['fps'],c['wrapMode'],len(c['frames']))for c in selected]!=expected_timing:raise ValueError('unvalidated corpse clip timing')
    if any(c.get('loopStart',0)!=0 or any(f.get('triggerEvent',False)for f in c['frames'])for c in selected):
        raise ValueError('unsupported corpse animation events/loop start')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    box=parts['BoxCollider2D'][1];off=box['m_Offset'];size=box['m_Size']
    if runner:
        if not death['m_Enabled'] or death['playerDataName'] not in ('ZombieRunner','ZombieBarger','ZombieHornhead','ZombieLeaper'):
            raise ValueError('unvalidated Runner corpse identity')
        # A mirrored placement is admitted (Crossroads_37's Leaper), so accept a
        # plain x mirror here too. sx/sy above already take the magnitude, and
        # the corpse's own facing comes from the actor at spawn, so only the
        # basis shape needs checking: unit magnitude, no rotation, no y flip.
        if (abs(sx-1)>1e-6 or abs(matrix[1][1]-1)>1e-6
                or abs(matrix[0][1])>1e-6 or abs(matrix[1][0])>1e-6):
            raise ValueError('unsupported Runner corpse actor transform')
        if not box['m_Enabled'] or box['m_IsTrigger'] or box['m_EdgeRadius']!=0 or not (0<size['x']<4 and 0<size['y']<4):
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

def _breaker_corpse_source(source,scene,actor,kind):
    # Buzzer: smashes on the first landing, no bounce. Gruzzer (Corpse Fly):
    # fling 20, ObjectBounce 0.7, smashes on its third landing, free rotation.
    gruzzer=kind=='Gruzzer'
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1:raise ValueError('actor needs exactly one EnemyDeathEffects')
    death=deaths[0]
    if any(death[k] for k in ('isCorpseRecyclable','corpseFacesRight','lowCorpseArc','rotateCorpse')) or death['corpseFlingSpeed']!=(20 if gruzzer else 15) \
            or death['corpseSpawnPoint']!={'x':0.,'y':0.,'z':0.}:
        raise ValueError('unsupported corpse launch')
    obj=source.ref(scene.file,death['corpsePrefab']);go=source.read(obj)
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);typ=source.typename(component)
        if typ in parts:raise ValueError('duplicate corpse component')
        parts[typ]=(component,source.read(component))
    if any(k not in parts for k in ('Corpse','tk2dSprite','tk2dSpriteAnimator','Rigidbody2D','BoxCollider2D','Transform')) or ('ObjectBounce' in parts)!=gruzzer:
        raise ValueError('unsupported breaker corpse prefab')
    corpse=parts['Corpse'][1];body=parts['Rigidbody2D'][1]
    if not corpse['breaker'] or corpse['smashBounces']!=(3 if gruzzer else 0) or any(corpse[k] for k in ('bigBreaker','chunker','deathStun','fungusExplode','goopExplode','hatcher','instantChunker','massless','spineBurst','zomHive','resetRotation')):
        raise ValueError('unsupported special corpse')
    if corpse['landEffects']['m_PathID']:raise ValueError('unsupported additional corpse land effects')
    if body['m_BodyType']!=0 or body['m_LinearDamping']!=0 or abs(body['m_GravityScale']-.7)>1e-6 or body['m_Constraints']!=(0 if gruzzer else 4):
        raise ValueError('unsupported corpse body')
    bounce=0.
    if gruzzer:
        ob=parts['ObjectBounce'][1]
        if abs(ob['bounceFactor']-.7)>1e-6 or ob['speedThreshold']!=1 or any(ob[k] for k in ('playSound','playAnimationOnBounce','sendFSMEvent')):
            raise ValueError('unsupported corpse bounce')
        bounce=.7
    sprite=parts['tk2dSprite'][1];transform=parts['Transform'][1]
    if sprite['_color']!={'r':1.0,'g':1.0,'b':1.0,'a':1.0} or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0} or transform['m_LocalScale']!={'x':1.0,'y':1.0,'z':1.0}:
        raise ValueError('unsupported corpse scale/color')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    box=parts['BoxCollider2D'][1];off=box['m_Offset'];size=box['m_Size']
    if not box['m_Enabled'] or box['m_IsTrigger'] or box['m_EdgeRadius']!=0:raise ValueError('unsupported breaker corpse collider')
    bounds=[(off['x']-size['x']/2)*sx,(off['y']-size['y']/2)*sy,(off['x']+size['x']/2)*sx,(off['y']+size['y']/2)*sy]
    anim=parts['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    selected=[next(c for c in library['clips']if c['name']==name)for name in ('Death Air','Death Land')]
    if [(c['fps'],c['wrapMode'],len(c['frames']))for c in selected]!=([(12,2,2),(15,2,6)] if gruzzer else [(12,2,3),(12,2,3)]):raise ValueError('unvalidated corpse clip timing')
    if any(c.get('loopStart',0)!=0 or any(f.get('triggerEvent',False)for f in c['frames'])for c in selected):
        raise ValueError('unsupported corpse animation events/loop start')
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':selected,'scale':[sx,sy],'bounds':[round(v*65536)for v in bounds],
            'spawn_offset':[0,0],'gravity':42*65536,'breaker':True,'smash_bounces':3 if gruzzer else 0,'fling_speed':round(death['corpseFlingSpeed']*65536),'bounce_factor':round(bounce*65536),'land_delay_ticks':0,
            'limitations':['Break pieces, spatter and infected wave on landing are not presented; the corpse is removed',
                           'Fixed-point terrain solver is not complete Box2D']}

def _roller_corpse_source(source,scene,actor):
    """`Corpse Roller Spawned` (.hkpsx/baldur/CONTRACT.md): a rolling circle body
    with 0.7 linear damping whose FSM shrinks and destroys it once it slows."""
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1:raise ValueError('actor needs exactly one EnemyDeathEffects')
    death=deaths[0]
    if any(death[k] for k in ('isCorpseRecyclable','corpseFacesRight','lowCorpseArc','rotateCorpse')) or death['corpseFlingSpeed']!=15 \
            or death['corpseSpawnPoint']['x']!=0 or abs(death['corpseSpawnPoint']['y']-.2)>1e-6:
        raise ValueError('unsupported corpse launch')
    obj=source.ref(scene.file,death['corpsePrefab']);go=source.read(obj)
    if go['m_Name']!='Corpse Roller Spawned':raise ValueError('unvalidated Roller corpse identity')
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);typ=source.typename(component)
        if typ in parts:raise ValueError('duplicate corpse component')
        parts[typ]=(component,source.read(component))
    # No Corpse component: the `corpse` FSM owns landing, shrink and destroy.
    if any(k not in parts for k in ('tk2dSprite','tk2dSpriteAnimator','Rigidbody2D','CircleCollider2D','Transform','PlayMakerFSM')) or 'ObjectBounce' in parts or 'Corpse' in parts:
        raise ValueError('unsupported Roller corpse prefab')
    body=parts['Rigidbody2D'][1]
    if body['m_BodyType']!=0 or abs(body['m_LinearDamping']-.7)>1e-6 or abs(body['m_GravityScale']-.8)>1e-6 or body['m_Constraints']!=0:
        raise ValueError('unsupported corpse body')
    fsm=parts['PlayMakerFSM'][1]['fsm']
    if fsm['name']!='corpse' or [st['name'] for st in fsm['states']]!=['Initiate','In Air','Landed','Shrink','Flame Check','Start Flame','Destroy']:
        raise ValueError('unsupported Roller corpse FSM')
    sprite=parts['tk2dSprite'][1];transform=parts['Transform'][1]
    if sprite['_color']!={'r':1.0,'g':1.0,'b':1.0,'a':1.0} or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0} or transform['m_LocalScale']!={'x':1.0,'y':1.0,'z':1.0}:
        raise ValueError('unsupported corpse scale/color')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    circle=parts['CircleCollider2D'][1];off=circle['m_Offset'];r=circle['m_Radius']
    if circle['m_IsTrigger'] or abs(r-.43)>1e-6 or off['x']!=0 or abs(off['y']+.2)>1e-6:raise ValueError('unsupported Roller corpse collider')
    bounds=[(off['x']-r)*sx,(off['y']-r)*sy,(off['x']+r)*sx,(off['y']+r)*sy]
    anim=parts['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    selected=[next(c for c in library['clips']if c['name']==name)for name in ('Death Air','Death Land')]
    if [(c['fps'],c['wrapMode'],len(c['frames']))for c in selected]!=[(12,2,3),(30,6,1)]:raise ValueError('unvalidated corpse clip timing')
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':selected,'scale':[sx,sy],'bounds':[round(v*65536)for v in bounds],
            'spawn_offset':[0,round(.2*65536)],'gravity':48*65536,'breaker':False,'smash_bounces':0,'remove_after_land':120,'fling_speed':15*65536,'bounce_factor':0,'land_delay_ticks':60,
            'limitations':['Circle body as its bounding box; the solver .2 slide replaces 0.7 linear damping; no spin',
                           'Removed 120 ticks after landing instead of the source slow-down/shrink/destroy sequence']}

def _egg_sac_corpse_source(source,scene,actor):
    """`Corpse Egg Sac`: no Rigidbody2D, no collider and no Corpse component.

    Its `Control` FSM is the whole lifetime: Spit plays one clip and waits, then
    Burst plays a second to completion and End deactivates the object. Nothing
    launches or falls, so this is the non-physics corpse form (`hold_ticks`)
    rather than the flung body every other family uses.
    """
    from combat import ticks
    from focus import action_fields
    def scalar(value):
        if isinstance(value,dict):
            if value.get('useVariable'):raise ValueError('dynamic Egg Sac corpse parameter')
            return value['value']
        return value
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1:raise ValueError('actor needs exactly one EnemyDeathEffects')
    death=deaths[0]
    # rotateCorpse is set here, but it only copies the owner's z rotation onto a
    # corpse the guest draws upright; there is no launch arc to rotate.
    if not death['m_Enabled'] or any(death[k] for k in ('isCorpseRecyclable','corpseFacesRight','lowCorpseArc','recycle')) \
            or death['corpseFlingSpeed']!=0 or death['corpseSpawnPoint']!={'x':0.,'y':0.,'z':0.} \
            or death['enemyDeathType']!=0 or death['playerDataName']!='EggSac':
        raise ValueError('unsupported Egg Sac corpse launch')
    obj=source.ref(scene.file,death['corpsePrefab']);go=source.read(obj)
    if go['m_Name']!='Corpse Egg Sac':raise ValueError('unvalidated Egg Sac corpse identity')
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);typ=source.typename(component)
        if typ in parts:raise ValueError('duplicate corpse component')
        parts[typ]=(component,source.read(component))
    if sorted(parts)!=sorted(('Transform','MeshFilter','MeshRenderer','tk2dSprite','tk2dSpriteAnimator',
                              'SetZ','PlayMakerFSM','AudioSource','PreInstantiateGameObject')):
        raise ValueError('unsupported Egg Sac corpse prefab')
    sprite=parts['tk2dSprite'][1];transform=parts['Transform'][1]
    if sprite['_color']!={'r':1.0,'g':1.0,'b':1.0,'a':1.0} or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0} or transform['m_LocalScale']!={'x':1.0,'y':1.0,'z':1.0}:
        raise ValueError('unsupported corpse scale/color')
    fsm=parts['PlayMakerFSM'][1]['fsm'];states={st['name']:st for st in fsm['states']}
    if fsm['name']!='Control' or fsm['startState']!='Init' or [st['name'] for st in fsm['states']]!=['Init','Spit','Burst','End'] \
            or [(t['fsmEvent']['name'],t['toState']) for st in fsm['states'] for t in st['transitions']]!=[('FINISHED','Spit'),('FINISHED','Burst'),('FINISHED','End')]:
        raise ValueError('unsupported Egg Sac corpse FSM')
    def only(state,action):
        data=states[state]['actionData']
        found=[i for i,n in enumerate(data['actionNames']) if n.rsplit('.',1)[-1]==action and data['actionEnabled'][i]]
        if len(found)!=1:raise ValueError(f'unsupported Egg Sac corpse action {state}/{action}')
        return action_fields(data,found[0])
    wait=only('Spit','Wait');hold=scalar(wait['time'])
    if wait['realTime'] or scalar(wait['finishEvent'])!='FINISHED' or not 0<hold<=10:
        raise ValueError('unsupported Egg Sac corpse hold')
    burst=only('Burst','Tk2dPlayAnimationWithEvents')
    if scalar(burst['animationCompleteEvent'])!='FINISHED' or scalar(burst['animationTriggerEvent']):
        raise ValueError('unsupported Egg Sac corpse burst completion')
    names=[scalar(only('Spit','Tk2dPlayAnimation')['clipName']),scalar(burst['clipName'])]
    # The Shiny the burst releases is recorded, not presented: its item resolves
    # through the ordinary Shiny Control routing, which nothing spawns yet.
    trinket=only('Init','SetFsmInt')
    if scalar(trinket['fsmName'])!='Shiny Control' or scalar(trinket['variableName'])!='Trinket Num':
        raise ValueError('unsupported Egg Sac corpse drop')
    anim=parts['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    if source.sid(library_o)!=actor['movement_control']['library_source']:
        raise ValueError('Egg Sac corpse animation library differs from the actor library')
    selected=[next(c for c in library['clips'] if c['name']==name) for name in names]
    if [(c['fps'],c['wrapMode'],len(c['frames'])) for c in selected]!=[(12,1,4),(18,2,4)]:
        raise ValueError('unvalidated Egg Sac corpse clip timing')
    if selected[0].get('loopStart',0)!=1 or selected[1].get('loopStart',0)!=0 \
            or any(f.get('triggerEvent',False) for c in selected for f in c['frames']):
        raise ValueError('unsupported Egg Sac corpse animation events/loop start')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    burst_seconds=len(selected[1]['frames'])/selected[1]['fps']
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':selected,'scale':[sx,sy],
            'bounds':[0,0,0,0],'spawn_offset':[0,0],'gravity':0,'breaker':False,'smash_bounces':0,'fling_speed':0,'bounce_factor':0,
            'hold_ticks':ticks(hold),'remove_after_land':ticks(burst_seconds),'shiny_trinket_num':scalar(trinket['setValue']),
            'limitations':['No rigid body or collider: the corpse holds the first clip in place and is removed when the second completes',
                           'The Shiny it releases is recorded as its Trinket Num and never spawned; the EnemyKillShake, one shot, looping audio and particles are not presented']}

class _ClipCooker:
    """Append tk2d clips from any library into the region frame/clip tables,
    sharing identical (library, clip, scale) entries."""
    def __init__(self,source,atlas,frames,clips):
        self.source=source;self.atlas=atlas;self.frames=frames;self.clips=clips
        self.textures={};self.collections={};self.images={};self.cooked={}
    def clip(self,library_sid,library_o,clip,sx,sy):
        from cook import tk_sprite,FOCAL,CAM_Z,guest_wrap
        source=self.source
        key=(library_sid,clip['name'],sx,sy)
        if key not in self.cooked:
            start=len(self.frames)
            for frame in clip['frames']:
                co=source.ref(library_o.assets_file,frame['spriteCollection']);sid=source.sid(co)
                if sid not in self.collections:self.collections[sid]=source.read(co)
                image_key=(sid,frame['spriteId'],sx,sy)
                if image_key not in self.images:
                    image,box=tk_sprite(source,co.assets_file,self.collections[sid],frame['spriteId'],self.textures)
                    box=(box[0]*sx,box[1]*sy,box[2]*sx,box[3]*sy)
                    texture=self.atlas.add(image,(box[2]-box[0])*FOCAL/-CAM_Z,(box[3]-box[1])*FOCAL/-CAM_Z,streamed=True)
                    self.images[image_key]=(texture,box)
                texture,box=self.images[image_key]
                self.frames.append({'texture':texture,'box':box,'sprite':f'{sid}:{frame["spriteId"]}','event':frame})
            self.cooked[key]=len(self.clips)
            self.clips.append({'name':f'{library_sid}/{clip["name"]}','start':start,'count':len(clip['frames']),'fps':clip['fps'],'wrap':guest_wrap(clip),'loopStart':clip.get('loopStart',0)})
        return self.cooked[key]

def _aspid_corpse_source(source,scene,actor):
    """`Corpse Spitter`: gravity .7, no collider (falls out of the scene). The
    guest removes it on its first landing instead."""
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1:raise ValueError('actor needs exactly one EnemyDeathEffects')
    death=deaths[0]
    if any(death[k] for k in ('isCorpseRecyclable','corpseFacesRight','lowCorpseArc','rotateCorpse')) or death['corpseFlingSpeed']!=15 \
            or death['corpseSpawnPoint']!={'x':0.,'y':0.,'z':0.}:
        raise ValueError('unsupported corpse launch')
    obj=source.ref(scene.file,death['corpsePrefab']);go=source.read(obj)
    if go['m_Name']!='Corpse Spitter':raise ValueError('unvalidated Aspid corpse identity')
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);typ=source.typename(component)
        if typ in parts:raise ValueError('duplicate corpse component')
        parts[typ]=(component,source.read(component))
    if any(k not in parts for k in ('Corpse','tk2dSprite','tk2dSpriteAnimator','Rigidbody2D','Transform')) or any(k in parts for k in ('BoxCollider2D','CircleCollider2D','ObjectBounce')):
        raise ValueError('unsupported Aspid corpse prefab')
    corpse=parts['Corpse'][1];body=parts['Rigidbody2D'][1]
    # `massless` is the source flag for a corpse without a collider.
    if not corpse['massless'] or any(corpse[k] for k in ('breaker','bigBreaker','chunker','deathStun','fungusExplode','goopExplode','hatcher','instantChunker','spineBurst','zomHive','resetRotation')):
        raise ValueError('unsupported special corpse')
    if body['m_BodyType']!=0 or body['m_LinearDamping']!=0 or abs(body['m_GravityScale']-.7)>1e-6 or body['m_Constraints']!=4:
        raise ValueError('unsupported corpse body')
    sprite=parts['tk2dSprite'][1];transform=parts['Transform'][1]
    if sprite['_color']!={'r':1.0,'g':1.0,'b':1.0,'a':1.0} or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0} or transform['m_LocalScale']!={'x':1.0,'y':1.0,'z':1.0}:
        raise ValueError('unsupported corpse scale/color')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    anim=parts['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    air=next(c for c in library['clips'] if c['name']=='Death Air')
    if (air['fps'],air['wrapMode'],len(air['frames']))!=(12,2,6):raise ValueError('unvalidated corpse clip timing')
    # Half-unit box standing in for the missing collider.
    bounds=[round(-.5*sx*65536),round(-.5*sy*65536),round(.5*sx*65536),round(.5*sy*65536)]
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':[air,air],'scale':[sx,sy],'bounds':bounds,
            'spawn_offset':[0,0],'gravity':42*65536,'breaker':True,'smash_bounces':0,'remove_after_land':0,'fling_speed':15*65536,'bounce_factor':0,'land_delay_ticks':0,
            'limitations':['No source collider: removed on the first landing instead of falling out of the scene']}

def append_corpse_art(source,scene,actors,atlas,frames,clips):
    cooker=_ClipCooker(source,atlas,frames,clips)
    for actor in actors:
        if not actor['movement_supported']:continue
        record=corpse_source(source,scene,actor);library_o=record.pop('library_object');selected=record.pop('clips');sx,sy=record['scale']
        for kind,clip in zip(('air','land'),selected):
            record[kind+'_clip']=cooker.clip(record['library'],library_o,clip,sx,sy)
        actor['corpse']=record
    append_shot_art(source,actors,cooker)

def append_shot_art(source,actors,cooker):
    """Aspid `Spitter Shot R` Idle/Impact clips at the prefab scale times the
    EnemyBullet scale; the source stretch and rotation are not cooked."""
    for actor in actors:
        control=actor.get('movement_control',{})
        if not actor['movement_supported'] or control.get('kind')!='Aspid':continue
        shot=control['shot'];library_o=shot.pop('library_object');library=source.read(library_o)
        scale=shot['scale']
        for kind,name in (('shot','Idle'),('impact','Impact')):
            clip=next(c for c in library['clips'] if c['name']==name)
            actor[kind+'_clip']=cooker.clip(shot['library'],library_o,clip,scale,scale)

def generated_corpse(record):
    if record is None:return 'None'
    if any(k not in record for k in ('air_clip','land_clip','bounds','spawn_offset','bounce_factor')):raise ValueError('corpse source missing cooked art/geometry')
    if type(record['bounce_factor']) is not int or not 0<=record['bounce_factor']<=65536:
        raise ValueError('corpse bounce factor outside Q16 range')
    fields={k:record[k]for k in ('air_clip','land_clip','bounce_factor')}
    fields['fling_speed']=record.get('fling_speed',15*65536)
    fields['gravity']=record.get('gravity',48*65536)
    fields['breaker']=str(bool(record.get('breaker',False))).lower()
    fields['smash_bounces']=int(record.get('smash_bounces',0))
    fields['remove_after_land']=int(record.get('remove_after_land',0))
    fields['hold_ticks']=int(record.get('hold_ticks',0))
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
