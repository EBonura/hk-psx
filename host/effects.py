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
    # A Gruz Mother reserve fly is the Gruzzer itself, `Corpse Fly` and all.
    if kind=='GruzzerReserve':kind='Gruzzer'
    if kind in ('Vengefly','Gruzzer','Mosquito'):return _breaker_corpse_source(source,scene,actor,kind)
    if kind=='Baldur':return _roller_corpse_source(source,scene,actor)
    if kind=='Aspid':return _aspid_corpse_source(source,scene,actor)
    if kind=='AcidFlyer':return _acid_flyer_corpse_source(source,scene,actor)
    if kind=='EggSac':return _egg_sac_corpse_source(source,scene,actor)
    if kind=='HuskGuard':return _guard_corpse_source(source,scene,actor)
    # A controller whose recognizer states it leaves no corpse is answered, not
    # refused. The False Knight is admitted invincible because Check Health
    # restores its body rather than letting it die, so nothing of it ever falls.
    if actor.get('movement_control',{}).get('no_corpse'):
        return None
    if kind not in ('WalkLeftRight','ZombieSwipeWalker','Climber','ZombieShield','MossWalker'):
        raise ValueError('unvalidated corpse controller family')
    # The Shield's EnemyDeathEffects is the conventional Zombie corpse, so it is
    # read against the Runner's expectations rather than its own. If its bounce
    # or spawn offset turn out to differ, the checks below name the field and
    # the value, which is the point of validating rather than trusting the name.
    runner=kind in ('ZombieSwipeWalker','ZombieShield');climber=kind=='Climber'
    # `Corpse Moss Crawler`: the Crawler's launch and offset, bounce 0.2, a
    # four-frame looping Death Air. resetRotation is moot: only unrotated
    # floor placements are admitted.
    moss=kind=='MossWalker'
    expected_offset={'x':0.,'y':0. if (runner or climber) else .5,'z':0.}
    expected_bounce=.2 if runner or moss else .45 if climber else .3
    # Climber corpse (.hkpsx/climber70/references.json): Death Air loops eight
    # frames at 30 fps, Death Land holds three frames at 15 fps.
    expected_timing=[(30,6,1),(12,2,8)] if runner else [(30,0,8),(15,2,3)] if climber else [(12,0,4),(12,2,2)] if moss else [(12,2,3),(12,2,2)]
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
    # The Shield's own corpse is the same structure under its own name, which is
    # why it reads against the Runner's expectations at all.
    if runner and not (go['m_Name'].startswith('Corpse Zombie Basic ')
                       or go['m_Name'] in ('Corpse Zombie Leaper','Corpse Zombie Shield')):
        raise ValueError(f'unvalidated corpse prefab for controller: {kind} spawns {go["m_Name"]!r}')
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
    special=('breaker','bigBreaker','chunker','deathStun','fungusExplode','goopExplode','hatcher','instantChunker','massless','spineBurst','zomHive')+(() if climber or moss else ('resetRotation',))
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
        if not death['m_Enabled'] or death['playerDataName'] not in ('ZombieRunner','ZombieBarger','ZombieHornhead','ZombieLeaper','ZombieShield'):
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
    # Mosquito (`Corpse Mosquito v2`): the Buzzer's shape with its own clips;
    # a breaker never shows Death Land, whose timing differs.
    gruzzer=kind=='Gruzzer';mosquito=kind=='Mosquito'
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
    if [(c['fps'],c['wrapMode'],len(c['frames']))for c in selected]!=([(12,2,2),(15,2,6)] if gruzzer else [(12,2,3),(30,2,1)] if mosquito else [(12,2,3),(12,2,3)]):raise ValueError('unvalidated corpse clip timing')
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

