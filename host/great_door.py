"""Observed Tutorial Great Door FSM and source stage art, without generic FSM execution."""
import copy,json,math
from pathlib import Path
from focus import action_fields

ROOT=Path(__file__).resolve().parents[1]

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
    for kind in ('SetCollider','SetMeshRenderer'):
        if literal(one('Activate',kind)['active']):raise ValueError('Great Door persistence activation changed')
    return {'stage_hits':stages,'cooldown_ticks':round(waits[0]*60),'transition_ticks':round(delay*60)+2,
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
    from cook import tk_sprite,FOCAL,CAM_Z,SCENERY_MAX_AXIS
    collection_o=source.ref(scene.file,sprite['collection']);collection=source.read(collection_o);textures={}
    matrix=scene.world(scene.go_transform[gid])
    if any(abs(matrix[i][j]-(1 if i==j else 0))>1e-6 for i in range(3)for j in range(3)):raise ValueError('Great Door transform changed')
    position=scene.point(gid);scale=FOCAL/(position[2]-CAM_Z);indices=[];boxes=[]
    for name in values['stage_names']:
        index=next(i for i,d in enumerate(collection['spriteDefinitions'])if d['name']==name)
        image,box=tk_sprite(source,collection_o.assets_file,collection,index,textures)
        w=(box[2]-box[0])*scale;h=(box[3]-box[1])*scale;factor=min(1,SCENERY_MAX_AXIS/max(w,h))
        texture=atlas.add(image,w*factor,h*factor,streamed=True)
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

def generate(metadata):
    records=[r.get('great_door')for r in metadata['regions']]
    source=next((r for r in records if r),None)
    if source is None:raise ValueError('Cook the source Great Door first')
    target=next(s['scene_id']for s in metadata['scenes']if s['scene_name']==source['target_scene'])
    def array(v):return '['+','.join(map(str,v))+']'
    q=lambda v:round(v*65536)
    lines=['// Generated from the local Windows Great Door FSM and sprite collection.',
           'pub const TARGET_SCENE:usize='+str(target)+';',
           'pub const PARAMS:Params=Params{stage_hits:'+array(source['stage_hits'])+',cooldown:'+str(source['cooldown_ticks'])+',transition:'+str(source['transition_ticks'])+'};',
           'pub const POSITION:[i32;2]='+array(map(q,source['position'][:2]))+';',
           'pub const SCALE:i32='+str(source['scale'])+';',
           'pub const BOUNDS:[i32;4]='+array(map(q,source['bounds']))+';',
           'pub static REGIONS:&[Option<Binding>]=&[']
    for record in records:
        lines.append('None,'if record is None else 'Some(Binding{frames:'+array(record['frames'])+',edges:&'+array(record['edges'])+'}),')
    return '\n'.join(lines)+'];\n'

def main():
    metadata=json.loads((ROOT/'data/regions.json').read_text())
    (ROOT/'data/great_door.rs').write_text(generate(metadata))
    record=next(r['great_door']for r in metadata['regions']if r.get('great_door'))
    (ROOT/'.hkpsx/great-door.json').write_text(json.dumps(record,indent=2)+'\n')

if __name__=='__main__':main()
