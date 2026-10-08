#!/usr/bin/env python3
"""Verified admission-only migration: six-page banks to five-page/384-CLUT banks.

Exact source/cooker revisions are pinned below so later implementation changes
cannot be accepted as a metadata-only refresh. Image sampling and every pack
remain unchanged. A complete normal recook is always the fallback.
"""
import hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from quality import SCENERY_MAX_AXIS,STATIC_PAGE_BUDGET,TEXTURE_BUDGET
from regions import row_over_budget
from world import generate
OLD={'host/quality.py': '018b0a75eeb864851ff6485ed843a29bb4d4fc1ccf6fef4a923f03471d8ca8d2', 'host/cook.py': 'e0477dd615544cd532d66ad7cf6412d59a6d119623fec9038790682300162696', 'host/regions.py': 'c35ef94829c9fef9784eac39c4f93aaec9d17dc1085afb185c7ef219339e5f3c'}
NEW={'host/quality.py': '11b3278c2796bfffc69fbc16fd538122fce637ff6e470f68f01bdfd07f150e40', 'host/cook.py': '3a1992563cf400a691d7dbd844273aa2abaf7657b8b6e047438c12e0e452d351', 'host/regions.py': '8b664bd64723e7d431cc1bde86b0ac3bf9f1da69efffbe0bf92f3311fa3734e7'}
def sha(p):
    with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def write(p,data):
    temporary=p.with_suffix('.tmp');temporary.write_text(json.dumps(data,indent=2));temporary.replace(p)
def main():
    cache_path=ROOT/'.hkpsx/regions-cook-cache.json';cache=json.loads(cache_path.read_text())
    provenance=json.loads((ROOT/'.hkpsx/regions-provenance.json').read_text())
    current=json.loads((ROOT/'.hkpsx/doctor.json').read_text())['installs'][0]['data_directory']
    if provenance['source']!=current:raise ValueError('source installation changed')
    for name,record in provenance['inputs'].items():
        if sha(Path(current)/name)!=record['sha256']:raise ValueError('source input changed: '+name)
    for name,digest in cache['outputs'].items():
        if sha(ROOT/name)!=digest:raise ValueError('cooked output changed: '+name)
    for name,digest in cache['code'].items():
        actual=sha(ROOT/('host/requirements.lock'if name=='requirements'else name))
        if name in NEW:
            if actual!=NEW[name]or digest not in (OLD[name],NEW[name]):raise ValueError('unrecognized budget migration code')
        elif actual!=digest:raise ValueError('unrelated cooker changed: '+name)
    path=ROOT/'data/regions.json';report=json.loads(path.read_text())
    if not report['complete']or report['quality']['scenery_max_axis']!=SCENERY_MAX_AXIS:raise ValueError('different texture quality')
    for region in report['regions']:
        # regions.row_over_budget is the admission rule; restating it here gated
        # texture records on the 416-slot CLUT budget, which refuses chunk 159 of
        # Crossroads_03 (421 records, 321 palettes) that the cooker admits.
        if row_over_budget(region):
            raise ValueError('existing region exceeds new admission budget')
    report['quality'].update(static_page_budget=STATIC_PAGE_BUDGET,texture_budget=TEXTURE_BUDGET)
    generate(report);write(path,report)
    cache['code'].update(NEW);cache['outputs']['data/regions.json']=sha(path);write(cache_path,cache)
    print('Admission metadata updated; every cooked pack remains byte-identical.')
if __name__=='__main__':main()