def _guard_corpse_source(source,scene,actor):
    """`Corpse Zombie Guard`: its own FSM, no Corpse component.

    `Death Stun` holds for `Wait` 1 s with velocity zeroed, then `Stun End`
    plays `Death Air` until the body lands and `Landed` plays `Death Land`,
    which stays. It is spawned where the guard stood and never flung (the
    stun zeroes the launch), so it is the hold form: Death Stun for the wait,
    then Death Land. The fall of Death Air, the thrown club and the steam are
    not presented.
    """
    from combat import ticks
    from focus import action_fields
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1 or deaths[0]['playerDataName']!='ZombieGuard' or deaths[0]['corpseSpawnPoint']!={'x':0.,'y':0.,'z':0.}:
        raise ValueError('unsupported Husk Guard death')
    obj=source.ref(scene.file,deaths[0]['corpsePrefab']);go=source.read(obj)
    if go['m_Name']!='Corpse Zombie Guard':raise ValueError('unvalidated Husk Guard corpse identity')
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);parts[source.typename(component)]=(component,source.read(component))
    fsm=parts['PlayMakerFSM'][1]['fsm'];states={st['name']:st for st in fsm['states']}
    data=states['Death Stun']['actionData']
    waits=[action_fields(data,i) for i,n in enumerate(data['actionNames']) if n.endswith('.Wait') and data['actionEnabled'][i]]
    if len(waits)!=1 or waits[0]['time']['useVariable']:raise ValueError('unsupported Husk Guard corpse stun')
    anim=parts['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    if source.sid(library_o)!=actor['movement_control']['library_source']:
        raise ValueError('Husk Guard corpse library differs from the actor library')
    if library['clips'][anim['defaultClipId']]['name']!='Death Stun':raise ValueError('unsupported Husk Guard corpse start')
    selected=[next(c for c in library['clips'] if c['name']==name) for name in ('Death Stun','Death Land')]
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':selected,'scale':[sx,sy],
            'bounds':[0,0,0,0],'spawn_offset':[0,0],'gravity':0,'breaker':False,'smash_bounces':0,'fling_speed':0,'bounce_factor':0,
            'hold_ticks':ticks(waits[0]['time']['value']),'remove_after_land':0,'tiled':True,
            'limitations':['Hold form: Death Stun for the source wait, then Death Land held; the Death Air fall, the thrown club and the steam are not presented']}

def append_guard_art(source,actors,cooker):
    """The Husk Guard's pooled `Shockwave Spurt` and `Slam Effect R`, cooked
    into its own scene's bank at unit scale."""
    for actor in actors:
        control=actor.get('movement_control',{})
        if not actor['movement_supported'] or control.get('kind')!='HuskGuard':continue
        for kind,ref in control['extra_art'].items():
            library_o=source.file(ref['file']).objects[ref['path_id']];library=source.read(library_o)
            clip=next(c for c in library['clips'] if c['name']==ref['clip'])
            actor[kind+'_clip']=cooker.clip(source.sid(library_o),library_o,clip,1.,1.,tiled=True)

class _ClipCooker:
    """Append tk2d clips from any library into the region frame/clip tables,
    sharing identical (library, clip, scale) entries."""
    def __init__(self,source,atlas,frames,clips):
        self.source=source;self.atlas=atlas;self.frames=frames;self.clips=clips
        self.textures={};self.collections={};self.images={};self.cooked={}
    def clip(self,library_sid,library_o,clip,sx,sy,tiled=False):
        """`tiled` admits a frame wider or taller than one animation slot
        (Atlas.add_tiled); a frame inside one slot cooks the same either way."""
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
                    w,h=(box[2]-box[0])*FOCAL/-CAM_Z,(box[3]-box[1])*FOCAL/-CAM_Z
                    texture=self.atlas.add_tiled(image,w,h) if tiled else self.atlas.add(image,w,h,streamed=True)
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

