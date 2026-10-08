"""Cook original tablet triggers, localized pages and a tiny original-font strip."""
import base64, hashlib, io, json, math, re, shutil, struct, subprocess
from pathlib import Path
import xml.etree.ElementTree as ET
from PIL import Image, ImageDraw, ImageFont
from source import Source, ROOT
from scene import Scene
from breakables import _components, collider_polygons
from focus import action_fields


def language_sheet(source, name):
    # Derive the installed reader's key rather than distributing it or retail text.
    import dnfile
    from dncil.cil.body.reader import read_method_body_from_bytes
    assembly=source.directory/'Managed/Assembly-CSharp.dll'
    pe=dnfile.dnPE(str(assembly));typ=next(t for t in pe.net.mdtables.TypeDef.rows if str(t.TypeName)=='StringEncrypt')
    methods={str(m.row.Name):m.row for m in typ.MethodList};keys=[]
    for i in read_method_body_from_bytes(pe.get_data(methods['.cctor'].Rva,100000)).instructions:
        if i.opcode.name=='ldstr':keys.append(pe.net.user_strings.get(i.operand.rid).value.encode('utf8'))
    body=read_method_body_from_bytes(pe.get_data(methods['DecryptData'].Rva,100000));settings={};ctor=None
    for n,i in enumerate(body.instructions):
        if i.opcode.name in ('newobj','callvirt'):
            row=pe.net.mdtables.tables[i.operand.table].rows[i.operand.rid-1]
            if i.opcode.name=='newobj':ctor=str(row.Class.row.TypeNamespace)+'.'+str(row.Class.row.TypeName)
            if str(row.Name) in ['set_Mode','set_Padding']:settings[str(row.Name)]=body.instructions[n-1].opcode.name
    if len(keys)!=1 or len(keys[0])!=32 or ctor!='System.Security.Cryptography.RijndaelManaged' or settings!={'set_Mode':'ldc.i4.2','set_Padding':'ldc.i4.2'}:
        raise ValueError('Unrecognized installed language reader')
    f=source.file('resources.assets');matches=[o for o in f.objects.values() if o.type.name=='TextAsset' and o.read().m_Name==name]
    if len(matches)!=1:raise ValueError('Ambiguous source language sheet')
    obj=matches[0];raw=base64.b64decode(source.read(obj)['m_Script'],validate=True)
    openssl=shutil.which('openssl')
    if not openssl:raise ValueError('OpenSSL is required to read the installed language sheet')
    xml=subprocess.run([openssl,'enc','-d','-aes-256-ecb','-K',keys[0].hex()],input=raw,capture_output=True,check=True).stdout
    entries={e.attrib['name']:e.text or '' for e in ET.fromstring(xml)}
    return entries,{'source':source.sid(obj),'sha256':hashlib.sha256(obj.get_raw_data()).hexdigest(),
                    'reader_sha256':hashlib.sha256(assembly.read_bytes()).hexdigest()}


def wrap_page(text, advances, width=272, lines_per_page=8):
    text=text.replace('<br>','\n').replace('’',"'").replace('‘',"'").replace('“','"').replace('”','"')
    if '<' in text or '>' in text or any(ord(c)>126 or (ord(c)<32 and c!='\n') for c in text):
        raise ValueError('Unsupported tablet text markup/character')
    lines=[]
    for para in text.split('\n'):
        line=''
        for word in para.split():
            candidate=(line+' '+word).strip()
            if sum(advances[ord(c)-32] for c in candidate)>width:
                if not line:raise ValueError('Tablet word exceeds panel width')
                lines.append(line);line=word
                if sum(advances[ord(c)-32] for c in line)>width:raise ValueError('Tablet word exceeds panel width')
            else:line=candidate
        lines.append(line)
    while lines and not lines[-1]:lines.pop()
    return [lines[i:i+lines_per_page] for i in range(0,len(lines),lines_per_page)] or [[]]


# A door whose room is not on the disc. The source's own locked doors are the
# model: Dirtmouth's `Jiji Door` (Conversation Control) keeps door_jiji's
# collider off, shows an Inspect prompt and, with no Simple Key, opens a
# dialogue box on `Prompts` JIJI_DOOR_NOKEY. Doors the source locks keep their
# own line; a door the source would simply open shows its own Enter prompt and
# a line of this port's, because the room behind it is not on the disc.
LOCKED_DOORS={'door_jiji':('Inspect','JIJI_DOOR_NOKEY'),'door_tram':('Inspect','TRAM_DOOR_NOPASS')}
OFF_DISC_LINE='This room is not on this disc.'


