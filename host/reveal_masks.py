"""Strict compiled subset of authored inverse reveal-mask FSMs; no asset writes."""
import collections
import math
import json
import struct
from pathlib import Path
from focus import action_fields
from breakables import _components, _descendants, collider_polygons
MAX_CONTROLLERS=16

def scalar(fields,name,variables=None):
    value=fields[name]
    if value['useVariable']:
        if variables is None or value['name'] not in variables:raise ValueError('unknown FSM variable '+name)
        return variables[value['name']]
    return value['value']

def actions(data):
    result=[]
    for i,name in enumerate(data['actionNames']):
        if not data['actionEnabled'][i]:continue
        fields=action_fields(data,i)
        start=data['actionStartIndex'][i];end=data['actionStartIndex'][i+1]if i+1<len(data['actionNames'])else len(data['paramName'])
        for k in range(start,end):
            kind=data['paramDataType'][k];pos=data['paramDataPos'][k];size=data['paramByteDataSize'][k]
            if kind==7:
                if size!=4:raise ValueError('invalid enum width')
                fields[data['paramName'][k]]=struct.unpack('<i',bytes(data['byteData'][pos:pos+size]))[0]
            elif kind==19:fields[data['paramName'][k]]=data['fsmGameObjectParams'][pos]
            # An FsmObject reference. Only the secret-mask shape carries one, on
            # AudioPlayerOneShotSingle's clip, and the decision it feeds is the
            # same whichever clip it names: no reveal clip is resident.
            elif kind==24:fields[data['paramName'][k]]=data['fsmObjectParams'][pos]
            elif kind not in (1,15,16,17,18,20,23):raise ValueError('unsupported action parameter encoding')
        result.append((name.rsplit('.',1)[-1],fields))
    return result

def verify_states(states,variables):
    """Validate complete enabled action sequences, never infer from owner names."""
    expected={'Idle':['Trigger2dEvent','iTweenFadeTo','iTweenFadeTo'],
      'Fade Out':['iTweenFadeTo','iTweenFadeTo','SetBoolValue','Trigger2dEvent'],
      'Fade In':['iTweenFadeTo','iTweenFadeTo','SetBoolValue','Trigger2dEvent'],
      'Pause':['FindChild','iTweenFadeTo','WaitForHeroInPosition','Wait'],'Hero Leave':[]}
    if set(states)!=set(expected):raise ValueError('unsupported reveal states')
    for state,names in expected.items():
        if [n for n,_ in states[state]]!=names:raise ValueError('unsupported enabled action sequence '+state)
    duration=variables.get('Fade Time')
    if not isinstance(duration,(int,float)) or not math.isfinite(duration) or not 0<duration<=10:raise ValueError('invalid Fade Time')
    for state,alpha in [('Idle',0),('Fade Out',1),('Fade In',0)]:
        fades=[f for name,f in states[state]if name=='iTweenFadeTo']
        for index,f in enumerate(fades):
            owner=f['gameObject'];target=owner['gameObject']
            if owner['ownerOption']!=index or (index==1 and (not target['useVariable']or target['name']!='Inverse Mask')):raise ValueError('unsupported fade target')
            if scalar(f,'alpha')!=(alpha if index==0 else 1-alpha):raise ValueError('unexpected fade alpha')
            validate_fade(f,0.01 if state=='Idle'else duration,variables,True)
        trigger=next(f for name,f in states[state]if name=='Trigger2dEvent')
        if trigger['trigger']!=(2 if state=='Fade Out'else 1) or trigger['sendEvent']!=('COVER'if state=='Fade Out'else'UNCOVER') or scalar(trigger,'collideTag')!='Player' or scalar(trigger,'collideLayer')!='':raise ValueError('unsupported trigger semantics')
        if state!='Idle':
            b=next(f for name,f in states[state]if name=='SetBoolValue')
            if not b['boolVariable']['useVariable']or b['boolVariable']['name']!='Activated' or scalar(b,'boolValue')is not True or b['everyFrame']:raise ValueError('unsupported persistent bookkeeping')
    find=states['Pause'][0][1];fade=states['Pause'][1][1];wait_hero=states['Pause'][2][1];wait=states['Pause'][3][1]
    if find['gameObject']['ownerOption']!=0 or scalar(find,'childName')!='Inverse Mask' or not find['storeResult']['useVariable']or find['storeResult']['name']!='Inverse Mask':raise ValueError('unsupported inverse lookup')
    if fade['gameObject']['ownerOption']!=1 or fade['gameObject']['gameObject']['name']!='Inverse Mask' or not fade['gameObject']['gameObject']['useVariable']or scalar(fade,'alpha')!=0:raise ValueError('unsupported pause target')
    validate_fade(fade,0,variables,False)
    if wait_hero['sendEvent']!='FINISHED' or scalar(wait_hero,'skipIfAlreadyPositioned')or wait['finishEvent']!='FINISHED'or abs(scalar(wait,'time')-2)>1e-5 or wait['realTime']:raise ValueError('unsupported pause wait')
    return math.ceil(duration*60-1e-5)

