"""Derive the no-charm FocusParams and clip requirements from Windows assets.

No retail payload is embedded here. Generated reports stay in .hkpsx; callers
may append generated_focus_params(values) to the guest's cooked parameters.
"""
import hashlib,json,struct
from combat import ticks
FOCUS_CLIPS=('Focus','Focus Get','Focus End','Focus Get Once')

def fsm_variables(fsm):
    """An FSM's declared variables as name to serialized value.

    PlayMaker lets two variables share a name, and `Spell Control` actually does
    (`Can Cancel` and `Is In Dream Focus` are each declared twice). A dict keyed
    by name silently keeps one of them, which is a wrong answer rather than a
    refusal, so a duplicate whose values disagree raises instead.
    """
    out={}
    for group in fsm['variables'].values():
        if not isinstance(group,list):continue
        for v in group:
            if not isinstance(v,dict) or 'name' not in v:continue
            name=v['name'];value=v.get('value')
            if name in out and out[name]!=value:
                raise ValueError(f'FSM {fsm.get("name")!r} declares {name!r} twice with different values')
            out[name]=value
    return out

def action_parameters(data,index,objects=False):
    """The same fields as `action_fields`, in declaration order with real names.

    An array-shaped action (IntSwitch's compareTo and sendEvent pairs, an event
    list) leaves its elements unnamed, and `action_fields` keys those by their
    index, which turns a dict lookup into positional guesswork. Two separate
    readers have already had to work around that locally, and a misread switch
    is a silent behaviour change rather than a refusal, so read arrays through
    this instead: `[(name_or_None, value), ...]`.
    """
    fields=action_fields(data,index,objects)
    start=data['actionStartIndex'][index]
    end=data['actionStartIndex'][index+1] if index+1<len(data['actionNames']) else len(data['paramName'])
    out=[]
    for i in range(start,end):
        name=data['paramName'][i]
        key=name or str(i)
        if key in fields:
            out.append((name or None,fields[key]))
    return out

def action_fields(data,index,objects=False):
    """Read compact scalar fields used by this observed PlayMaker version."""
    start=data['actionStartIndex'][index]
    end=data['actionStartIndex'][index+1] if index+1<len(data['actionNames']) else len(data['paramName'])
    fields={}
    for i in range(start,end):
        name=data['paramName'][i] or str(i);kind=data['paramDataType'][i]
        pos=data['paramDataPos'][i];size=data['paramByteDataSize'][i]
        raw=bytes(data['byteData'][pos:pos+size])
        if kind in (15,16,17) and size:
            n=1 if kind==17 else 4
            if size<n+1:raise ValueError('truncated compact FSM scalar')
            value={'value':struct.unpack({15:'<f',16:'<i',17:'<?'}[kind],raw[:n])[0],
                   'useVariable':bool(raw[n]),'name':raw[n+1:].decode('utf8')}
        elif kind==23:value=raw.decode('utf8')
        elif kind==19 and objects:
            # An FsmGameObject reference: the same {value, useVariable, name}
            # shape as the compact scalars. Off by default, because the actor
            # recognizers match on the exact field set an action decodes to and
            # a new field changes what they see.
            value=data['fsmGameObjectParams'][pos]
        elif kind in (18,20,21,31,39):
            value=data[{18:'fsmStringParams',20:'fsmOwnerDefaultParams',21:'functionCallParams',31:'fsmEventTargetParams',39:'fsmVarParams'}[kind]][pos]
        elif kind==1 and size==1:value=bool(raw[0])
        else:continue
        fields[name]=value
    return fields