def door_points(source,advances):
    report=json.loads((ROOT/'data/regions.json').read_text())
    on_disc={s['scene_name'] for s in report['scenes']};prompts=None;points=[];scenes={}
    for scene in report['scenes']:
        for gate in scene['gates']:
            name=gate['name'];control=gate.get('door_control')
            # Door Control's own target, which is what a door actually loads
            # (Crossroads_01's well door serializes another).
            if (not name.startswith('door') or 'dreamReturn' in name or not gate['enabled'] or control is None
                    or control['target_scene'] in on_disc or 'trigger_bounds' not in gate):
                continue
            sc=scenes.get(scene['file']) or scenes.setdefault(scene['file'],Scene(source,scene['file']))
            pid=int(gate['source'].split(':')[1]);data=source.read(sc.file.objects[pid])
            gid=data['m_GameObject']['m_PathID'] if 'm_GameObject' in data else pid
            children=[sc.transforms[v['m_PathID']]['m_GameObject']['m_PathID'] for v in sc.transforms[sc.go_transform[gid]]['m_Children']]
            markers=[c for c in children if sc.gos[c]['m_Name']=='Prompt Marker']
            if len(markers)!=1:raise ValueError(f'{scene["scene_name"]} {name}: no single Prompt Marker')
            if name in LOCKED_DOORS:
                label,key=LOCKED_DOORS[name]
                if prompts is None:prompts,_=language_sheet(source,'EN_Prompts')
                text=prompts[key]
            else:
                label,key,text='Enter','',OFF_DISC_LINE
            x0,y0,x1,y1=gate['trigger_bounds']
            points.append({'source':gate['source'],'scene':scene['scene_id'],'polygon':[[x0,y0],[x1,y0],[x1,y1],[x0,y1]],
                           'marker':sc.point(markers[0])[:2],'key':key or name,'label':label,
                           'pages':wrap_page(text,advances),'door':name,'target':control['target_scene']})
    return points