def verify_ordinary_states(states,variables):
    """Authored reversible reveal: Stay uncovers; Exit restores initial opacity."""
    expected={'Idle':['Trigger2dEvent'],
        'Fade Out':['iTweenFadeTo','SetBoolValue','Trigger2dEvent'],
        'Fade In':['iTweenFadeTo','SetBoolValue','Trigger2dEvent'],
        'Pause':['WaitForHeroInPosition','Wait'],'Hero Leave':[]}
    if set(states)!=set(expected):raise ValueError('unsupported ordinary reveal states')
    for state,names in expected.items():
        if [n for n,_ in states[state]]!=names:raise ValueError('unsupported ordinary action sequence '+state)
    duration=variables.get('Fade Time')
    if not isinstance(duration,(int,float))or not math.isfinite(duration)or not 0<duration<=10:raise ValueError('invalid Fade Time')
    for state in ('Idle','Fade Out','Fade In'):
        trigger=next(f for name,f in states[state]if name=='Trigger2dEvent')
        if trigger['trigger']!=(2 if state=='Fade Out'else 1)or trigger['sendEvent']!=('COVER'if state=='Fade Out'else'UNCOVER')or scalar(trigger,'collideTag')!='Player'or scalar(trigger,'collideLayer')!='':raise ValueError('unsupported ordinary trigger semantics')
        if state!='Idle':
            fade=states[state][0][1]
            if fade['gameObject']['ownerOption']!=0 or scalar(fade,'alpha')!=(0 if state=='Fade Out'else 1):raise ValueError('unsupported ordinary fade target')
            validate_fade(fade,duration,variables,True)
            b=states[state][1][1]
            if not b['boolVariable']['useVariable']or b['boolVariable']['name']!='Activated'or scalar(b,'boolValue')is not True or b['everyFrame']:raise ValueError('unsupported persistent bookkeeping')
    hero=states['Pause'][0][1];wait=states['Pause'][1][1]
    if hero['sendEvent']!='FINISHED'or scalar(hero,'skipIfAlreadyPositioned')or wait['finishEvent']!='FINISHED'or abs(scalar(wait,'time')-1)>1e-5 or wait['realTime']:raise ValueError('unsupported ordinary pause wait')
    return math.ceil(duration*60-1e-5)


# ----------------------------------------------------------- secret masks
#
# The third authored reveal shape, and the only one-way one. It hides a secret
# behind a black mask and takes it away for good the first time the hero enters
# the trigger: `Pause` waits for the hero, `Idle` reads the persistent
# `Activated` bool, `Idle Stay` carries the trigger, and `Fade` tweens the owner
# and its children to alpha 0, latches `Activated` and branches to `Sound`.
# Nothing ever fades back, so unlike the reversible shapes this one declares no
# COVER event and no `Hero Leave` global.
#
# Selection is structural, never by definition name. Of the 21 instances in the
# admitted catalogue two carry the definition name `FSM` rather than `unmasker`,
# and `unmasker` also names three unrelated shapes, so a name selector both
# undercounts and overcounts. The digest below is taken over the definition with
# its name normalised out for the same reason.
SECRET_TRANSITIONS={'Idle':[('UNCOVER','Fade'),('STAY','Idle Stay'),('FINISHED','Idle Stay'),('ACTIVATE','Activate')],
    'Fade':[('SOUND','Sound')],'Pause':[('FINISHED','Idle')],'Idle Stay':[('UNCOVER','Fade')],'Sound':[],'Activate':[]}
