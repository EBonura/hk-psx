"""Observed Tutorial Great Door FSM and source stage art, without generic FSM execution."""
import copy,json,math
from pathlib import Path
from focus import action_fields

ROOT=Path(__file__).resolve().parents[1]
# The door's long-axis texel cap. Its three poses project to about 117x216 and
# 142x209 screen pixels, so the old 48-texel scenery cap sampled them at about
# 26x48. At 128 every pose is a 2x2 rectangle of 64x64 animation slots
# (Atlas.add_tiled, the NPC route): four of the frame working set's keys while
# the door is on screen, instead of one. Full size would bind 8 to 12.
DOOR_MAX_AXIS=128

def contract(fsm):
    states={s['name']:s for s in fsm['states']}
    def actions(state,kind):
        d=states[state]['actionData']
        return [action_fields(d,i)for i,n in enumerate(d['actionNames'])if n.rsplit('.',1)[-1]==kind and d['actionEnabled'][i]]
    def one(state,kind):
        result=actions(state,kind)
        if len(result)!=1:raise ValueError(f'Great Door expected one {state}/{kind}')
        return result[0]
    def literal(field):
        if field['useVariable']:raise ValueError('Great Door requires literal value')
        return field['value']
    def transition(state,event):return next(t['toState']for t in states[state]['transitions']if t['fsmEvent']['name']==event)
    if fsm['name']!='Great Door' or transition('Idle','HIT')!='Check If Nail' or transition('Check If Nail','TRUE')!='Hit':raise ValueError('Great Door hit path changed')
    if transition('Hit','FINISHED')!='Check Hits':raise ValueError('Great Door hit increment order changed')
    nail=one('Check If Nail','IntSwitch')
    nail_values=[literal(v)for k,v in nail.items()if k.isdigit()and isinstance(v,dict)]
    if nail_values!=[0]:raise ValueError('Great Door requires nail-only damage')
    add=one('Check Hits','IntAdd')
    if add['intVariable']['name']!='Hits' or literal(add['add'])!=1:raise ValueError('Great Door hit increment changed')
    switch=one('Check Hits','IntSwitch')
    stages=[literal(v)for k,v in switch.items()if k.isdigit()and isinstance(v,dict)]
    if stages!=[4,8,13]:raise ValueError('Great Door stage thresholds changed')
    sprites=[literal(one(st,'Tk2dSpriteSetId')['ORSpriteName'])for st in ('Break 1','Break 2')]
    if sprites!=['door_v02','door_v03']:raise ValueError('Great Door stage art changed')
    waits=[literal(one(st,'Wait')['time'])for st in ('Check Hits','Break 1','Break 2')]
    if any(abs(t-.15)>1e-6 for t in waits):raise ValueError('Great Door hit cooldown changed')
    move=one('Move','BeginSceneTransition')
    target=literal(move['sceneName']);entry=literal(move['entryGateName']);delay=literal(move['entryDelay'])
    if (target,entry)!=('Town','left1') or delay!=2.5:raise ValueError('Great Door destination changed')
    if transition('Break','RIGHT')!='Extra Frame' or transition('Extra Frame','FINISHED')!='Move':raise ValueError('Great Door next-frame transition changed')
    if one('Break','NextFrameEvent')['sendEvent']!='RIGHT' or one('Extra Frame','NextFrameEvent')['sendEvent']!='FINISHED':
        raise ValueError('Great Door departure events changed')
    fades=[a for a in actions('Break','SendEventByName') if literal(a['sendEvent'])=='FADE OUT INSTANT']
    if len(fades)!=1 or literal(fades[0]['delay'])!=0:raise ValueError('Great Door instant blackout changed')
    for kind in ('SetCollider','SetMeshRenderer'):
        if literal(one('Activate',kind)['active']):raise ValueError('Great Door persistence activation changed')
    return {'stage_hits':stages,'cooldown_ticks':round(waits[0]*60),'transition_ticks':2,'entry_delay_ticks':round(delay*60),
            'stage_names':['door_v01']+sprites,'target_scene':target,'entry_gate':entry}

