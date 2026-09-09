"""Apply pinned SDK hazard analysis to exact linked executable code ranges.

The SDK's standalone scanner heuristically guesses code in PS-X EXE data.
Texture bytes can be valid-looking MIPS branches. The link map supplies exact
.text bounds, so this wrapper scans final EXE bytes without modifying them.
"""
import importlib.util,contextlib,io,re,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

def scan(exe,map_path,report_path=None):
    spec=importlib.util.spec_from_file_location('sdk_hazard',ROOT/'.psoxide/tools/hazard_scan.py');mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
    symbols={}
    for line in Path(map_path).read_text().splitlines():
        for name in ['__text_start','__text_end','__bss_start','__bss_end','__data_start','__data_end']:
            if re.search(r'\b'+name+r'\s*=',line):symbols[name]=int(line.split()[0],16)
    lo,hi=symbols['__text_start'],symbols['__text_end']
    assert 0x80010000==lo<hi<symbols['__bss_end']<=0x801f8000
    listing=mod.disassemble(str(exe));code={a:v for a,v in listing.items() if lo<=a<hi}
    mod.disassemble=lambda _:code
    mod.looks_like_code=lambda *a,**k:True
    warnings=io.StringIO()
    with contextlib.redirect_stdout(warnings):hazards=mod.scan(str(exe))
    report={'exe_sha256':hashlib.file_digest(Path(exe).open('rb'),'sha256').hexdigest(),'code_start':lo,'code_end':hi,'symbols':symbols,'branch_hazards':hazards,'warnings':warnings.getvalue(),'scope':'final EXE .text, exact link-map bounds; data is not executable; no post-link patching'}
    (Path(report_path) if report_path is not None else ROOT/'.hkpsx/hazards.json').write_text(json.dumps(report,indent=2))
    if hazards or warnings.getvalue():raise ValueError(f'Final code hazard scan failed: {hazards} {warnings.getvalue()}')
    print(f'0 hazards in final EXE code {lo:#x}..{hi:#x}',flush=True)
    return report