SECRET_CANONICAL_NAME='secret mask'
# The one variable a placement is allowed to differ on: 5 of the 21 chime on
# reveal, 16 are silent. Every other serialized scalar is identical family-wide,
# which is why one digest can stand for all of them.
SECRET_PLACEMENT_VARIABLES=('Play Sound',)
SECRET_SHA256='12e1a80b34d99f72c9552401c98ae76ced219aa7151e865f834d1d2f3103aa90'
# `Activate` is the already-revealed path: a 0.1s tween straight to clear, where
# the hero-triggered `Fade` takes its serialized 0.5s.
SECRET_ACTIVATED_SECONDS=0.1
# `plays_sound` is the placement's `Play Sound`: the guest plays the
# `secret_discovered_temp` chime (host/scene_sfx.py `secret_discovered`) only
# for a mask authored to chime.


def verify_secret_states(states,variables):
    """Authored one-way reveal: the first hero entry uncovers the secret for good.

    Returns (fade ticks, whether the reveal is authored to chime).
    """
    expected={'Pause':['WaitForHeroInPosition'],'Idle':['BoolTest'],'Idle Stay':['Trigger2dEvent'],
        'Fade':['iTweenFadeTo','SetBoolValue','BoolTest'],'Activate':['iTweenFadeTo'],
        'Sound':['AudioPlayerOneShotSingle']}
    if set(states)!=set(expected):raise ValueError('unsupported secret reveal states')
    for state,names in expected.items():
        if [n for n,_ in states[state]]!=names:raise ValueError('unsupported secret action sequence '+state)
    hero=states['Pause'][0][1]
    if hero['sendEvent']!='FINISHED' or scalar(hero,'skipIfAlreadyPositioned'):raise ValueError('unsupported secret pause wait')
    gate=states['Idle'][0][1]
    if not gate['boolVariable']['useVariable'] or gate['boolVariable']['name']!='Activated' \
            or gate['isTrue']!='ACTIVATE' or gate['isFalse'] or gate['everyFrame']:
        raise ValueError('unsupported secret activation test')
    # Nothing in this port answers `Activated`: there is no save and no scene
    # data, so the cook reproduces the fresh-save answer the serialized value
    # gives. A placement shipped already revealed would need one.
    if variables.get('Activated') not in (0,False):raise ValueError('secret mask is serialized already revealed')
    trigger=states['Idle Stay'][0][1]
    # `trigger` is 1, OnTriggerStay2D (PlayMaker's Trigger2DType: Enter 0,
    # Stay 1, Exit 2). The guest's inside test answers Stay.
    if trigger['trigger']!=1 or trigger['sendEvent']!='UNCOVER' or scalar(trigger,'collideTag')!='Player' \
            or scalar(trigger,'collideLayer')!='':
        raise ValueError('unsupported secret trigger semantics')
    fade=states['Fade'][0][1]
    if fade['gameObject']['ownerOption']!=0 or scalar(fade,'alpha')!=0:raise ValueError('unsupported secret fade target')
    duration=scalar(fade,'time',variables)
    if not isinstance(duration,(int,float)) or not math.isfinite(duration) or not 0<duration<=10:
        raise ValueError('invalid secret fade time')
    validate_fade(fade,duration,variables,True)
    latch=states['Fade'][1][1]
    if not latch['boolVariable']['useVariable'] or latch['boolVariable']['name']!='Activated' \
            or scalar(latch,'boolValue')is not True or latch['everyFrame']:
        raise ValueError('unsupported secret persistent bookkeeping')
    chime=states['Fade'][2][1]
    if not chime['boolVariable']['useVariable'] or chime['boolVariable']['name']!='Play Sound' \
            or chime['isTrue']!='SOUND' or chime['isFalse'] or chime['everyFrame']:
        raise ValueError('unsupported secret sound test')
    # The already-revealed path cannot be reached without a save system, but it
    # still has to agree with `Fade` about where the mask ends up: pinning only
    # the reachable half would leave the other half free to say anything.
    revealed=states['Activate'][0][1]
    if revealed['gameObject']['ownerOption']!=0 or scalar(revealed,'alpha')!=0:
        raise ValueError('unsupported secret activate target')
    validate_fade(revealed,SECRET_ACTIVATED_SECONDS,variables,True)
    sound=variables.get('Play Sound')
    if sound not in (0,1,False,True):raise ValueError('secret Play Sound is not a serialized boolean')
    return math.ceil(duration*60-1e-5),bool(sound)