def cook():
    source=Source();sc=Scene(source,'level6');entries,sheet=language_sheet(source,'EN_Lore Tablets')
    f=source.file('resources.assets');font_obj=next(o for o in f.objects.values() if o.type.name=='Font' and o.read().m_Name=='Perpetua')
    font=ImageFont.truetype(io.BytesIO(bytes(source.read(font_obj)['m_FontData'])),12)
    advances=[max(1,math.ceil(font.getlength(chr(c)))) for c in range(32,127)]
    if max(advances)>12:raise ValueError('Source font exceeds reserved cell')
    pixels=[];previews=[]
    for page in range(5):
        im=Image.new('L',(256,12));d=ImageDraw.Draw(im)
        for col in range(21):
            c=32+page*21+col
            if c<=126:d.text((col*12,-1),chr(c),font=font,fill=255)
        values=bytes((v+8)//17 for v in im.tobytes())
        pixels.extend(values[i]|(values[i+1]<<4) for i in range(0,len(values),2));previews.append(im)
    palette=[0]+[((v*17>>3)*0x421) for v in range(1,16)]
    blob=struct.pack('<16H',*palette)+bytes(pixels)
    assert len(blob)==7712
    points=[]
    for ident,(kind,t) in sc.objects.items():
        if kind!='PlayMakerFSM':continue
        fsm=t['fsm'];variables={v['name']:v['value'] for v in fsm['variables']['stringVariables']}
        if variables.get('Sheet Name')!='Lore Tablets':continue
        gid=t['m_GameObject']['m_PathID']
        if not sc.active(gid):continue
        key=variables['Convo Name'];states={st['name']:st for st in fsm['states']}
        def actions(name):
            d=states[name]['actionData'];return [(n.rsplit('.',1)[-1],action_fields(d,i)) for i,n in enumerate(d['actionNames']) if d['actionEnabled'][i]]
        listens=[v for n,v in actions('In Range') if n=='ListenForUp']
        if len(listens)!=1 or listens[0]['wasPressed']!='UP PRESSED' or listens[0]['isPressed']:
            raise ValueError('Unknown tablet input contract')
        methods=[v['methodName']['value'] for n,v in actions('Can Inspect?') if n=='CallMethodProper']
        if methods!=['CanInput']+['GetState']*6:raise ValueError('Unknown tablet eligibility contract')
        calls=[v for n,v in actions('Send Text') if n=='CallMethodProper']
        if len(calls)!=1 or calls[0]['methodName']['value']!='StartConversation':raise ValueError('Unknown dialogue dispatch')
        colliders=[(i,k,c) for i,k,c in _components(sc,gid) if k=='BoxCollider2D' and c['m_Enabled'] and c['m_IsTrigger']]
        if len(colliders)!=1:raise ValueError('Unknown tablet trigger')
        ci,ck,ct=colliders[0];polys=collider_polygons(sc,gid,ck,ct)
        if len(polys)!=1:raise ValueError('Unknown tablet trigger shape')
        transform=sc.transforms[sc.go_transform[gid]]
        markers=[sc.transforms[v['m_PathID']]['m_GameObject']['m_PathID'] for v in transform['m_Children'] if sc.gos[sc.transforms[v['m_PathID']]['m_GameObject']['m_PathID']]['m_Name']=='Prompt Marker']
        if len(markers)!=1:raise ValueError('Unknown prompt marker')
        label=next(v['setValue']['value'] for n,v in actions('Init') if n=='SetFsmString')
        pages=[wrapped for page in entries[key].split('<page>') for wrapped in wrap_page(page,advances)]
        if not 1<=len(pages)<=8:raise ValueError('Tablet page budget exceeded')
        if any(sum(len(line.replace(' ','')) for line in page)+len('X:NextO:Close')>384 for page in pages):
            raise ValueError('Tablet glyph packet budget exceeded')
        points.append({'source':source.sid(sc.file.objects[ident]),'source_sha256':hashlib.sha256(sc.file.objects[ident].get_raw_data()).hexdigest(),
                       'collider':source.sid(sc.file.objects[ci]),'scene':0,'polygon':polys[0],
                       'marker':sc.point(markers[0])[:2],'key':key,'label':label,'pages':pages})
    if len(points)!=3:raise ValueError('Expected three supported Tutorial tablets')
    points+=door_points(source,advances)
    out=['// Generated from the local Windows source.','pub static POINTS:&[ReadPoint]=&[']
    for p in sorted(points,key=lambda p:(p['scene'],p['key'])):
        poly=','.join('['+','.join(str(round(v*65536)) for v in xy)+']' for xy in p['polygon'])
        marker=','.join(str(round(v*65536)) for v in p['marker'])
        pages=','.join('&['+','.join(json.dumps(line) for line in page)+']' for page in p['pages'])
        out.append('ReadPoint{scene:'+str(p['scene'])+',source_id:'+p['source'].split(':')[1]+',polygon:&['+poly+'],marker:['+marker+'],pages:&['+pages+'],label:'+json.dumps(p['label'])+'},')
    out.extend(['];','pub const ADVANCES:[u8;95]=['+','.join(map(str,advances))+'];'])
    (ROOT/'data/read_points.rs').write_text('\n'.join(out)+'\n');(ROOT/'data/read_font.hk').write_bytes(blob)
    audit=ROOT/'.hkpsx/read-points';audit.mkdir(exist_ok=True)
    preview=Image.new('L',(256,60))
    for i,im in enumerate(previews):preview.paste(im,(0,i*12))
    preview.save(audit/'font.png')
    (audit/'provenance.json').write_text(json.dumps({'points':points,'sheet':sheet,'font_source':source.sid(font_obj),'font_sha256':hashlib.sha256(font_obj.get_raw_data()).hexdigest(),
      'outputs':{name:hashlib.sha256((ROOT/'data'/name).read_bytes()).hexdigest() for name in ['read_points.rs','read_font.hk']},
      'adaptations':['Bounded static page layout at320x240','Cross advances, Circle closes','Original hero alignment/turn animation, dialogue sounds and Focus-specific prompt art remain unimplemented']},indent=2)+'\n')
    print('Cooked three original tablet interactions and',len(points)-3,'closed doors;',sum(len(p['pages']) for p in points),'pages; font7712bytes')

if __name__=='__main__':cook()