def _acid_flyer_corpse_source(source,scene,actor):
    """`Corpse Acid Fly`: no Corpse component and an empty `corpse` FSM, so
    nothing lands it, smashes it or removes it. It is a permanent physics prop:
    a 0.91 circle, gravity 0.8, ObjectBounce 0.6, spun once by SpinSelfSimple,
    holding Death Air for good."""
    components=list(_components(scene,actor['game_object']))
    deaths=[d for _,typ,d in components if typ=='EnemyDeathEffects']
    if len(deaths)!=1:raise ValueError('actor needs exactly one EnemyDeathEffects')
    death=deaths[0]
    if any(death[k] for k in ('isCorpseRecyclable','corpseFacesRight','lowCorpseArc','rotateCorpse')) or death['corpseFlingSpeed']!=20 \
            or death['corpseSpawnPoint']!={'x':0.,'y':.5,'z':0.}:
        raise ValueError('unsupported corpse launch')
    obj=source.ref(scene.file,death['corpsePrefab']);go=source.read(obj)
    if go['m_Name']!='Corpse Acid Fly':raise ValueError('unvalidated Acid Flyer corpse identity')
    parts={}
    for ref in go['m_Component']:
        component=source.ref(obj.assets_file,ref['component']);typ=source.typename(component)
        if typ in parts:raise ValueError('duplicate corpse component')
        parts[typ]=(component,source.read(component))
    if any(k not in parts for k in ('tk2dSprite','tk2dSpriteAnimator','Rigidbody2D','CircleCollider2D','ObjectBounce','Transform','PlayMakerFSM')) \
            or any(k in parts for k in ('Corpse','BoxCollider2D')):
        raise ValueError('unsupported Acid Flyer corpse prefab')
    fsm=parts['PlayMakerFSM'][1]['fsm']
    if [(st['name'],len(st['transitions']),len(st['actionData']['actionNames'])) for st in fsm['states']]!=[('State 1',0,0)]:
        raise ValueError('unsupported Acid Flyer corpse FSM')
    body=parts['Rigidbody2D'][1];ob=parts['ObjectBounce'][1]
    if body['m_BodyType']!=0 or body['m_LinearDamping']!=0 or abs(body['m_GravityScale']-.8)>1e-6:
        raise ValueError('unsupported corpse body')
    if abs(ob['bounceFactor']-.6)>1e-6 or ob['speedThreshold']!=1 or any(ob[k] for k in ('playSound','playAnimationOnBounce','sendFSMEvent')):
        raise ValueError('unsupported corpse bounce')
    sprite=parts['tk2dSprite'][1];transform=parts['Transform'][1]
    if sprite['_color']!={'r':1.0,'g':1.0,'b':1.0,'a':1.0} or sprite['_scale']!={'x':1.0,'y':1.0,'z':1.0} or transform['m_LocalScale']!={'x':1.0,'y':1.0,'z':1.0}:
        raise ValueError('unsupported corpse scale/color')
    matrix=scene.world(scene.go_transform[actor['game_object']]);sx=abs(matrix[0][0]);sy=abs(matrix[1][1])
    circle=parts['CircleCollider2D'][1];off=circle['m_Offset'];r=circle['m_Radius']
    if circle['m_IsTrigger'] or abs(r-.91)>1e-6 or abs(off['x']+.08)>1e-6 or abs(off['y']+.09)>1e-6:raise ValueError('unsupported Acid Flyer corpse collider')
    bounds=[(off['x']-r)*sx,(off['y']-r)*sy,(off['x']+r)*sx,(off['y']+r)*sy]
    anim=parts['tk2dSpriteAnimator'][1];library_o=source.ref(obj.assets_file,anim['library']);library=source.read(library_o)
    air=next(c for c in library['clips'] if c['name']=='Death Air')
    if (air['fps'],air['wrapMode'],len(air['frames']))!=(30,6,1) or anim['defaultClipId']!=library['clips'].index(air) or not anim['playAutomatically']:
        raise ValueError('unvalidated corpse clip timing')
    return {'source':source.sid(obj),'library':source.sid(library_o),'library_object':library_o,'clips':[air,air],'scale':[sx,sy],'bounds':[round(v*65536)for v in bounds],
            'spawn_offset':[0,round(.5*65536)],'gravity':48*65536,'breaker':False,'smash_bounces':0,'remove_after_land':0,'fling_speed':20*65536,'bounce_factor':round(.6*65536),'land_delay_ticks':0,
            'limitations':['Circle body as its bounding box with no spin; it stays where it comes to rest, as the source never removes it',
                           'Corpse Steam is not presented']}

def append_corpse_art(source,scene,actors,atlas,frames,clips):
    cooker=_ClipCooker(source,atlas,frames,clips)
    for actor in actors:
        if not actor['movement_supported']:continue
        record=corpse_source(source,scene,actor)
        if record is None:continue
        library_o=record.pop('library_object');selected=record.pop('clips');sx,sy=record['scale']
        tiled=record.pop('tiled',False)
        for kind,clip in zip(('air','land'),selected):
            record[kind+'_clip']=cooker.clip(record['library'],library_o,clip,sx,sy,tiled)
        actor['corpse']=record
    append_shot_art(source,actors,cooker)
    append_guard_art(source,actors,cooker)