NO_COLLIDER='no trigger collider on the owner, so the authored Trigger2dEvent can never fire'

def no_collider_reason(sc,gid):
    """Why an owner with no Collider2D is refused, and what it is waiting on.

    An owner with no Collider2D never receives OnTriggerEnter2D at all, so its
    Trigger2dEvent cannot fire however much the port builds. On its own that
    reads as broken authoring, and it is not: five of the catalogue's secret
    masks are uncovered by a hidden wall broadcasting UNCOVER when it breaks,
    not by the hero, so the refusal names the wall it waits on.

    breakables.uncover_drivers reads scene structure only and never asks back
    about reveal admission, so there is no cycle between the two recognizers.
    """
    from breakables import uncover_drivers
    driver=uncover_drivers(sc).get(gid)
    if not driver:return NO_COLLIDER
    return (f"{NO_COLLIDER}; uncovered instead by the hidden wall {driver['name']} "
            f"({driver['source']}, definition {driver['definition']}), which has to be admitted first")


def validate_fade(f,time,variables,loop_finish):
    if (abs(scalar(f,'time',variables)-time)>1e-5 or scalar(f,'delay')!=0 or not scalar(f,'includeChildren')
        or scalar(f,'namedValueColor')!='_Color' or f['easeType']!=21 or f['loopType']!=0
        or scalar(f,'realTime') or not scalar(f,'stopOnExit') or scalar(f,'loopDontFinish')!=loop_finish
        or f['startEvent'] or f['finishEvent']):raise ValueError('unsupported tween semantics')

# ----------------------------------------------------- driven and two-state
#
# Two more authored shapes, both one-way and neither saved on its own:
#
#   two-state   `unmasker` with `Idle -EVENT-> Fade`, where EVENT is HIT or
#               UNCOVER and `Fade` is one iTweenFadeTo of the owner and its
#               children to alpha 0. `Idle` either carries a Trigger2dEvent
#               (OnTriggerEnter2D, tag Player) that sends EVENT on the owner's
#               own boxes (Tutorial_01 Tut_msk_02, Crossroads_46's eggs) or has
#               no actions, when only another object sends it
#               (Crossroads_03's `crossroads_03_mask`, named by its wall).
#   floor fade  `fade` on Crossroads_09's `msk_generic`: `Pause -FINISHED->
#               Idle [BoolTest Activated isTrue=HIT] -HIT-> Fade [iTweenFadeTo
#               0.4 s, SetBoolValue]`. Only the Mawlek wall's `Break` sends HIT.
#
# A mask a hidden wall or a cracked floor uncovers has no trigger of its own;
# its record names that secret's state (`driver_state`) and the guest fires it
# on the break. One a C# Breakable's `hitEventReciever` drives is the
# Breakable's own mask fade (host/breakables.py `mask_fades`), not a controller.
TWO_STATE_EVENTS=('HIT','UNCOVER')

def _fade_action(fields,variables=None):
    """One iTweenFadeTo of the owner and its children to 0: its seconds."""
    if fields['gameObject']['ownerOption']!=0 or scalar(fields,'alpha')!=0:raise ValueError('unsupported fade target')
    seconds=scalar(fields,'time',variables)
    if not isinstance(seconds,(int,float)) or not math.isfinite(seconds) or not 0<seconds<=10:raise ValueError('invalid fade time')
    if scalar(fields,'delay')!=0 or not scalar(fields,'includeChildren') or fields['easeType']!=21 or fields['loopType']!=0 \
            or scalar(fields,'realTime') or scalar(fields,'namedValueColor')!='_Color':
        raise ValueError('unsupported tween semantics')
    return seconds