def append_art(source,scene,atlas,frames):
    """Append three streamed frames to the existing scene-wide animation bank."""
    if Path(scene.file.name).name!='level6':return None
    candidates=[(i,t)for i,(kind,t)in scene.objects.items()if kind=='PlayMakerFSM'and t['fsm']['name']=='Great Door']
    if len(candidates)!=1:raise ValueError('Expected one Tutorial Great Door')
    sid,tree=candidates[0];values=contract(tree['fsm']);gid=tree['m_GameObject']['m_PathID']
    components=[(i,kind,t)for i,(kind,t)in scene.objects.items()if t.get('m_GameObject',{}).get('m_PathID')==gid]
    collider_id,_,collider=next(c for c in components if c[1]=='BoxCollider2D')
    sprite_id,_,sprite=next(c for c in components if c[1]=='tk2dSprite')
    from breakables import collider_polygons
    polygons=collider_polygons(scene,gid,'BoxCollider2D',collider)
    points=[p for poly in polygons for p in poly]
    bounds=[min(p[0]for p in points),min(p[1]for p in points),max(p[0]for p in points),max(p[1]for p in points)]
    from cook import tk_sprite,FOCAL,CAM_Z
    collection_o=source.ref(scene.file,sprite['collection']);collection=source.read(collection_o);textures={}
    matrix=scene.world(scene.go_transform[gid])
    if any(abs(matrix[i][j]-(1 if i==j else 0))>1e-6 for i in range(3)for j in range(3)):raise ValueError('Great Door transform changed')
    position=scene.point(gid);scale=FOCAL/(position[2]-CAM_Z);indices=[];boxes=[]
    for name in values['stage_names']:
        index=next(i for i,d in enumerate(collection['spriteDefinitions'])if d['name']==name)
        image,box=tk_sprite(source,collection_o.assets_file,collection,index,textures)
        # Never above the sprite's own pixels, never above the door cap.
        w=(box[2]-box[0])*scale;h=(box[3]-box[1])*scale;factor=min(1,DOOR_MAX_AXIS/max(w,h),max(image.size)/max(w,h))
        texture=atlas.add_tiled(image,w*factor,h*factor)
        indices.append(len(frames));boxes.append(box)
        frames.append({'texture':texture,'box':box,'sprite':source.sid(collection_o)+':'+str(index),'event':{}})
    return dict(values,source=f'level6:{sid}',collider_source=f'level6:{collider_id}',sprite_source=f'level6:{sprite_id}',
                position=position,scale=round(scale*4096),bounds=bounds,frames=indices,frame_boxes=boxes)

def bind(record,row,frame_base):
    if record is None:return None
    result=copy.deepcopy(record);result['frames']=[i+frame_base for i in record['frames']]
    result['edges']=[i for i,s in enumerate(row['edge_sources'])if s==record['collider_source']]
    if len(result['edges'])>4:raise ValueError('Great Door edge budget')
    return result

def horizontal_entry_ray(position, offset, gate_name):
    """Hero EnterScene left/right spawn X and FindGroundPointY ray origin.

    The source intentionally ignores entryOffset.y for horizontal ground rays.
    The ray lies two units inside the level relative to the hidden spawn.
    """
    if gate_name.startswith('left'):
        direction=1
    elif gate_name.startswith('right'):
        direction=-1
    else:
        raise ValueError('Horizontal entry requires an authored left/right gate')
    x=position[0]+offset['x']-direction
    return x,[x+2*direction,position[1]]


