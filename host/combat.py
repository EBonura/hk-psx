"""Verified no-charm nail and GrassCut source subset; no generic FSM execution."""
import math

# HealthManager.NonFatalHit (Assembly-CSharp IL) sets evasionByHitRemaining to
# this literal after any hit that does not kill, unless the hit ignores
# invulnerability or plays an alternate hit animation. The serialized
# `invulnerableTime` field is read by no method in the assembly, so it is not
# the evasion window, whatever a placement authors there.
HIT_EVASION_SECONDS = .2

def ticks(seconds):
    # Ceiling in the 60 Hz guest, tolerating serialized float32 roundoff at integers.
    return math.ceil(seconds*60-1e-5)

def grass_sources(sc,bounds=None,errors=None):
    """Supported GrassCut instances in `bounds`.

    With `errors`, an unsupported instance is recorded there (like
    breakable_sources) instead of aborting the whole region cook.
    """
    result=[]
    scene_file=sc.file.name.rsplit('/',1)[-1]
    for i,(typ,t) in sc.objects.items():
        if typ!='GrassCut' or not t['m_Enabled']:continue
        try:
            record=_grass_source(sc,t,i,bounds,scene_file)
        except ValueError as ex:
            if errors is None:raise
            errors.append({'id':f'{scene_file}:{i}','type':'unsupported GrassCut','error':str(ex)});continue
        if record:result.append(record)
    if len(result)>32:raise ValueError('GrassCut pool exceeds 32')
    return result

def _grass_source(sc,t,i,bounds,scene_file):
    gid=t['m_GameObject']['m_PathID']
    if not sc.active(gid):return None
    pos=sc.point(gid)
    # GrassBehaviour.Start destroys collision beyond its gameplay depth band.
    area=bounds or (20,0,70,25)
    if not area[0]<pos[0]<area[2] or not area[1]<pos[1]<area[3] or abs(pos[2]-.004)>1.8:return None
    cols=[(j,c) for j,(ty,c) in sc.objects.items() if ty=='BoxCollider2D' and c['m_GameObject']['m_PathID']==gid and c['m_Enabled']]
    if len(cols)!=1 or not cols[0][1]['m_IsTrigger']:raise ValueError('unsupported GrassCut collider')
    if len(t['disable'])!=1 or len(t['enable'])!=1 or t['disableColliders'] or t['enableColliders']:raise ValueError('unsupported GrassCut renderer/collider topology')
    off,on=[r[0]['m_PathID'] for r in (t['disable'],t['enable'])]
    if any(r[0]['m_FileID'] for r in (t['disable'],t['enable'])):raise ValueError('external GrassCut renderer')
    if sc.objects[off][0]!='SpriteRenderer' or sc.objects[on][0]!='SpriteRenderer':raise ValueError('non-sprite GrassCut renderer')
    c=cols[0][1];sx=c['m_Size']['x']/2;sy=c['m_Size']['y']/2;o=c['m_Offset']
    corners=[sc.point(gid,x+o['x'],y+o['y']) for x,y in [(-sx,-sy),(sx,-sy),(sx,sy),(-sx,sy)]]
    if any(abs(a[0]-b[0])>.005 and abs(a[1]-b[1])>.005 for a,b in zip(corners,corners[1:]+corners[:1])):raise ValueError('rotated GrassCut box unsupported')
    box=(min(p[0] for p in corners),min(p[1] for p in corners),max(p[0] for p in corners),max(p[1] for p in corners))
    if any(not math.isfinite(v) or abs(v)>512 for v in box):raise ValueError('GrassCut world coordinate exceeds Q16 bound')
    return {'source':f'{scene_file}:{i}','collider':f'{scene_file}:{cols[0][0]}','off':off,'on':on,'box':box,'serialized':t}