def two_state(fsm,states,transitions):
    """(event, seconds, own trigger) of a two-state unmasker, or None."""
    if fsm['startState']!='Idle' or set(states)!={'Idle','Fade'} or transitions.get('Fade'):return None
    idle=transitions.get('Idle',[])
    if len(idle)!=1 or idle[0][1]!='Fade' or idle[0][0] not in TWO_STATE_EVENTS:return None
    event=idle[0][0]
    if [n for n,_ in states['Fade']]!=['iTweenFadeTo']:raise ValueError('two-state fade is not one iTweenFadeTo')
    seconds=_fade_action(states['Fade'][0][1])
    actions=[n for n,_ in states['Idle']]
    if actions==[]:return event,seconds,False
    if actions!=['Trigger2dEvent']:raise ValueError('two-state Idle is not one Trigger2dEvent')
    trigger=states['Idle'][0][1]
    if trigger['trigger']!=0 or trigger['sendEvent']!=event or scalar(trigger,'collideTag')!='Player' or scalar(trigger,'collideLayer')!='':
        raise ValueError('unsupported two-state trigger semantics')
    return event,seconds,True

def floor_fade(fsm,states,transitions):
    """The `fade` FSM's seconds, or None when this is not that shape."""
    if fsm['startState']!='Pause' or set(states)!={'Pause','Idle','Fade'}:return None
    if transitions!={'Pause':[('FINISHED','Idle')],'Idle':[('HIT','Fade')],'Fade':[]}:return None
    if [n for n,_ in states['Pause']]!=['NextFrameEvent'] or [n for n,_ in states['Idle']]!=['BoolTest'] \
            or [n for n,_ in states['Fade']]!=['iTweenFadeTo','SetBoolValue']:
        raise ValueError('unsupported floor mask fade actions')
    gate=states['Idle'][0][1]
    if gate['boolVariable']['name']!='Activated' or gate['isTrue']!='HIT' or gate['isFalse'] or gate['everyFrame']:
        raise ValueError('unsupported floor mask activation test')
    return _fade_action(states['Fade'][0][1])

def _breakable_receivers(sc):
    """GameObjects a C# Breakable forwards HIT to."""
    out=set()
    for typ,tree in sc.objects.values():
        if typ=='Breakable' and tree.get('hitEventReciever',{}).get('m_PathID'):out.add(tree['hitEventReciever']['m_PathID'])
    return out

