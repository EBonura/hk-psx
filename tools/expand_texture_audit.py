#!/usr/bin/env python3
import hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from audit_texture_similarity import inventory
from texture_match_groups import all_static_matches,review_groups
raw=(ROOT/'data/regions.json').read_bytes();out=ROOT/'.hkpsx/texture-similarity';items,_=inventory(json.loads(raw))
r=all_static_matches(items,80);r['metadata_sha256']=hashlib.sha256(raw).hexdigest();r['code_sha256']=hashlib.sha256((ROOT/'tools/texture_match_groups.py').read_bytes()).hexdigest()
r['groups']=review_groups(r['candidates'],items,80)
(out/'all-static-matches.json').write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({k:v for k,v in r.items()if k not in ('candidates','groups')},indent=2),flush=True)
print('Pairs',len(r['candidates']),'textures',len({p[k]for p in r['candidates']for k in ('a','b')}),'groups',len(r['groups']),flush=True)
print('Largest groups',[(g['representative'],len(g['members']),g['minimum_similarity'])for g in r['groups'][:8]],flush=True)
