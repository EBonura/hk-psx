#!/usr/bin/env python3
"""Audit local cooked textures; write ignored comparisons, never modify game data.

Run: .venv/bin/python tools/audit_texture_similarity.py
Requires Pillow/numpy. Identity retains every PSX texel bit. Near-match scores
are foreground-union RGB RMSE across black/mid/white backgrounds, using the
actual PSX blend mode. They are a ranking, not a perceptual-equivalence proof.
"""
import argparse, collections, hashlib, json, struct, sys
from pathlib import Path
import numpy as np
from PIL import Image, ImageDraw, ImageFont
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from region_delta import layout, textures
from materials import binary_black_palette
sys.path.insert(0,str(ROOT/'tools'))
from audit_resident_bank import dense_pack, aligned


def words(blob):
    w,h=struct.unpack_from('<HH',blob);p=np.frombuffer(blob[4:36],dtype='<u2');b=np.frombuffer(blob[36:],dtype=np.uint8).reshape(h,(w+1)//2)
    idx=np.empty((h,((w+1)//2)*2),dtype=np.uint8);idx[:,::2]=b&15;idx[:,1::2]=b>>4
    return p[idx[:,:w]]


def composite(tex,bg):
    q=tex['words'];rgb=np.stack([q&31,(q>>5)&31,(q>>10)&31],axis=-1).astype(np.float32)
    # BLACK_AVERAGE shader sentinel has an explicitly corrected red endpoint.
    if tex['black']:rgb[:]=0
    background=np.full_like(rgb,bg)
    if tex['modes']==(1,):blended=np.floor((background+rgb)/2)
    else:blended=np.minimum(31,background+rgb)
    return np.where((q==0)[...,None],background,np.where(((q&32768)!=0)[...,None],blended,rgb))


def metrics(a,b):
    union=(a['words']!=0)|(b['words']!=0);den=max(1,int(union.sum()));diff=[]
    for bg in (0,12,31):
        d=composite(a,bg)-composite(b,bg);diff.append(float(np.sum(d[union]**2)/(den*3)))
    score=100*(1-(max(diff)**.5)/31)
    coverage=float(np.count_nonzero((a['words']!=0)!=(b['words']!=0))/den)
    stp=float(np.count_nonzero(((a['words']&32768)!=0)!=((b['words']&32768)!=0))/den)
    changed=float(np.count_nonzero(a['words']!=b['words'])/den)
    return dict(similarity=score,coverage_mismatch=coverage,stp_mismatch=stp,changed_texels=changed)


def inventory(meta):
    items=[];byblob={};refs=0
    for room in meta['regions']:
        raw=(ROOT/room['path']).read_bytes()
        if hashlib.sha256(raw).hexdigest()!=room['sha256']:raise ValueError('Stale room metadata')
        c,p,ps,ss=layout(raw);blobs=textures(raw);modes=collections.defaultdict(set);names=collections.defaultdict(set);sprites=collections.defaultdict(set)
        at=40+c[1]*16
        for j in range(c[2]):
            record=raw[at+j*44:at+(j+1)*44];modes[struct.unpack_from('<H',record)[0]].add(record[43])
        source_path=(ROOT/room.get('base_path',room['path'])).with_name('scene.json')
        if source_path.exists():
            s=json.loads(source_path.read_text());mp=room.get('texture_deduplication',{}).get('old_to_canonical',list(range(c[1])))
            for d in s['draws']:
                old=d['texture']
                if old<len(mp):
                    names[mp[old]].add(d.get('name',d.get('sprite','unknown')));sprites[mp[old]].add(d.get('sprite','unknown'))
        for i,blob in enumerate(blobs):
            stream=struct.unpack_from('<H',raw,40+i*16)[0]==65535;refs+=1
            key=(stream,blob)
            if key not in byblob:
                tid=len(items);byblob[key]=tid;w,h=struct.unpack_from('<HH',blob)
                items.append(dict(id=tid,blob=blob,words=words(blob),w=w,h=h,stream=stream,black=binary_black_palette(blob[4:36]),modes=set(),names=set(),sprites=set(),rooms=set(),references=[],covers=set()))
            t=items[byblob[key]];t['modes'].update(modes[i]);t['names'].update(names[i]);t['sprites'].update(sprites[i]);t['rooms'].add(room['scene_id']);t['references'].append([room['chunk_id'],i])
            if not stream:
                cover=struct.unpack_from('<I',raw,40+i*16+12)[0];t['covers'].add(raw[ss+cover:ss+cover+20])
    for t in items:t['modes']=tuple(sorted(t['modes']));t['covers']=tuple(sorted(t['covers']))
    return items,refs


def safeclass(t):return t['stream'],t['black'],t['modes'],t['w'],t['h']
def exactkey(t):return safeclass(t),t['covers'],t['words'].tobytes()
def brief(t):return dict(id=t['id'],dimensions=[t['w'],t['h']],streamed=t['stream'],black_mask=t['black'],material_modes=t['modes'],names=sorted(t['names'])[:8],source_sprites=sorted(t['sprites']),source_locations=t['references'],sha256=hashlib.sha256(t['blob']).hexdigest())
def rectangles(items,ids):return[(aligned(items[i]['w']),items[i]['h'],i) for i in ids if not items[i]['stream']]


def font(size):
    for p in ('/System/Library/Fonts/Supplemental/Arial.ttf','/System/Library/Fonts/Helvetica.ttc'):
        if Path(p).exists():return ImageFont.truetype(p,size)
    return ImageFont.load_default()


def tile(t,scale=4):
    # Two neutral backgrounds show both dark silhouettes and bright art.
    q=t['words'];yy,xx=np.indices(q.shape);bg=np.where(((xx//6+yy//6)%2)==0,10,17)
    a=composite(t,0);b=composite(t,31);rgb=np.empty_like(a)
    for val in (10,17):rgb[bg==val]=composite(t,val)[bg==val]
    im=Image.fromarray(np.uint8(np.clip(rgb*255/31,0,255)))
    return im.resize((t['w']*scale,t['h']*scale),Image.Resampling.NEAREST)


def sheet(path,title,pairs,items):
    width=1080;row=260;im=Image.new('RGB',(width,95+row*len(pairs)),(18,22,29));d=ImageDraw.Draw(im)
    d.text((24,18),title,font=font(25),fill='white');d.text((24,54),'Cooked 4bpp textures | 4x nearest pixels | differences at right amplified 8x',font=font(16),fill='#b9c8d8')
    for n,p in enumerate(pairs):
        a,b=items[p['a']],items[p['b']];y=95+n*row
        d.line((24,y,width-24,y),fill='#435165')
        for x,t in ((24,a),(288,b)):
            d.text((x,y+10),f"#{t['id']}  {t.get('original_dimensions',(t['w'],t['h']))[0]} x {t.get('original_dimensions',(t['w'],t['h']))[1]}",font=font(17),fill='#b9c8d8')
            thumb=tile(t);im.paste(thumb,(x,y+40))
        delta=np.max(np.stack([np.abs(composite(a,k)-composite(b,k)) for k in (0,12,31)]),axis=0)
        di=Image.fromarray(np.uint8(np.minimum(255,delta*255/31*8))).resize((a['w']*4,a['h']*4),Image.Resampling.NEAREST)
        im.paste(di,(552,y+40));d.text((552,y+10),'Difference x8',font=font(17),fill='#b9c8d8')
        x=805
        for j,txt in enumerate([f"{p['similarity']:.3f}% similarity",f"{p['changed_texels']*100:.1f}% texels differ",f"{p['coverage_mismatch']*100:.2f}% edge mismatch",f"{p['stp_mismatch']*100:.2f}% blend mismatch",'ANIMATION' if a['stream'] else 'BLACK MASK' if a['black'] else 'STATIC SCENERY']):
            d.text((x,y+40+j*25),txt,font=font(16),fill='white' if j==0 else '#b9c8d8')
        label=' / '.join(sorted(a['names'])[:1]+sorted(b['names'])[:1])
        d.text((24,y+235),label[:118] or 'Source locations are recorded in report.json',font=font(14),fill='#b9c8d8')
    im.save(path)


def normalized(t):
    result=dict(t)
    # Common texel grid for comparison only. Source draw geometry stays intact.
    result['original_dimensions']=(t['w'],t['h'])
    result['words']=np.array(Image.fromarray(t['words']).resize((48,48),Image.Resampling.NEAREST),dtype=np.uint16)
    result['w']=result['h']=48
    return result


def normalized_audit(items,output):
    norm=[normalized(t) for t in items]
    groups=collections.defaultdict(set)
    for t in items:
        if t['stream'] or t['black'] or np.all((t['words']&32767)<=1) or len(t['modes'])!=1:continue
        for sprite in t['sprites']:groups[(sprite,t['modes'])].add(t['id'])
    pairs={}
    for ids in groups.values():
        ids=sorted(ids)
        for n,a in enumerate(ids):
            for b in ids[n+1:]:
                if (a,b)in pairs:continue
                same_size=(items[a]['w'],items[a]['h'])==(items[b]['w'],items[b]['h'])
                pairs[a,b]=dict(a=a,b=b,comparison='native' if same_size else 'normalized',**(metrics(items[a],items[b])if same_size else metrics(norm[a],norm[b])))
    ranked=sorted(pairs.values(),key=lambda p:-p['similarity'])
    selected=[];seen=set()
    for p in ranked:
        a,b=items[p['a']],items[p['b']]
        if (a['w'],a['h'])==(b['w'],b['h']) or min(a['w'],a['h'],b['w'],b['h'])<12:continue
        if p['a']in seen or p['b']in seen:continue
        selected.append(p);seen.update((p['a'],p['b']))
        if len(selected)==4:break
    if selected:
        sheet(output/'size-variants.png','Same original sprite, different cooked sizes (normalized preview)',selected,norm)
    # Deliberately lossy proposal: <=1% coverage / blend mismatch. No chain
    # clustering; each eliminated entry must pass against its retained texture.
    stats=[]
    for threshold in (99,97,95,92,90,85):
        reps={};kept=[]
        for t in sorted(items,key=lambda t:(-t['w']*t['h'],t['id'])):
            i=t['id'];choice=None
            for j in kept:
                m=pairs.get(tuple(sorted((i,j))))
                if m and m['similarity']>=threshold and m['coverage_mismatch']<=.01 and m['stp_mismatch']<=.01:
                    choice=j;break
            if choice is None:kept.append(i)
            else:reps[i]=choice
        scenes=[]
        for scene in sorted({s for t in items for s in t['rooms']}):
            src={t['id'] for t in items if scene in t['rooms']};dst={reps.get(i,i)for i in src}
            pages,_=dense_pack(rectangles(items,dst));saved=sum(aligned(items[i]['w'])//2*items[i]['h']for i in src if not items[i]['stream'])-sum(aligned(items[i]['w'])//2*items[i]['h']for i in dst if not items[i]['stream'])
            scenes.append(dict(scene_id=scene,removed_textures=len(src)-len(dst),pages=pages,static_pixel_bytes_saved=saved))
        stats.append(dict(threshold=threshold,removed_textures=len(reps),scenes=scenes,mapping=reps))
    return dict(same_source_pairs=len(pairs),candidates=ranked,thresholds=stats,samples=selected,method='Same original sprite and material only. 48x48 nearest-normalized comparison; preserve higher-area representative. Coverage and STP mismatches each <=1%. Lossy proposal only; source draw geometry is not altered. Native GPU in-game validation still required.')


def run(output):
    output.mkdir(parents=True,exist_ok=True);metadata=(ROOT/'data/regions.json').read_bytes();meta=json.loads(metadata)
    items,refs=inventory(meta);exact={};aliases={};exactpairs=[]
    for t in items:
        k=exactkey(t)
        if k in exact:
            aliases[t['id']]=exact[k];exactpairs.append(dict(a=exact[k],b=t['id'],**metrics(items[exact[k]],t)))
        else:exact[k]=t['id']
    groups=collections.defaultdict(list)
    for t in items:
        if t['id'] not in aliases:groups[safeclass(t)].append(t['id'])
    candidates=[]
    for ids in groups.values():
        for n,a in enumerate(ids):
            for b in ids[n+1:]:
                m=metrics(items[a],items[b])
                if m['similarity']>=95:candidates.append(dict(a=a,b=b,**m))
    candidates.sort(key=lambda p:(-p['similarity'],p['a'],p['b']))
    stats=[]
    for threshold in (100,99.9,99.5,99,98,97,95):
        mapping=dict(aliases);kept=[]
        for t in items:
            i=t['id']
            if i in mapping:continue
            # Direct comparisons to the final representative, never chained
            # A~B~C clustering that can exceed the requested error threshold.
            chosen=None
            if not t['stream'] and not t['black'] and len(t['modes'])==1:
                for j in kept:
                    other=items[j]
                    if safeclass(t)!=safeclass(other) or t['covers']!=other['covers']:continue
                    m=metrics(t,other)
                    if m['similarity']>=threshold and m['coverage_mismatch']==0 and m['stp_mismatch']==0:
                        chosen=j;break
            if chosen is None:kept.append(i)
            else:mapping[i]=chosen
        rows=[]
        for scene in sorted({r['scene_id']for r in meta['regions']}):
            original={t['id'] for t in items if scene in t['rooms']};ids={mapping.get(i,i)for i in original}
            while any(i in mapping for i in ids):ids={mapping.get(i,i)for i in ids}
            pages,_=dense_pack(rectangles(items,ids));oldpages,_=dense_pack(rectangles(items,original))
            pixelbytes=lambda ids:sum(aligned(items[i]['w'])//2*items[i]['h'] for i in ids if not items[i]['stream'])
            rows.append(dict(scene_id=scene,baseline_pages=oldpages,pages=pages,removed_textures=len(original)-len(ids),static_pixel_bytes_saved=pixelbytes(original)-pixelbytes(ids)))
        stats.append(dict(threshold=threshold,removed_textures=len(mapping),scenes=rows))
    def select(pool,count=4):
        result=[];seen=set()
        for p in pool:
            if p['a']in seen or p['b']in seen:continue
            if min(items[p['a']]['w'],items[p['a']]['h'])<12:continue
            result.append(p);seen.update((p['a'],p['b']))
            if len(result)==count:break
        return result
    selections={'exact':select(exactpairs),'conservative':select(p for p in candidates if p['similarity']<100 and p['similarity']>=99.5 and not items[p['a']]['black'] and not items[p['a']]['stream'] and p['coverage_mismatch']==0 and p['stp_mismatch']==0),
        'threshold_99':select(p for p in candidates if 99<=p['similarity']<99.5 and not items[p['a']]['stream'] and not items[p['a']]['black']),
        'risky':select(p for p in candidates if (items[p['a']]['stream'] or p['coverage_mismatch']>0) and p['similarity']<99.5)}
    for name,pairs in selections.items():
        if pairs:sheet(output/(name+'.png'),{'exact':'Exact render duplicates','conservative':'Very close static textures: at least 99.5%','threshold_99':'Static textures: 99% to 99.5%','risky':'High similarity can still hide meaningful differences'}[name],pairs,items)
    normreport=normalized_audit(items,output)
    report=dict(normalized=normreport,metadata_sha256=hashlib.sha256(metadata).hexdigest(),cooker_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),room_count=len(meta['regions']),texture_references=refs,byte_distinct_textures=len(items),exact_render_distinct=len(exact),exact_aliases=aliases,thresholds=stats,samples=selections,candidates=candidates,textures=[brief(t)for t in items],method='Same dimensions/storage/material. 100*(1 - worst-background foreground-union RGB RMSE/31), PSX additive or average blend, neutral tint. Threshold savings require identical transparency and STP masks and alpha covers; static non-black only; direct final-representative comparisons. Exact aliases preserve all 16 texel bits. Estimates are repacked offline and do not measure PS1 performance.')
    (output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k]for k in ('room_count','texture_references','byte_distinct_textures','exact_render_distinct','thresholds')},indent=2))
    print('Normalized thresholds:',json.dumps([{k:v for k,v in r.items()if k!='mapping'}for r in normreport['thresholds']],indent=2))
    print('Samples:',output)

if __name__=='__main__':
    ap=argparse.ArgumentParser();ap.add_argument('--output',type=Path,default=ROOT/'.hkpsx/texture-similarity');a=ap.parse_args();run(a.output)