def entry_contract(source, scene_file='level7', gate_name='left1'):
    """Original horizontal-gate placement, coroutine waits and camera fade contract.

    The only runtime approximation here is 60Hz quantization and a linear PS1
    fade; the source camera/blanker shader pipeline is not imported.
    """
    import dnfile
    from dncil.cil.body.reader import read_method_body_from_bytes
    from scene import Scene
    sc=Scene(source,scene_file)
    gates=[(i,t)for i,(kind,t)in sc.objects.items()if kind=='TransitionPoint' and sc.gos[t['m_GameObject']['m_PathID']]['m_Name']==gate_name]
    if len(gates)!=1:raise ValueError(f'{scene_file}/{gate_name} gate changed')
    gid,gate=gates[0];position=sc.point(gate['m_GameObject']['m_PathID'])
    if gate['entryDelay']!=0 or gate['customFade'] or gate['isADoor']:raise ValueError(f'{scene_file}/{gate_name} entry kind changed')
    x,(ray_x,ray_y)=horizontal_entry_ray(position,gate['entryOffset'],gate_name)
    resource=source.file('resources.assets')
    hero=next(o for o in resource.objects.values()if o.type.name=='MonoBehaviour'and source.typename(o)=='HeroController')
    hc=source.read(hero);go=source.ref(resource,hc['m_GameObject'])
    body=next(source.read(o)for o in (source.ref(resource,c['component'])for c in source.read(go)['m_Component'])if o.type.name=='BoxCollider2D')
    elevation=body['m_Size']['y']/2-body['m_Offset']['y']+.01
    hits=[]
    for sid,(kind,t)in sc.objects.items():
        if kind not in ('BoxCollider2D','PolygonCollider2D','EdgeCollider2D') or not t['m_Enabled'] or t['m_IsTrigger']:continue
        goid=t['m_GameObject']['m_PathID']
        if not sc.active(goid) or sc.gos[goid]['m_Layer']!=8:continue
        if kind=='BoxCollider2D':
            a,b=t['m_Size']['x']/2,t['m_Size']['y']/2
            paths=[[{'x':u,'y':v}for u,v in [(-a,-b),(a,-b),(a,b),(-a,b),(-a,-b)]]]
        elif kind=='EdgeCollider2D':paths=[t['m_Points']]
        else:paths=[p+p[:1]for p in t['m_Points']['m_Paths']]
        for path in paths:
            pts=[sc.point(goid,p['x']+t['m_Offset']['x'],p['y']+t['m_Offset']['y'])for p in path]
            for a,b in zip(pts,pts[1:]):
                if a[0]==b[0] or not min(a[0],b[0])<=ray_x<=max(a[0],b[0]):continue
                y=a[1]+(ray_x-a[0])*(b[1]-a[1])/(b[0]-a[0])
                if 0<=ray_y-y<=10:hits.append((y,sid))
    if not hits:raise ValueError(f'{scene_file}/{gate_name} entry ground ray missed source terrain')
    ground,ground_sid=max(hits)
    # Fail after source code changes instead of silently retaining literal waits.
    pe=dnfile.dnPE(str(source.directory/'Managed/Assembly-CSharp.dll'))
    hero_type=next(t for t in pe.net.mdtables.TypeDef.rows if str(t.TypeName)=='HeroController')
    ctor=next(m.row for m in hero_type.MethodList if str(m.row.Name)=='.ctor')
    ctor_code=read_method_body_from_bytes(pe.get_data(ctor.Rva,100000)).instructions
    if not any(i.offset==0xad and i.opcode.name=='ldc.r4' and i.operand==10. for i in ctor_code):raise ValueError('Hero ground ray distance changed')
    t=next(t for t in pe.net.mdtables.TypeDef.rows if str(t.TypeName)=='<EnterScene>d__487')
    method=next(m.row for m in t.MethodList if str(m.row.Name)=='MoveNext')
    code=read_method_body_from_bytes(pe.get_data(method.Rva,100000)).instructions
    literals={i.offset:float(i.operand)for i in code if i.opcode.name=='ldc.r4'}
    expected=((0x6ab,1.),(0x6b6,2.),(0x6f6,.165),(0x7a6,.2),(0x830,.33)) if gate_name.startswith('left') else ((0x8bf,1.),(0x8ca,2.),(0x90a,.165),(0x9ba,.2),(0xa45,.33))
    for offset,value in expected:
        if abs(literals.get(offset,-999)-value)>1e-6:raise ValueError(f'Hero {gate_name} entry coroutine changed')
    # Validate mirrored arithmetic and forced facing, not just matching literals.
    opcodes={i.offset:i.opcode.name for i in code}
    add_or_sub=(0x6b0,'sub',0x6bb,'add') if gate_name.startswith('left') else (0x8c4,'add',0x8cf,'sub')
    if opcodes.get(add_or_sub[0])!=add_or_sub[1] or opcodes.get(add_or_sub[2])!=add_or_sub[3]:raise ValueError('Hero entry/ray direction changed')
    facing_offset,facing_name=(0x6f0,'FaceRight') if gate_name.startswith('left') else (0x904,'FaceLeft')
    facing=next((i for i in code if i.offset==facing_offset),None)
    if facing is None or facing.opcode.name!='call':raise ValueError('Hero entry facing call changed')
    called=pe.net.mdtables.tables[facing.operand.table].rows[facing.operand.rid-1]
    if str(called.Name)!=facing_name:raise ValueError('Hero entry facing changed')
    fsm=source.read(source.file('level1').objects[9790])['fsm']
    fade=next(s for s in fsm['states']if s['name']=='FadeIn')['actionData']
    if fsm['name']!='CameraFade' or fade['actionNames'][0].rsplit('.',1)[-1]!='CameraFadeInWithDelay' or fade['actionNames'][2].rsplit('.',1)[-1]!='SendEventByName':raise ValueError('Camera fade actions changed')
    def old_scalar(action,name):
        start=fade['actionStartIndex'][action];end=fade['actionStartIndex'][action+1]if action+1<len(fade['actionNames'])else len(fade['paramName'])
        p=next(p for p in range(start,end)if fade['paramName'][p]==name)
        if fade['paramDataType'][p]!=15 or fade['paramByteDataSize'][p]!=0:raise ValueError('Camera legacy float layout changed')
        v=fade['fsmFloatParams'][fade['paramDataPos'][p]]
        if v['useVariable']:raise ValueError('Camera fade now variable')
        return v['value']
    if abs(old_scalar(0,'time')-.5)>1e-6 or abs(old_scalar(2,'delay')-.1)>1e-6:raise ValueError('Camera entry fade changed')
    return dict(spawn=[x,ground+elevation],gate_source=scene_file+':'+str(gid),ground_source=scene_file+':'+str(ground_sid),
                ground_ray=[ray_x,ray_y],settle_ticks=10,fade_delay_ticks=7,fade_ticks=30,
                lead_ticks=12,walk_ticks=math.ceil((.33+1/hc['RUN_SPEED'])*60),speed=hc['RUN_SPEED'],
                source_method='HeroController/<EnterScene>d__487::MoveNext',camera_source='level1:9790',
                limitations=['60Hz wait quantization','linear PS1 fade, not original camera and blanker shader parity'])

