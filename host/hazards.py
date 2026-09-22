"""Patch and prove R3000 load-delay hazards over exact linked code ranges.

The SDK's standalone tools heuristically guess code in PS-X EXE data, and
texture bytes can be valid-looking MIPS branches. The link map supplies exact
.text bounds, so both the patcher and the scan here see real code only.

The guest is built with every delay-slot filler search on (the SDK's
PSX_DELAY_SLOT_FLAGS), which fills slots with useful work instead of nops and
can leave a load whose consumer runs inside its delay. patch() reroutes each
such branch through the HAZARD_TRAMPOLINES array psx-rt links into .data;
scan() then proves the final code clean.
"""
import importlib.util,contextlib,io,re,hashlib,json,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]

def _symbols(map_path):
    symbols={}
    for line in Path(map_path).read_text().splitlines():
        for name in ['__text_start','__text_end','__bss_start','__bss_end','__data_start','__data_end']:
            if re.search(r'\b'+name+r'\s*=',line):symbols[name]=int(line.split()[0],16)
    lo,hi=symbols['__text_start'],symbols['__text_end']
    assert 0x80010000==lo<hi<symbols['__bss_end']<=0x801f8000
    return symbols

def _module(name,path):
    spec=importlib.util.spec_from_file_location(name,path);mod=importlib.util.module_from_spec(spec);spec.loader.exec_module(mod)
    return mod

def patch(exe,map_path):
    """Trampoline every hazardous branch in exe's .text, in place, with the
    pinned SDK's patcher (.psoxide/tools/hazard_patch.py). It covers every
    delay-slot consumer class, including a function's own `jr ra` whose slot
    loads the return value (cs-psx 31517d4) and an indirect `jalr` whose slot
    loads something the unknown callee may read first (core::fmt's
    Formatter::pad), which HK used to carry in a vendored copy.
    Returns the patcher's summary; raises when a site cannot be patched, the
    array is full or anything is left."""
    symbols=_symbols(map_path);lo,hi=symbols['__text_start'],symbols['__text_end']
    mod=_module('sdk_hazard_patch',ROOT/'.psoxide/tools/hazard_patch.py')
    full=mod.disassemble
    mod.disassemble=lambda path,base=mod.LOAD_ADDR:{a:v for a,v in full(path,base).items() if lo<=a<hi}
    text=Path(map_path).read_text()
    array=re.search(r'^([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s+HAZARD_TRAMPOLINES$',text,re.M)
    if not array:raise ValueError('HAZARD_TRAMPOLINES is not in the link map')
    data=Path(exe).read_bytes()
    first=data.find(mod.MAGIC.to_bytes(4,'little'),mod.HEADER)
    while first>=0 and first%4:first=data.find(mod.MAGIC.to_bytes(4,'little'),first+1)
    # The patcher takes the first magic word it meets; make sure that is the array.
    if mod.LOAD_ADDR+first-mod.HEADER!=int(array[1],16):
        raise ValueError(f'first trampoline magic is at {mod.LOAD_ADDR+first-mod.HEADER:#x}, not HAZARD_TRAMPOLINES {array[1]}')
    out=io.StringIO();argv=sys.argv
    try:
        sys.argv=['hazard_patch.py',str(exe)]
        with contextlib.redirect_stdout(out):status=mod.main()
    finally:
        sys.argv=argv
    log=out.getvalue();print(log,end='',flush=True)
    if status!=0:raise ValueError('Hazard patch failed:\n'+log)
    m=re.search(r'(\d+) patched, (\d+) remaining, (\d+)/(\d+) trampoline words used',log)
    summary={'sites':int(m[1]),'trampoline_words':int(m[3]),'capacity_words':int(m[4])} if m else \
        {'sites':0,'trampoline_words':0,'capacity_words':int.from_bytes(data[first+4:first+8],'little')}
    summary['returns']=log.count('patched jr ra at ');summary['calls']=log.count('patched jalr ')
    summary['register_jumps']=len(re.findall(r'^patched jr (?!ra )',log,re.M));summary['log']=log
    return summary

def scan(exe,map_path,report_path=None,patched=None):
    mod=_module('sdk_hazard',ROOT/'.psoxide/tools/hazard_scan.py')
    symbols=_symbols(map_path);lo,hi=symbols['__text_start'],symbols['__text_end']
    listing=mod.disassemble(str(exe));code={a:v for a,v in listing.items() if lo<=a<hi}
    mod.disassemble=lambda *_:code
    mod.looks_like_code=lambda *a,**k:True
    warnings=io.StringIO()
    with contextlib.redirect_stdout(warnings):hazards=mod.scan(str(exe))
    report={'exe_sha256':hashlib.file_digest(Path(exe).open('rb'),'sha256').hexdigest(),'code_start':lo,'code_end':hi,'symbols':symbols,'branch_hazards':hazards,'warnings':warnings.getvalue(),'post_link_patch':patched,'scope':'final EXE .text, exact link-map bounds, after the SDK hazard_patch.py; its trampolines are in .data and hazard-free by construction'}
    (Path(report_path) if report_path is not None else ROOT/'.hkpsx/hazards.json').write_text(json.dumps(report,indent=2))
    if hazards or warnings.getvalue():raise ValueError(f'Final code hazard scan failed: {hazards} {warnings.getvalue()}')
    print(f'0 hazards in final EXE code {lo:#x}..{hi:#x}',flush=True)
    return report
