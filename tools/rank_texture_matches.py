#!/usr/bin/env python3
"""Rank measured matches and cook an explicit local static-only threshold trial.

Requires audit_texture_similarity.py report. No playable disc files are written.
"""
import argparse,base64,csv,hashlib,html,io,json,struct,sys
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'));sys.path.insert(0,str(ROOT/'host'))
from audit_texture_similarity import inventory,normalized,tile,dense_pack,rectangles
from texture_dedup import deduplicate_room
from region_delta import textures,layout
from texture_match_groups import review_groups


def build(threshold=95,trial=False):
 out=ROOT/'.hkpsx/texture-similarity';report=json.loads((out/'report.json').read_text());rawmeta=(ROOT/'data/regions.json').read_bytes()
 if hashlib.sha256(rawmeta).hexdigest()!=report['metadata_sha256']:raise ValueError('Audit is stale; regenerate')
 meta=json.loads(rawmeta);items,_=inventory(meta)
 expanded=out/'all-static-matches.json'
 source=json.loads(expanded.read_text())if expanded.exists()else report['normalized']
 if expanded.exists() and source['metadata_sha256']!=report['metadata_sha256']:raise ValueError('Expanded audit stale')
 if threshold<95 and not expanded.exists():raise ValueError('Generate expanded audit first')
 pairs=[p for p in source['candidates']if p['similarity']>=threshold]
 # Rank native comparisons at native size, resizing only when dimensions differ.
 pairs.sort(key=lambda p:(-p['similarity'],p['a'],p['b']))
 def label(t):return sorted(t['names'])[0]if t['names']else f"Texture {t['id']}"
 def thumbnail(t):
  im=tile(normalized(t));b=io.BytesIO();im.save(b,format='PNG');return 'data:image/png;base64,'+base64.b64encode(b.getvalue()).decode()
 md=['# Most similar static textures','',f'{len(pairs)} pairs score at least {threshold:g}%. RGB error is measured over visible pixels on black, mid-grey and white backgrounds. Different-sized textures are nearest-normalized to48×48 for comparison; same-sized textures use original pixels. These are custom comparison scores, not percentages of identical pixels.','', '| Rank | Similarity | Texture A | Size A | Texture B | Size B | Comparison |','|---:|---:|---|---|---|---|---|'];rows=[];cards=[]
 for rank,p in enumerate(pairs,1):
  a,b=(items[p[k]]for k in ('a','b'));row=[rank,round(p['similarity'],3),label(a),f"{a['w']}×{a['h']}",label(b),f"{b['w']}×{b['h']}",p.get('comparison','normalized')];rows.append(row)
  md.append('| '+' | '.join(str(x)for x in row)+' |')
 (out/'ranked-matches.md').write_text('\n'.join(md)+'\n')
 with (out/'ranked-matches.csv').open('w')as f:
  w=csv.writer(f);w.writerow(['rank','similarity','texture_a','size_a','texture_b','size_b','comparison']);w.writerows(rows)
 included={p[k]for p in pairs for k in ('a','b')}
 payload=dict(threshold=threshold,textures=[dict(id=t['id'],name=label(t),w=t['w'],h=t['h'],search=(' '.join(sorted(t['names']))+' '+str(t['id'])).lower(),image=thumbnail(t))for t in items if t['id']in included],pairs=pairs)
 template=(ROOT/'tools/texture_gallery.html').read_text()
 encoded=json.dumps(payload,separators=(',',':')).replace('<','\\u003c')
 (out/'ranked-matches.html').write_text(template.replace('__DATA__',encoded))
 groups=review_groups(pairs,items,threshold)
 (out/'match-groups.json').write_text(json.dumps(dict(threshold=threshold,groups=groups),indent=2)+'\n')
 print('Textures:',len(included),'Groups:',len(groups),'Largest:',max((len(g['members'])for g in groups),default=0))
 if trial:
  pairmap={tuple(sorted((p['a'],p['b']))):p for p in pairs};mapping={};kept=[]
  # Prefer highest sampling density and compare to the FINAL representative.
  # Never use threshold-transitive closure that can accumulate visible error.
  for t in sorted(items,key=lambda t:(-t['w']*t['h'],t['id'])):
   match=next((j for j in kept if tuple(sorted((t['id'],j)))in pairmap),None)
   if match is None:kept.append(t['id'])
   else:mapping[t['id']]=match
  local={}
  for i,j in mapping.items():
   for chunk,tid in items[i]['references']:local.setdefault(chunk,{})[tid]=items[j]['blob']
  trialout=out/f'trial-{threshold:g}';trialout.mkdir(exist_ok=True);records=[]
  for r in meta['regions']:
   raw=(ROOT/r['path']).read_bytes();changes=local.get(r['chunk_id'],{});result,entry=deduplicate_room(raw,changes)
   # Repacking may move texture IDs, but all source draw and frame geometry,
   # material flags, clips, terrain and ordering must remain byte-exact.
   c,_,_,_=layout(raw);nc,_,_,_=layout(result);oldat=40+c[1]*16;newat=40+nc[1]*16
   for count,size,skip in zip(c[2:],(44,20,16,16),(2,4,0,0)):
    for n in range(count):
     if raw[oldat+n*size+skip:oldat+(n+1)*size]!=result[newat+n*size+skip:newat+(n+1)*size]:raise ValueError('Trial changed geometry/material/gameplay record')
    oldat+=count*size;newat+=count*size
   path=trialout/f'chunk_{r["chunk_id"]}.hk';path.write_bytes(result);entry.update(chunk_id=r['chunk_id'],source_sha256=r['sha256'],output_sha256=hashlib.sha256(result).hexdigest(),path=str(path));records.append(entry)
  scenes=[]
  for scene in sorted({r['scene_id']for r in meta['regions']}):
   src={t['id']for t in items if scene in t['rooms']};dst={mapping.get(i,i)for i in src};pages,_=dense_pack(rectangles(items,dst));before,_=dense_pack(rectangles(items,src));saved=lambda ids:sum(((items[i]['w']+3)//4*4)//2*items[i]['h']for i in ids if not items[i]['stream'])
   scenes.append(dict(scene_id=scene,removed_textures=len(src)-len(dst),pages_before=before,pages_after=pages,static_pixel_bytes_saved=saved(src)-saved(dst)))
  summary=dict(threshold=threshold,eligible_pairs=len(pairs),removed_unique_textures=len(mapping),replacements=[dict(texture=i,representative=j,similarity=pairmap[tuple(sorted((i,j)))]['similarity'],source_sha256=hashlib.sha256(items[i]['blob']).hexdigest(),representative_sha256=hashlib.sha256(items[j]['blob']).hexdigest())for i,j in mapping.items()],scenes=scenes,regions=records,geometry_materials_ordering_clips_and_collision_exact=True,scope=f'Ignored cooked-packs only; no disc or guest build. Explicit static art trial at{threshold:g}%; animations and black masks excluded. Every alias is compared directly with its retained representative.')
  (trialout/'report.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps({k:v for k,v in summary.items()if k not in ('regions','replacements')},indent=2))
 print('Ranking:',out/'ranked-matches.html');print('Pairs:',len(pairs))

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--threshold',type=float,default=95);p.add_argument('--trial',action='store_true');a=p.parse_args();build(a.threshold,a.trial)