def reveal_mask_sources(sc):
    if hasattr(sc,'_reveal_masks'):return sc._reveal_masks
    from secret_breaks import secret_drivers
    file=Path(sc.file.name).name;records=[];unsupported=[]
    drivers=secret_drivers(sc);receivers=_breakable_receivers(sc)
    for sid,(typ,tree) in sorted(sc.objects.items()):
        if typ!='PlayMakerFSM':continue
        fsm=tree['fsm'];names=[a for st in fsm['states']for a in st['actionData']['actionNames']]
        if not any(n.endswith('iTweenFadeTo')for n in names):continue
        events={t['fsmEvent']['name'] for st in fsm['states'] for t in st['transitions']}
        if not any(n.endswith('Trigger2dEvent')for n in names) and not events&set(TWO_STATE_EVENTS):continue
        gid=tree['m_GameObject']['m_PathID']
        if not tree['m_Enabled'] or not sc.active(gid):continue
        try:
            transitions={st['name']:[(t['fsmEvent']['name'],t['toState'])for t in st['transitions']]for st in fsm['states']}
            globals_=[(t['fsmEvent']['name'],t['toState'])for t in fsm['globalTransitions']]
            variables={v['name']:v['value']for group in fsm['variables'].values()if isinstance(group,list)for v in group if isinstance(v,dict)and'name'in v and'value'in v}
            states={st['name']:actions(st['actionData'])for st in fsm['states']}
            driver=drivers.get(gid);own_trigger=True;replay=False;one_way=False;plays_sound=False
            shape=None if globals_ else two_state(fsm,states,transitions)
            fade=None if shape or globals_ else floor_fade(fsm,states,transitions)
            if shape:
                event,seconds,own_trigger=shape
                if gid in receivers:raise ValueError('a Breakable forwards HIT to this mask: it is that Breakable\'s own mask fade')
                if not own_trigger and driver is None:raise ValueError(f'nothing in this port sends {event} to this mask')
                ticks=math.ceil(seconds*60-1e-5);ordinary=True;one_way=True
                # `Idle` has no Activated test and no save, so the wall's own
                # Activated path sends UNCOVER again on every load.
                replay=driver is not None
                kind='two_state'
            elif fade is not None:
                if driver is None:raise ValueError('nothing in this port sends HIT to this floor mask')
                ticks=math.ceil(fade*60-1e-5);ordinary=True;one_way=True;own_trigger=False;kind='floor_fade'
            else:
                if fsm['startState']!='Pause':raise ValueError('unsupported initial state')
                one_way=transitions==SECRET_TRANSITIONS
                if one_way:
                    from false_knight import fsm_digest
                    if globals_:raise ValueError('unsupported global transitions')
                    digest=fsm_digest(dict(fsm,name=SECRET_CANONICAL_NAME),SECRET_PLACEMENT_VARIABLES)
                    if digest!=SECRET_SHA256:raise ValueError(f'unverified secret mask variant: {digest}')
                    ticks,plays_sound=verify_secret_states(states,variables)
                    # The mask is drawn at the authored opacity and only ever leaves.
                    ordinary=True;kind='secret'
                else:
                    if transitions!={'Idle':[('UNCOVER','Fade Out')],'Fade Out':[('COVER','Fade In')],'Pause':[('FINISHED','Idle')],'Fade In':[('UNCOVER','Fade Out')],'Hero Leave':[]}:raise ValueError('unsupported transitions')
                    if globals_!=[('HERO LEAVE','Hero Leave')]:raise ValueError('unsupported global transitions')
                    ordinary=[n for n,_ in states.get('Idle',[])]==['Trigger2dEvent']
                    ticks=(verify_ordinary_states if ordinary else verify_states)(states,variables)
                    children=sc.transforms[sc.go_transform[gid]]['m_Children']
                    if not ordinary and any(sc.gos[sc.transforms[c['m_PathID']]['m_GameObject']['m_PathID']]['m_Name']=='Inverse Mask'for c in children):raise ValueError('non-null inverse target needs separate bindings')
                    kind='ordinary'if ordinary else'inverse'
            components=list(_components(sc,gid));colliders=[(i,t,d)for i,t,d in components if t.endswith('Collider2D')and d['m_Enabled']]
            polygons=[]
            if own_trigger:
                # Separate reasons, because these two refusals mean different
                # things; `no_collider_reason` explains the first and names its
                # driver. A driven one-way mask needs none: its break fires it.
                if not colliders and driver is None:raise ValueError(no_collider_reason(sc,gid))
                # Trigger2dEvent answers any Collider2D on the owner, so several
                # boxes are one trigger: the guest ORs their polygons.
                for collider,ctype,body in colliders:
                    if ctype not in ('BoxCollider2D','PolygonCollider2D')or not body['m_IsTrigger']:raise ValueError('unsupported trigger geometry')
                    polygons+=collider_polygons(sc,gid,ctype,body)
                if len(polygons)>8:raise ValueError('more than eight trigger paths')
            if driver is not None and kind not in('secret','two_state','floor_fade'):raise ValueError('a secret uncovers a reversible mask')
            if driver is not None:polygons=[]
            renderers=[];alphas=[]
            for child in sorted(_descendants(sc,gid)):
                for ri,rt,rd in _components(sc,child):
                    # A renderer with no sprite, or switched off, draws nothing in
                    # Unity: the `Secret Sound Region` and `sounder` objects are
                    # triggers with a chime and no art.
                    if rt=='SpriteRenderer'and rd['m_Enabled']and sc.active(child)and rd['m_Sprite']['m_PathID']:
                        renderers.append(f'{file}:{ri}');alphas.append(rd['m_Color']['a'])
            if not renderers and not one_way:raise ValueError('no active renderer')
            if not renderers and not polygons and driver is None:raise ValueError('no active renderer and nothing fires it')
            persistence=[{'source':f'{file}:{i}','dont_save':bool(d['dontSave']),'semi_persistent':bool(d['semiPersistent']),'authored':d['persistentBoolData']}for i,t,d in components if t=='PersistentBoolItem']
            saved=one_way and driver is None and any(not p['dont_save'] and not p['semi_persistent'] for p in persistence)
            records.append({'source':f'{file}:{sid}','source_id':sid,'game_object':f'{file}:{gid}','name':sc.gos[gid]['m_Name'],
                'trigger_sources':[f'{file}:{c[0]}' for c in colliders] if polygons else [],
                'trigger':polygons[0] if polygons else None,'triggers':polygons,'fade_ticks':ticks,'renderer_sources':renderers,'renderer_alphas':alphas,
                'persistence':persistence,'saved':saved,'initial_opacity':128 if ordinary else 0,'kind':kind,'one_way':one_way,
                'plays_sound':plays_sound,'driver_state':driver,'replay_on_load':replay,'states':states})
        except (ValueError,KeyError,IndexError,TypeError)as e:unsupported.append({'source':f'{file}:{sid}','name':sc.gos[gid]['m_Name'],'error':str(e)})
    if len(records)>MAX_CONTROLLERS:raise ValueError('reveal controller scene pool exceeds16')
    for i,r in enumerate(records):r['controller']=i
    # The save key of a one-way mask: its rank among the scene's saved one-way
    # masks by source id. The controller index moves whenever a controller is
    # admitted or refused; this does not, and for every mask a save can already
    # hold it equals the index that save was written with.
    for slot,r in enumerate(sorted((r for r in records if r['saved']),key=lambda r:r['source_id'])):r['persist_slot']=slot
    sc._reveal_masks={'controllers':records,'unsupported':unsupported}
    return sc._reveal_masks