def nail_sources(s,rf,hero_gid):
    # Identify Knight descendants carrying NailSlash, independent of path IDs.
    result={}
    for o in rf.objects.values():
        if o.type.name!='MonoBehaviour' or s.typename(o)!='NailSlash':continue
        n=s.read(o);go=s.ref(rf,n['m_GameObject']);g=s.read(go)
        if g['m_Name'] not in ('Slash','AltSlash','UpSlash','DownSlash'):continue
        components=[s.ref(rf,c['component']) for c in g['m_Component']]
        tr=next(c for c in components if c.type.name=='Transform');t=s.read(tr)
        parent=s.read(s.ref(rf,t['m_Father']))
        while parent['m_GameObject']['m_PathID']!=hero_gid and parent['m_Father']['m_PathID']:
            # Intermediate 'Attacks' transform must be identity for local bounds.
            if any(parent['m_LocalPosition'][k]!=0 or parent['m_LocalScale'][k]!=1 for k in 'xyz') or parent['m_LocalRotation']!={'x':0.0,'y':0.0,'z':0.0,'w':1.0}:
                raise ValueError('non-identity nail ancestor unsupported')
            parent=s.read(s.ref(rf,parent['m_Father']))
        if parent['m_GameObject']['m_PathID']!=hero_gid:continue
        if t['m_LocalRotation']!={'x':0.0,'y':0.0,'z':0.0,'w':1.0}:raise ValueError('rotated NailSlash unsupported')
        co=next(c for c in components if c.type.name=='PolygonCollider2D');c=s.read(co)
        paths=c['m_Points']['m_Paths']
        if len(paths)!=1 or not 3<=len(paths[0])<=16:raise ValueError('NailSlash polygon budget')
        # StartSlash writes serialized NailSlash.scale over Transform.localScale.
        pos=t['m_LocalPosition'];scale=n['scale'];off=c['m_Offset']
        poly=[(pos['x']+(p['x']+off['x'])*scale['x'],pos['y']+(p['y']+off['y'])*scale['y']) for p in paths[0]]
        if any(not math.isfinite(v) or abs(v)>16 for point in poly for v in point):raise ValueError('NailSlash local polygon exceeds Q16 bound')
        result[g['m_Name']]={'source':s.sid(o),'transform':s.sid(tr),'collider':s.sid(co),'clip':n['animName'],'position':pos,'scale':scale,'polygon':poly,'serialized':n}
    if set(result)!= {'Slash','AltSlash','UpSlash','DownSlash'}:raise ValueError('Knight NailSlash objects missing')
    return [result[k] for k in ('Slash','AltSlash','UpSlash','DownSlash')]

def transformed_box(box,nail):
    p=nail['position'];s=nail['scale']
    return (p['x']+box[0]*s['x'],p['y']+box[1]*s['y'],p['x']+box[2]*s['x'],p['y']+box[3]*s['y'])

def generated_params(constants,dt,nails,grass,draws):
    if any(not math.isfinite(v) or abs(v)>16 for n in nails for pt in n['polygon'] for v in pt):raise ValueError('NailSlash local polygon exceeds Q16 bound')
    if any(not math.isfinite(v) or abs(v)>512 for g in grass for v in g['box']):raise ValueError('GrassCut world coordinate exceeds Q16 bound')
    ids={d['source']:i for i,d in enumerate(draws)}
    out='pub const ATTACK_PARAMS: hk_sim::AttackParams = hk_sim::AttackParams {'
    # HeroController::.ctor sets ATTACK_QUEUE_STEPS to 5, the same shape as
    # JUMP_QUEUE_STEPS; it is a constructor literal, not a serialized field.
    ATTACK_QUEUE_STEPS=5
    values={'duration':ticks(constants['ATTACK_DURATION']),'cooldown':ticks(constants['ATTACK_COOLDOWN_TIME']),'alternate_reset':ticks(constants['ALT_ATTACK_RESET']),
        'hit_start':ticks(dt),'hit_end':ticks(5*dt),
        'queue_ticks':round(ATTACK_QUEUE_STEPS*dt*60),'recovery_ticks':ticks(constants['ATTACK_RECOVERY_TIME'])}
    out+=','.join(f'{k}:{v}' for k,v in values.items())+'};\n'
    out+='pub const NAIL_POLYGONS: [&[[i32;2]];4] = ['
    out+=','.join('&['+','.join(f'[{round(x*65536)},{round(y*65536)}]' for x,y in n['polygon'])+']' for n in nails)+'];\n'
    out+='pub const GRASS: &[hk_sim::Grass] = &[\n'
    for g in grass:
        # Preserve the original helper's default for synthetic callers; every
        # extracted GrassCut now carries its explicit serialized source file.
        scene_file=g.get('source','level6:0').split(':')[0]
        if any(f'{scene_file}:{g[k]}' not in ids for k in ('off','on')):raise ValueError('GrassCut renderer omitted by chamber culling')
        g['off_draw']=ids[f'{scene_file}:{g["off"]}'];g['on_draw']=ids[f'{scene_file}:{g["on"]}']
        out+='hk_sim::Grass {bounds:['+','.join(str(round(v*65536)) for v in g['box'])+f'],off_draw:{g["off_draw"]},on_draw:{g["on_draw"]}'+'},\n'
    return out+'];\n'