def source_focus_values(source,hero_constants=None):
    import dnfile
    from dncil.cil.body.reader import read_method_body_from_bytes
    file=source.file('resources.assets')
    hero=next(o for o in file.objects.values() if o.type.name=='MonoBehaviour' and source.typename(o)=='HeroController')
    gid=hero.parse_monobehaviour_head().m_GameObject.m_PathID
    fsms=[o for o in file.objects.values() if o.type.name=='MonoBehaviour' and source.typename(o)=='PlayMakerFSM'
          and o.parse_monobehaviour_head().m_GameObject.m_PathID==gid]
    candidates=[(o,source.read(o)['fsm']) for o in fsms]
    fsm_o,fsm=next((o,f) for o,f in candidates if f['name']=='Spell Control')
    states={s['name']:s for s in fsm['states']}
    variables=fsm_variables(fsm)
    def actions(state,kind):
        d=states[state]['actionData']
        return [action_fields(d,i) for i,n in enumerate(d['actionNames'])
                if n.rsplit('.',1)[-1]==kind and d['actionEnabled'][i]]
    def scalar(value):return variables[value['name']] if value['useVariable'] else value['value']
    def wait(state):
        a=actions(state,'Wait');assert len(a)==1 and not a[0]['realTime'];return scalar(a[0]['time'])
    # Bind the actual no-charm branch, rather than the stale default drain value.
    speed=actions('Set Focus Speed','SetFloatValue')[0]
    assert speed['floatVariable']['name']=='Time Per MP Drain'
    assert speed['floatValue']['name']=='Time Per MP Drain UnCH'
    charm=actions('Set Focus Speed','PlayerDataBoolTest')[0]
    assert scalar(charm['boolName'])=='equippedCharm_7' and charm['isFalse']=='FINISHED'
    cost_gate=actions('Can Focus?','IntCompare');assert len(cost_gate)==1
    assert cost_gate[0]['integer1']['name']=='MP' and cost_gate[0]['integer2']['name']=='Focus MP amount'
    assert cost_gate[0]['lessThan']=='CANCEL'
    assert {scalar(a['intName']) for a in actions('Can Focus?','GetPlayerDataInt')}=={'MPCharge','focusMP_amount'}
    initial_grace=actions('First Grace Check','FloatCompare')[0]
    repeat_grace=actions('Grace Check','FloatCompare')[0]
    for state in ['First Grace Check','Grace Check']:
        restore=actions(state,'SendMessage')[0]['functionCall']
        assert restore['FunctionName']=='SetMPCharge' and restore['IntParameter']['name']=='Start MP'
    for state in ['Focus Cancel','Focus Heal','Focus Get Finish','Cancel All']:
        assert any(scalar(a['methodName'])=='StopMPDrain' for a in actions(state,'CallMethodProper'))
    heal=actions('Set HP Amount','SetIntValue')[0];assert heal['intVariable']['name']=='Health Increase'
    animation=None
    for o in file.objects.values():
        if o.type.name=='MonoBehaviour' and source.typename(o)=='tk2dSpriteAnimation':
            t=source.read(o);clips={c['name']:c for c in t['clips']}
            if set(FOCUS_CLIPS).issubset(clips):animation=o;break
    if animation is None:raise ValueError('source focus clips missing')
    if hero_constants is None:
        from UnityPy.helpers import TypeTreeHelper
        node=hero._get_typetree_node();children=node.m_Children
        node.m_Children=children[:next(i for i,n in enumerate(children) if n.m_Name=='hero_state')]
        old=TypeTreeHelper.read_typetree_boost
        try:
            TypeTreeHelper.read_typetree_boost=None
            hero_constants=hero.read_typetree(nodes=node,check_read=False)
        finally:TypeTreeHelper.read_typetree_boost=old;node.m_Children=children
    assembly=source.directory/'Managed/Assembly-CSharp.dll';pe=dnfile.dnPE(str(assembly));methods={};costs=[]
    for typ in pe.net.mdtables.TypeDef.rows:
        for ref in typ.MethodList:
            m=ref.row;key=(str(typ.TypeName),str(m.Name))
            if key not in {('PlayerData','SetupNewPlayerData'),('HeroController','CanFocus'),('HeroController','StartMPDrain'),
                           ('HeroController','StopMPDrain'),('HeroController','Update'),('HeroController','SetMPCharge')}:continue
            body=read_method_body_from_bytes(pe.get_data(m.Rva,100000));methods['.'.join(key)]=hashlib.sha256(bytes(body.code_size.to_bytes(4,'little'))+pe.get_data(m.Rva,body.size)).hexdigest()
            if key==('PlayerData','SetupNewPlayerData'):
                from actors import _literal
                for i,ins in enumerate(body.instructions):
                    if ins.opcode.name=='stfld' and i:
                        token=ins.operand;row=pe.net.mdtables.tables[token.table].rows[token.rid-1]
                        if str(row.Name)=='focusMP_amount':costs.append(_literal(body.instructions[i-1]))
    if len(costs)!=1 or not isinstance(costs[0],int):raise ValueError('focus cost initializer changed')
    values={'hold_seconds':wait('Button Down'),'start_seconds':wait('Focus Start'),
        'drain_seconds':scalar(speed['floatValue']),'heal_seconds':wait('Focus Heal'),
        'cancel_seconds':len(clips['Focus End']['frames'])/clips['Focus End']['fps'],
        'finish_seconds':wait('Focus Get Finish'),'first_grace_seconds':scalar(initial_grace['float2']),
        'repeat_grace_seconds':scalar(repeat_grace['float2']),'cost':costs[0],'heal_amount':scalar(heal['intValue']),
        'attack_recovery_seconds':hero_constants['ATTACK_RECOVERY_TIME'],
        'hero':source.sid(hero),'spell_fsm':source.sid(fsm_o),'animation_library':source.sid(animation),
        'clips':{name:{k:clips[name][k] for k in ['fps','wrapMode','loopStart']}|{'frames':len(clips[name]['frames'])} for name in FOCUS_CLIPS},
        'resources_sha256':hashlib.sha256((source.directory/'resources.assets').read_bytes()).hexdigest(),
        'assembly_sha256':hashlib.sha256(assembly.read_bytes()).hexdigest(),'method_sha256':methods,
        'limitations':['No charms, spells, dream focus, SOUL reserve or particles implemented',
            'Update/PlayMaker event ordering resampled to deterministic60Hz; original-input timing capture not yet available',
            'Grace refund restores cycle-start SOUL; damage/scene interruption bypasses grace',
            'Full health is not an entry rejection in the observed Can Focus? state']}
    generated_focus_params(values) # Reject parameters outside the bounded subset.
    return values

def generated_focus_params(values):
    fields={name+'_ticks':ticks(values[name+'_seconds']) for name in ['hold','start','heal','cancel','finish','first_grace','repeat_grace','attack_recovery']}
    fields|={'drain_interval_us':round(values['drain_seconds']*1_000_000),'cost':values['cost'],'heal_amount':values['heal_amount']}
    if any(type(v)is not int or not 0<v<=65535 for v in fields.values()) or fields['drain_interval_us']<16667:
        raise ValueError('focus values exceed the bounded no-charm implementation')
    return 'pub const FOCUS_PARAMS: hk_sim::FocusParams = hk_sim::FocusParams {'+','.join(f'{k}:{v}' for k,v in fields.items())+'};\n'

if __name__=='__main__':
    from source import Source,ROOT
    values=source_focus_values(Source())
    (ROOT/'.hkpsx/focus-source.json').write_text(json.dumps(values,indent=2))
    (ROOT/'.hkpsx/focus-params.rs').write_text(generated_focus_params(values))
    print(generated_focus_params(values),end='')