def region_literal(slot):
    """Catalogue slot of an entry's spawn; fixtures without regions get usize::MAX."""
    return 'usize::MAX' if slot is None else str(slot)


def generate(metadata,entry,return_entry):
    from world import locate_region
    records=[r.get('great_door')for r in metadata['regions']]
    source=next((r for r in records if r),None)
    if source is None:raise ValueError('Cook the source Great Door first')
    target=next(s['scene_id']for s in metadata['scenes']if s['scene_name']==source['target_scene'])
    def array(v):return '['+','.join(map(str,v))+']'
    q=lambda v:round(v*65536)
    lines=['// Generated from the local Windows Great Door FSM and sprite collection.',
           'pub const TARGET_SCENE:usize='+str(target)+';',
           'pub const PARAMS:Params=Params{stage_hits:'+array(source['stage_hits'])+',cooldown:'+str(source['cooldown_ticks'])+',transition:'+str(source['transition_ticks'])+',entry_delay:'+str(source['entry_delay_ticks'])+'};',
           'pub const ENTRY:Entry=Entry{spawn:'+array(map(q,entry['spawn']))+',region:'+region_literal(locate_region(metadata['regions'],target,entry['spawn']))+',settle:'+str(entry['settle_ticks'])+',fade_delay:'+str(entry['fade_delay_ticks'])+',fade:'+str(entry['fade_ticks'])+',lead:'+str(entry['lead_ticks'])+',walk:'+str(entry['walk_ticks'])+',speed:'+str(q(entry['speed']))+'};',
           'pub const RETURN_ENTRY:Entry=Entry{spawn:'+array(map(q,return_entry['spawn']))+',region:'+region_literal(locate_region(metadata['regions'],0,return_entry['spawn']))+',settle:'+str(return_entry['settle_ticks'])+',fade_delay:'+str(return_entry['fade_delay_ticks'])+',fade:'+str(return_entry['fade_ticks'])+',lead:'+str(return_entry['lead_ticks'])+',walk:'+str(return_entry['walk_ticks'])+',speed:'+str(q(return_entry['speed']))+'};',
           'pub const POSITION:[i32;2]='+array(map(q,source['position'][:2]))+';',
           'pub const SCALE:i32='+str(source['scale'])+';',
           'pub const BOUNDS:[i32;4]='+array(map(q,source['bounds']))+';',
           '// Only the catalogue slots that see the door carry a binding.',
           'pub static REGIONS:&[(u16,Binding)]=&[']
    for slot,record in enumerate(records):
        if record is None:continue
        if slot>=65536:raise ValueError('Great Door binding slot exceeds u16')
        lines.append('('+str(slot)+',Binding{frames:'+array(record['frames'])+',edges:&'+array(record['edges'])+'}),')
    return '\n'.join(lines)+'];\n'

def main():
    metadata=json.loads((ROOT/'data/regions.json').read_text())
    from source import Source
    source=Source();entry=entry_contract(source);return_entry=entry_contract(source,'level6','right1')
    record=next(r['great_door']for r in metadata['regions']if r.get('great_door'))
    file,sid=record['source'].split(':')
    values=contract(source.read(source.file(file).objects[int(sid)])['fsm'])
    for region in metadata['regions']:
        if region.get('great_door'):region['great_door'].update(values)
    (ROOT/'data/great_door.rs').write_text(generate(metadata,entry,return_entry))
    record=dict(record,entry_sequence=entry,return_entry_sequence=return_entry)
    (ROOT/'.hkpsx/great-door.json').write_text(json.dumps(record,indent=2)+'\n')

if __name__=='__main__':main()