def validate_palette(raw, draw):
    """Renderer opacity uses a subtractive CLUT only for this exact black mask."""
    from region_delta import layout
    counts,prefix,_,_=layout(raw)
    textures,draws=counts[1:3]
    if not 0<=draw<draws:raise ValueError('reveal draw outside room')
    texture=struct.unpack_from('<H',raw,40+textures*16+draw*44)[0]
    if texture>=textures:raise ValueError('reveal texture outside room')
    page,*_,palette=struct.unpack_from('<6H',raw,40+texture*16)
    if page==65535 or palette>=textures:raise ValueError('reveal mask must use static valid palette')
    words=struct.unpack_from('<16H',raw,prefix+palette*32)
    if words!=(0,1,*([0x8000]*14)):raise ValueError('reveal mask requires exact binary black palette')


def refuse_partial_one_way(by_scene,scene_hits):
    """A one-way reveal takes its whole authored fade with it, or it does not run.

    A reversible mask can afford a draw that will not fade: the hero leaves and
    the room is as it was. A secret mask never covers again, so a renderer the
    subtractive CLUT cannot fade would sit black over the secret for the rest of
    the game, and one that was never cooked into a draw would leave the hero
    walking through a reveal that reveals nothing. This is the destruction-output
    rule read for a reveal: an instance whose authored output the port cannot
    produce is refused here and stays whole scenery instead of half-opening.

    `scene_hits` is scene id to the (controller index, renderer source, palette
    error) triples found across every region of that scene. Refused controllers
    move to `unsupported` and the survivors are renumbered, because the guest
    indexes its scene pool by position. Returns scene id to {old index: new
    index} so the caller can re-key the bindings it already found.
    """
    renumbered={}
    for scene,entry in by_scene.items():
        hits=scene_hits.get(scene,())
        kept=[];moved={}
        for index,record in enumerate(entry['controllers']):
            if record.get('one_way'):
                mine=[hit for hit in hits if hit[0]==index]
                drawn={source for _,source,_ in mine}
                absent=[source for source in record['renderer_sources']if source not in drawn]
                # A member whose palette is not binary black fades with the
                # draw's gain instead (game/src/reveal_masks.rs apply): iTween
                # multiplies the material colour, so a coloured or partly
                # transparent member darkening to nothing as the black lifts
                # is the same product. Only art that is never drawn refuses.
                unfadeable=[]
                if absent:
                    reasons=[]
                    if absent:reasons.append(f"{len(absent)} of {len(record['renderer_sources'])} "
                                             'renderers are not cooked into any draw')
                    # Counted per renderer, not per draw: `unfadeable` is a set of
                    # renderer sources and the same renderer is cooked into one
                    # draw per region it is resident in. Crossroads_37's Mask
                    # Bottom is 4 renderers over 16 draws, and reading that 4 as
                    # a draw count understates the offending art fourfold.
                    if unfadeable:reasons.append(f"{len(unfadeable)} of {len(record['renderer_sources'])} "
                                                 'renderers draw with a palette the subtractive fade cannot use')
                    entry.setdefault('unsupported',[]).append({'source':record.get('source'),'name':record.get('name'),
                        'error':'one-way reveal cannot fade its whole authored group: '+'; '.join(reasons)})
                    continue
            moved[index]=len(kept);kept.append(record)
        entry['controllers']=kept
        for index,record in enumerate(kept):record['controller']=index
        renumbered[scene]=moved
    return renumbered


