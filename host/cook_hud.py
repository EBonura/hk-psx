"""Source health masks and SOUL orb; static liquid pose, no gameplay uploads."""
import hashlib,json,struct
from PIL import Image,ImageChops
from source import Source,ROOT,dump
from cook import tk_sprite,Atlas
from focus import action_fields

def soul_art(s,textures):
 f=s.file('resources.assets')
 def read(i):return s.read(f.objects[i])
 def position(i):return read(i)['m_LocalPosition']
 clip=next(c for c in read(20665)['clips'] if c['name']=='HUD Frame Idle')
 assert len(clip['frames'])==1
 ref=clip['frames'][0];collection=s.ref(f,ref['spriteCollection'])
 frame,fb=tk_sprite(s,f,s.read(collection),ref['spriteId'],textures)
 fp=position(10329);fb=tuple(v+fp['xy'[i%2]] for i,v in enumerate(fb))
 scale=24/(fb[3]-fb[1]);assert round((fb[2]-fb[0])*scale)==40
 liquid=read(22030);lc=s.ref(f,liquid['collection'])
 li,lb=tk_sprite(s,f,s.read(lc),liquid['_spriteId'],textures)
 mat=read(20);mask_o=s.ref(f,dict(mat['m_SavedProperties']['m_TexEnvs'])['_AlphaTex']['m_Texture'])
 mask=mask_o.read().image.convert('RGBA')
 mt=read(10520);mp=mt['m_LocalPosition'];ms=mt['m_LocalScale'];parent=position(10002)
 # Source projected alpha mask maps its transformed unit quad onto world XY.
 mb=(parent['x']+mp['x']-ms['x']/2,parent['y']+mp['y']-ms['y']/2,
     parent['x']+mp['x']+ms['x']/2,parent['y']+mp['y']+ms['y']/2)
 fsm=read(21294)['fsm'];states={st['name']:st for st in fsm['states']}
 variables={v['name']:v['value'] for group in fsm['variables'].values() if isinstance(group,list)
            for v in group if isinstance(v,dict) and 'name'in v and 'value'in v}
 def actions(state,name):
  d=states[state]['actionData'];return [action_fields(d,i) for i,n in enumerate(d['actionNames'])
      if n.rsplit('.',1)[-1]==name and d['actionEnabled'][i]]
 assert actions('MP Gain','FloatMultiply')[0]['multiplyBy']['name']=='Liquid Y Per MP'
 assert actions('MP Gain','FloatAdd')[0]['add']['name']=='Liquid Bottom Y'
 assert any(a['intName'].get('value')=='focusMP_amount' for a in actions('Check Can Heal','GetPlayerDataInt'))
 eyes_at=actions('Check Eyes','IntCompare')[0]['integer2']['value']
 # Master cooker extracts this PlayerData value for FOCUS_PARAMS too.
 focus=json.load(open(ROOT/'.hkpsx/focus-source.json'))
 source_hash=hashlib.sha256((s.directory/'resources.assets').read_bytes()).hexdigest()
 assert focus['resources_sha256']==source_hash,'stale focus source report'
 cost=focus['cost']
 lt=read(9228);ls=lt['m_LocalScale'];lp=lt['m_LocalPosition']
 bottom=variables['Liquid Bottom Y'];per_mp=variables['Liquid Y Per MP']
 top_zero=parent['y']+bottom+lb[3]*ls['y']
 width=round((mb[2]-mb[0])*scale);height=round((mb[3]-mb[1])*scale)
 vitals=json.load(open(ROOT/'.hkpsx/vitals-source.json'))
 assembly_hash=hashlib.sha256((s.directory/'Managed/Assembly-CSharp.dll').read_bytes()).hexdigest()
 assert focus['assembly_sha256']==vitals['assembly_sha256']==assembly_hash,'stale gameplay source reports'
 max_mp=vitals['max_soul'];assert vitals['focus_cost']==cost and 1<=cost<=max_mp<=99
 liquid_box=(parent['x']+lp['x']+lb[0]*ls['x'],parent['y']+bottom+max_mp*per_mp+lb[1]*ls['y'],
             parent['x']+lp['x']+lb[2]*ls['x'],top_zero+max_mp*per_mp)
 a=li.width/(liquid_box[2]-liquid_box[0]);b=li.height/(liquid_box[3]-liquid_box[1])
 fill=li.transform((width,height),Image.Transform.AFFINE,
      ((mb[2]-mb[0])/width*a,0,(mb[0]-liquid_box[0])*a,
       0,(mb[3]-mb[1])/height*b,(liquid_box[3]-mb[3])*b),Image.Resampling.BILINEAR)
 fill.putalpha(ImageChops.multiply(fill.getchannel('A'),mask.getchannel('A').resize((width,height),Image.Resampling.LANCZOS)))
 es=read(23485);ec=s.ref(f,es['collection'])
 eyes,eb=tk_sprite(s,f,s.read(ec),es['_spriteId'],textures);ep=position(9949)
 eye_wh=(round((eb[2]-eb[0])*scale),round((eb[3]-eb[1])*scale))
 def xy(x,y):return (round((x-fb[0])*scale),round((fb[3]-y)*scale))
 fill_xy=xy(mb[0],mb[3]);eye_xy=xy(ep['x']+eb[0],ep['y']+eb[3])
 d=next(st for st in read(21891)['fsm']['states'] if st['name']=="Can't Heal")['actionData']
 i=d['paramName'].index('toValue');p=d['paramDataPos'][i]
 tint=struct.unpack('<4f',bytes(d['byteData'][p:p+16]));assert tint[0]==tint[1]==tint[2] and tint[3]==1
 gain=round(tint[0]*128)
 cut_zero=round((mb[3]-top_zero)*scale*65536);cut_per_mp=round(per_mp*scale*65536)
 assert 0<=gain<=128 and 0<=cut_zero<=32*65536 and 0<cut_per_mp<65536
 assert abs(cut_zero-max_mp*cut_per_mp)<32*65536
 meta=struct.pack('<4h2i5H',*fill_xy,*eye_xy,cut_zero,cut_per_mp,cost,eyes_at,1,gain,max_mp)+bytes(6)
 report={'frame_library':s.sid(f.objects[20665]),'frame_collection':s.sid(collection),'frame_sprite':ref['spriteId'],
         'liquid_sprite_source':s.sid(f.objects[22030]),'mask_texture':s.sid(mask_o),'eyes_source':s.sid(f.objects[23485]),
         'control_fsm':s.sid(f.objects[21294]),'liquid_y_bottom':bottom,'liquid_y_per_mp':per_mp,
         'focus_cost':cost,'max_soul':max_mp,'eyes_threshold':eyes_at,'under_focus_tint':tint,'fill_offset':fill_xy,'eyes_offset':eye_xy,
         'source_resources_sha256':source_hash,'source_assembly_sha256':assembly_hash,
         'limitations':'Static liquid pose with bottom crop; no liquid waves, tint/eye easing, full flash or entry animation.'}
 return [(frame,40,24),(fill,width,height),(eyes,*eye_wh)],meta,report