def append_shot_art(source,actors,cooker):
    """Pooled-projectile Idle/Impact clips at the prefab scale times the
    EnemyBullet scale; the source stretch and rotation are not cooked.

    Two families share this path because they share the pool: the Aspid's
    `Spitter Shot R` and the Blocker's `Shot Mawlek`. Both carry the same two
    clip names at the same timings, and each recognizer has already refused a
    prefab whose body or box the pool would get wrong."""
    for actor in actors:
        control=actor.get('movement_control',{})
        if not actor['movement_supported'] or control.get('kind')not in('Aspid','Blocker'):continue
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

# Breakable.Break's rigid fling, as a shared definition rather than the four
# Tutorial door sprites it was first written against. Every fragment in the 45
# admitted scenes flings at flingSpeed 10..17 and angleOffset -60, so those are
# the contract, not the room. breakables.rigid_fragment owns the body, bounce
# and spin side; this file owns the art and the solver's per-instance geometry.
FLING_SPEED=[10.,17.]
FLING_ANGLE_OFFSET=-60
# One scene's fragments share the runtime pool in game/src/debris.rs, and a
# Spec's id is its slot in it, so the pool is the limit on how many of a scene's
# fragments can exist at once. Two of the 32 scenes that author fragments hold
# more than the pool (Crossroads_ShamanTemple 57, Crossroads_07 33); the
# overflow is a recorded refusal rather than a silently shortened list.
FRAGMENT_POOL=28
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

def fragment_polygon(components):
    """The fragment's own collider as local points, box or polygon alike."""
    kind=next(typ for typ in components if typ.endswith('Collider2D'))
    collider=components[kind][1]
    offset=collider['m_Offset']
    if kind=='BoxCollider2D':
        x,y=collider['m_Size']['x']/2,collider['m_Size']['y']/2
        paths=[[(-x,-y),(x,-y),(x,y),(-x,y)]]
    elif kind=='PolygonCollider2D':
        paths=[[(p['x'],p['y'])for p in path]for path in collider['m_Points']['m_Paths']]
    else:
        raise ValueError(f'unsupported rigid fragment collider {kind}')
    if len(paths)!=1:raise ValueError('unsupported composite fragment collider')
    return [[x+offset['x'],y+offset['y']]for x,y in paths[0]]


