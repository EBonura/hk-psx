"""Strict compiled subset of authored inverse reveal-mask FSMs; no asset writes."""
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


def validate_fade(f,time,variables,loop_finish):
    if (abs(scalar(f,'time',variables)-time)>1e-5 or scalar(f,'delay')!=0 or not scalar(f,'includeChildren')
        or scalar(f,'namedValueColor')!='_Color' or f['easeType']!=21 or f['loopType']!=0
        or scalar(f,'realTime') or not scalar(f,'stopOnExit') or scalar(f,'loopDontFinish')!=loop_finish
        or f['startEvent'] or f['finishEvent']):raise ValueError('unsupported tween semantics')

def reveal_mask_sources(sc):
    if hasattr(sc,'_reveal_masks'):return sc._reveal_masks
    file=Path(sc.file.name).name;records=[];unsupported=[]
    for sid,(typ,tree) in sorted(sc.objects.items()):
        if typ!='PlayMakerFSM':continue
        fsm=tree['fsm'];names=[a for st in fsm['states']for a in st['actionData']['actionNames']]
        if not any(n.endswith('iTweenFadeTo')for n in names)or not any(n.endswith('Trigger2dEvent')for n in names):continue
        gid=tree['m_GameObject']['m_PathID']
        if not tree['m_Enabled'] or not sc.active(gid):continue
        try:
            if fsm['startState']!='Pause':raise ValueError('unsupported initial state')
            transitions={st['name']:[(t['fsmEvent']['name'],t['toState'])for t in st['transitions']]for st in fsm['states']}
            if transitions!={'Idle':[('UNCOVER','Fade Out')],'Fade Out':[('COVER','Fade In')],'Pause':[('FINISHED','Idle')],'Fade In':[('UNCOVER','Fade Out')],'Hero Leave':[]}:raise ValueError('unsupported transitions')
            if [(t['fsmEvent']['name'],t['toState'])for t in fsm['globalTransitions']]!=[('HERO LEAVE','Hero Leave')]:raise ValueError('unsupported global transitions')
            variables={v['name']:v['value']for group in fsm['variables'].values()if isinstance(group,list)for v in group if isinstance(v,dict)and'name'in v and'value'in v}
            states={st['name']:actions(st['actionData'])for st in fsm['states']}
            ordinary=[n for n,_ in states.get('Idle',[])]==['Trigger2dEvent']
            ticks=(verify_ordinary_states if ordinary else verify_states)(states,variables)
            children=sc.transforms[sc.go_transform[gid]]['m_Children']
            if not ordinary and any(sc.gos[sc.transforms[c['m_PathID']]['m_GameObject']['m_PathID']]['m_Name']=='Inverse Mask'for c in children):raise ValueError('non-null inverse target needs separate bindings')
            components=list(_components(sc,gid));colliders=[(i,t,d)for i,t,d in components if t.endswith('Collider2D')and d['m_Enabled']]
            if len(colliders)!=1 or colliders[0][1]not in ('BoxCollider2D','PolygonCollider2D')or not colliders[0][2]['m_IsTrigger']:raise ValueError('unsupported trigger geometry')
            collider,ctype,body=colliders[0];polygons=collider_polygons(sc,gid,ctype,body)
            if len(polygons)!=1:raise ValueError('multiple trigger paths unsupported')
            renderers=[]
            for child in sorted(_descendants(sc,gid)):
                for ri,rt,rd in _components(sc,child):
                    if rt=='SpriteRenderer'and rd['m_Enabled']and sc.active(child):
                        if rd['m_Color']['a']!=1:raise ValueError('non-unit authored alpha needs separate gain')
                        renderers.append(f'{file}:{ri}')
            if not renderers:raise ValueError('no active renderer')
            persistence=[{'source':f'{file}:{i}','dont_save':bool(d['dontSave']),'semi_persistent':bool(d['semiPersistent']),'authored':d['persistentBoolData']}for i,t,d in components if t=='PersistentBoolItem']
            records.append({'source':f'{file}:{sid}','source_id':sid,'game_object':f'{file}:{gid}','name':sc.gos[gid]['m_Name'],'trigger_source':f'{file}:{collider}','trigger':polygons[0],'fade_ticks':ticks,'renderer_sources':renderers,'persistence':persistence,'initial_opacity':128 if ordinary else 0,'kind':'ordinary'if ordinary else'inverse','states':states})
        except (ValueError,KeyError,IndexError,TypeError)as e:unsupported.append({'source':f'{file}:{sid}','name':sc.gos[gid]['m_Name'],'error':str(e)})
    if len(records)>MAX_CONTROLLERS:raise ValueError('reveal controller scene pool exceeds16')
    for i,r in enumerate(records):r['controller']=i
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


def bind_regions(report,by_scene,draw_sources=None,room_bytes=None):
    report['reveal_mask_scenes']={str(k):v for k,v in by_scene.items()}
    from source import ROOT
    for region in report['regions']:
        chunk=region['chunk_id'];sources=draw_sources[chunk]if draw_sources is not None else [d['source']for d in json.loads((ROOT/f'data/regions/region-{chunk:03}/scene.json').read_text())['draws']]
        if len(sources)!=region['draws']:raise ValueError('reveal draw provenance count mismatch')
        records=by_scene[region['scene_id']]['controllers'];bindings=[];owned=set()
        if len(records)>MAX_CONTROLLERS:raise ValueError('reveal controller scene pool exceeds16')
        for i,r in enumerate(records):
            if r['controller']!=i:raise ValueError('noncanonical reveal controller index')
            for draw,source in enumerate(sources):
                if source in r['renderer_sources']:
                    if draw in owned:raise ValueError('multiple reveal owners fordraw')
                    owned.add(draw);bindings.append({'controller':i,'draw':draw,'renderer_source':source})
        if bindings:
            raw=room_bytes[chunk]if room_bytes is not None else (ROOT/region['path']).read_bytes()
            for binding in bindings:validate_palette(raw,binding['draw'])
        region['reveal_mask_bindings']=bindings
    report['reveal_mask_policy']={'scope':'Verified ordinary/inverse reversible FSM subsets; scene state shared across all residency regions','initialization':'Scene-ready begins Idle at authored opacity (ordinary128, inverse0); approximates source Pause hero-position callback/1s ordinary or2s inverse timeout and inverse0.01s initialization tween','persistence':'Activated bookkeeping does not control visibility; no memory-card save implementation','max_controllers_per_scene':MAX_CONTROLLERS}

def postpack_reveal_masks(report,source,scenes):
    from scene import Scene
    by_scene={}
    for info in report['scenes']:
        i=info['scene_id']
        if i not in scenes:scenes[i]=Scene(source,info['file'])
        by_scene[i]=reveal_mask_sources(scenes[i])
    bind_regions(report,by_scene)