def main():
 s=Source();obj=s.file('resources.assets').objects[21155];c=s.read(obj)
 textures={};atlas=Atlas(deduplicate=False)
 for index in (9,0):
  im,box=tk_sprite(s,obj.assets_file,c,index,textures)
  ratio=min(14/im.width,14/im.height)
  atlas.add(im,im.width*ratio,im.height*ratio,streamed=True)
  im.save(ROOT/f'data/hud-source-{index}.png')
 art,meta,report=soul_art(s,textures)
 for index,(im,w,h) in enumerate(art):
  atlas.add(im,w,h,streamed=True);im.save(ROOT/f'data/hud-soul-source-{index}.png')
 atlas.pack();bank=bytearray(b'HKHUD002')
 for e in atlas.entries:bank.extend(struct.pack('<4H',e[3],e[4],e[6],0))
 bank.extend(meta);assert len(bank)==80
 bank.extend(b''.join(atlas.palettes[e[5]] for e in atlas.entries));bank.extend(atlas.stream)
 assert [(e[3],e[4]) for e in atlas.entries[:2]]==[(12,14),(6,14)]
 for e,max_w in zip(atlas.entries,[16,16,40,24,16]):assert e[3]<=max_w and e[4]<=32
 (ROOT/'data/hud.hk').write_bytes(bank)
 report.update({'health_collection_id':s.sid(obj),'health_definition_indices':[9,0],
    'bytes':len(bank),'sha256':hashlib.sha256(bank).hexdigest(),'format':'HKHUD002',
    'textures':[{'width':e[3],'height':e[4],'stream_offset':e[6]}for e in atlas.entries]})
 dump(ROOT/'.hkpsx/hud-provenance.json',report)
 print('Health + SOUL HUD:',len(bank),'bytes')
if __name__=='__main__':main()