def bind_regions(report,by_scene,draw_sources=None,room_bytes=None):
    report['reveal_mask_scenes']={str(k):v for k,v in by_scene.items()}
    from source import ROOT
    # Every candidate binding first, because a one-way controller's verdict is
    # scene-wide: its renderers can be spread over several residency regions and
    # one of them failing refuses the whole controller, not just that draw.
    found={};scene_hits=collections.defaultdict(list)
    for region in report['regions']:
        chunk=region['chunk_id'];sources=draw_sources[chunk]if draw_sources is not None else [d['source']for d in json.loads((ROOT/f'data/regions/region-{chunk:03}/scene.json').read_text())['draws']]
        if len(sources)!=region['draws']:raise ValueError('reveal draw provenance count mismatch')
        records=by_scene[region['scene_id']]['controllers'];owned=set();raw=None;hits=[]
        if len(records)>MAX_CONTROLLERS:raise ValueError('reveal controller scene pool exceeds16')
        for i,r in enumerate(records):
            if r['controller']!=i:raise ValueError('noncanonical reveal controller index')
            for draw,source in enumerate(sources):
                if source not in r['renderer_sources']:continue
                if draw in owned:raise ValueError('multiple reveal owners fordraw')
                owned.add(draw)
                if raw is None:raw=room_bytes[chunk]if room_bytes is not None else (ROOT/region['path']).read_bytes()
                try:validate_palette(raw,draw);error=None
                except ValueError as e:error=str(e)
                hits.append((i,draw,source,error))
        found[chunk]=hits
        scene_hits[region['scene_id']]+=[(i,source,error)for i,_,source,error in hits]
    renumbered=refuse_partial_one_way(by_scene,scene_hits)
    for region in report['regions']:
        moved=renumbered[region['scene_id']]
        kept=[];rejected=[]
        for i,draw,source,error in found[region['chunk_id']]:
            if i not in moved:continue  # refused scene-wide above; its draw stays static
            binding={'controller':moved[i],'draw':draw,'renderer_source':source}
            # A draw the subtractive CLUT cannot fade is faded by its gain; the
            # record says which so a report can tell the two apart.
            kept.append(binding)
            if error:rejected.append(dict(binding,fade='gain',reason=error))
        if rejected:region['reveal_mask_gain']=rejected
        region['reveal_mask_bindings']=kept
    report['reveal_mask_policy']={'scope':'Verified ordinary/inverse reversible, one-way secret, two-state HIT/UNCOVER and floor fade FSM subsets; scene state shared across all residency regions','initialization':'Scene-ready begins Idle at authored opacity (ordinary and secret128, inverse0); approximates source Pause hero-position callback/1s ordinary or2s inverse timeout and inverse0.01s initialization tween','persistence':'A saved one-way mask is a SecretMask item keyed by its persist_slot (rank among the scene\'s saved one-way masks); a mask a hidden wall or cracked floor uncovers follows that secret\'s Breakable item','secret_masks':'One-way: uncovered on first hero entry (or when their wall or floor breaks) and never re-covered while the scene is loaded. The reveal chime plays only for masks authored with Play Sound. Binary black members fade through the subtractive CLUT, every other member through its gain','max_controllers_per_scene':MAX_CONTROLLERS}

def postpack_reveal_masks(report,source,scenes):
    from scene import Scene
    by_scene={}
    for info in report['scenes']:
        i=info['scene_id']
        if i not in scenes:scenes[i]=Scene(source,info['file'])
        by_scene[i]=reveal_mask_sources(scenes[i])
    bind_regions(report,by_scene)