def fragment_records(source,scene,atlas,frames,refusals=None):
    """Every rigid Breakable fragment this scene authors, with its solver geometry.

    Reproduces Breakable.Break's FlingObject path for any instance matching the
    shared definition in breakables.rigid_fragment, not the four Tutorial door
    sprites this was first written against. A part the solver has not been shown
    is a recorded refusal for its owner, never a fragment quietly left out.
    """
    from cook import native_sprite,FOCAL,CAM_Z
    from breakables import breakable_sources,rigid_fragment
    import math
    file=Path(scene.file.name).name
    textures={};records=[];errors=[];refusals=refusals if refusals is not None else []
    for owner in breakable_sources(scene,errors=errors,contract=False):
        selected=[]
        for part in owner['debris']:
            gid=int(part['game_object'].split(':')[-1]);components={typ:(i,d)for i,typ,d in _components(scene,gid)}
            try:
                if rigid_fragment(scene,gid) is None:continue
                if owner['fling_speed']!=FLING_SPEED or round(owner['angle_offset'])!=FLING_ANGLE_OFFSET:
                    raise ValueError(f"unmeasured fling {owner['fling_speed']} at {owner['angle_offset']}")
                rid,renderer=components['SpriteRenderer'];obj=source.ref(scene.file,renderer['m_Sprite']);sid=source.sid(obj)
                spin=next((components[name][1]for name in ('SpinSelf','SpinSelfSimple')if name in components),None)
                # SpinSelfSimple turns at a fixed rate; the solver only carries
                # SpinSelf's speed-driven torque, so the others are refused here
                # rather than drawn spinning at the wrong rate.
                if 'SpinSelfSimple' in components:raise ValueError('SpinSelfSimple fixed-rate spin is not in the solver')
                # Spec carries one speed-driven torque and generated_debris
                # requires it positive, so a spinless fragment has no Spec to
                # write rather than one that silently never turns.
                if spin is None or spin['spinFactor']<=0:raise ValueError('fragment has no positive SpinSelf factor')
                if renderer['m_Color']!={'r':1.,'g':1.,'b':1.,'a':1.} or renderer['m_FlipX'] or renderer['m_FlipY']:
                    raise ValueError('unsupported fragment sprite color/reflection')
                matrix=scene.world(scene.go_transform[gid])
                if abs(matrix[0][1])>1e-6 or abs(matrix[1][0])>1e-6 or not all(.5<=abs(matrix[i][i])<=2 for i in range(2)):
                    raise ValueError('unsupported fragment world scale/rotation')
                points=fragment_polygon(components)
                center,_=polygon_moment(points)
                sx,sy=matrix[0][0],matrix[1][1]
                _,inertia=polygon_moment([[x*sx,y*sy]for x,y in points])
                factor=spin['spinFactor']if spin else 0.
            except ValueError as error:
                refusals.append({'owner':owner['source'],'part':part['game_object'],'reason':str(error)});continue
            key=(sid,sx,sy)
            if key not in textures:
                image,box=native_sprite(obj)
                tex=atlas.add(image,(box[2]-box[0])*abs(sx)*FOCAL/-CAM_Z,(box[3]-box[1])*abs(sy)*FOCAL/-CAM_Z,streamed=False)
                textures[key]=len(frames)
                frames.append({'texture':tex,'box':box,'sprite':sid,'event':{'source':'Breakable debris/SpinSelf'}})
            selected.append({'source':f'{file}:{rid}','game_object':part['game_object'],'sprite':sid,'door_source':owner['source'],
                'door_state':owner['state_index'],'frame':textures[key],'scale':[round(sx*65536),round(sy*65536)],'origin':[round(v*65536)for v in part['position'][:2]],
                'centroid':[round(v*65536)for v in center],'polygon':[[round(v*65536)for v in p]for p in points],
                'inertia_estimate':inertia,'torque':round((factor/50)*(180/math.pi)/inertia*65536),'angle_offset':round(owner['angle_offset']),
                'z':part['position'][2],'order':renderer['m_SortingOrder'],'layer':renderer['m_SortingLayer'],
                'limitations':['Uniform-polygon inertia and 50Hz approximation are not verified Unity native physics',
                    'Source ObjectBounce cache/threshold/reflection and sleep are implemented; contact manifold/inertia/friction remain fixed-point native-physics approximations',
                    'No authored lifetime; persistent per-scene pool, deterministic RNG differs from Unity']})
        records.extend(selected)
    # Stable identity independent of texture/region ordering; draw original Z order.
    indices={p['source']:i for i,p in enumerate(sorted(records,key=lambda p:int(p['source'].split(':')[-1])))}
    for p in records:p['id']=indices[p['source']]
    if len(records)>FRAGMENT_POOL:
        authored=len(records)
        for p in records:
            if p['id']>=FRAGMENT_POOL:
                refusals.append({'owner':p['door_source'],'part':p['game_object'],
                    'reason':f'rigid fragment pool holds {FRAGMENT_POOL}; this scene authors {authored}'})
        records=[p for p in records if p['id']<FRAGMENT_POOL]
    return sorted(records,key=lambda p:(-p['z'],p['layer'],p['order'],p['source']))


def append_door_debris_art(source,scene,atlas,frames):
    """Cooked rigid fragments for this scene; see fragment_records."""
    return fragment_records(source,scene,atlas,frames)

# How many distinct door-debris variants the catalogue may hold. Not a format
# limit: the world bank carries the index as a full i32 payload word and the
# guest indexes a slice with it. It is a guard against the table growing without
# anyone looking, and it was written in two places, `host/world.py` and
# `tools/world_metadata.py`, which is why raising one of them did not help.
# Measured at 11 once rigid fragments cooked beyond Tutorial_01, because a
# region's fragment set differs per region rather than per scene.
DEBRIS_VARIANT_LIMIT = 12

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
