"""Patch and prove R3000 load-delay hazards over exact linked code ranges.

The SDK's standalone tools heuristically guess code in PS-X EXE data, and
texture bytes can be valid-looking MIPS branches. The link map supplies exact
.text bounds, so both the patcher and the scan here see real code only. The
tools are the pinned SDK's Rust `tools/psoxide-hazard` (`hazard-patch`,
`hazard-scan`, `stack-guard`), built from the hydrated `.psoxide` once per run.

The guest is built with every delay-slot filler search on (the SDK's
PSX_DELAY_SLOT_FLAGS), which fills slots with useful work instead of nops and
can leave a load whose consumer runs inside its delay. patch() reroutes each
such branch through the HAZARD_TRAMPOLINES array psx-rt links into .data;
scan() then proves the final code clean.
"""
import os,re,hashlib,json,subprocess
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

MAGIC=0x48415A54  # psoxide-hazard's HAZARD_TRAMPOLINES magic
HEADER=0x800
LOAD_ADDR=0x80010000
_TOOLS=None

def _tools():
    """The directory holding the SDK's built post-link tools."""
    global _TOOLS
    if _TOOLS is None:
        sdk=ROOT/'.psoxide'
        subprocess.run(['cargo','build','--release','--locked','--manifest-path',str(sdk/'Cargo.toml'),'-p','psoxide-hazard'],check=True)
        _TOOLS=Path(os.environ.get('CARGO_TARGET_DIR') or sdk/'target')/'release'
    return _TOOLS

def _run(tool,exe,map_path,extra=()):
    command=[str(_tools()/tool),str(exe),'--map',str(map_path)]
    for lo,hi in extra:command+=['--code',f'{lo:x}..{hi:x}']
    done=subprocess.run(command,capture_output=True,text=True)
    return done.returncode,done.stdout+done.stderr

def patch(exe,map_path,extra=(),array_at=None):
    """Trampoline every hazardous branch in exe's .text, in place, with the
    pinned SDK's patcher (tools/psoxide-hazard `hazard-patch`). It covers every
    delay-slot consumer class, including a function's own `jr ra` whose slot
    loads the return value (cs-psx 31517d4) and an indirect `jalr` whose slot
    loads something the unknown callee may read first (core::fmt's
    Formatter::pad), which HK used to carry in a vendored copy. It also pads
    the flat file with zeros to the sector multiple its header claims.
    Returns the patcher's summary; raises when a site cannot be patched, the
    array is full or anything is left. `extra` adds (lo, hi) code ranges: a
    code module's addresses, when exe is a composite image holding that
    module at its link address with its own trampoline array at `array_at`
    (host/code_modules.py)."""
    symbols=_symbols(map_path)
    text=Path(map_path).read_text()
    array=re.search(r'^([0-9a-f]+)\s+[0-9a-f]+\s+([0-9a-f]+)\s+\d+\s+HAZARD_TRAMPOLINES$',text,re.M)
    if not array:raise ValueError('HAZARD_TRAMPOLINES is not in the link map')
    data=Path(exe).read_bytes()
    first=data.find(MAGIC.to_bytes(4,'little'),HEADER)
    while first>=0 and first%4:first=data.find(MAGIC.to_bytes(4,'little'),first+1)
    # The patcher takes the first magic word it meets; make sure that is the array.
    # A code module's composite image carries its own array (array_at).
    expected=int(array[1],16) if array_at is None else array_at
    if LOAD_ADDR+first-HEADER!=expected:
        raise ValueError(f'first trampoline magic is at {LOAD_ADDR+first-HEADER:#x}, not {expected:#x}')
    status,log=_run('hazard-patch',exe,map_path,extra)
    print(log,end='',flush=True)
    if status!=0:raise ValueError('Hazard patch failed:\n'+log)
    m=re.search(r'(\d+) patched, (\d+) remaining, (\d+)/(\d+) trampoline words used',log)
    summary={'sites':int(m[1]),'trampoline_words':int(m[3]),'capacity_words':int(m[4])} if m else \
        {'sites':0,'trampoline_words':0,'capacity_words':int.from_bytes(data[first+4:first+8],'little')}
    # One `hazard ADDR: OP ARGS | slot ...` line per consumer; a patched branch names its address.
    ops={}
    for site in re.finditer(r'^hazard ([0-9a-f]+): (\S+) ?([^|]*)\|',log,re.M):ops[site[1]]=(site[2],site[3].strip())
    patched=[ops.get(site[1],('',''))for site in re.finditer(r'^patched ([0-9a-f]+) -> trampoline',log,re.M)]
    summary['returns']=sum(op=='jr' and args=='ra' for op,args in patched);summary['calls']=sum(op=='jalr' for op,_ in patched)
    summary['register_jumps']=sum(op=='jr' and args!='ra' for op,args in patched);summary['log']=log
    return summary

# A GTE command in a branch delay slot runs twice when an interrupt is taken on
# it (silicon v1.24). The scanner reports the sites as a warning, not a hazard
# of the load-delay kind this gate proves absent; they are kept in the report.
_GTE_DELAY_SLOT=re.compile(r'^warning: \d+ GTE commands in branch delay slots')

def scan(exe,map_path,report_path=None,patched=None,extra=()):
    symbols=_symbols(map_path);lo,hi=symbols['__text_start'],symbols['__text_end']
    status,log=_run('hazard-scan',exe,map_path,extra)
    lines=log.splitlines()
    warnings=[l for l in lines if l.startswith('warning:')]
    fatal=[l for l in warnings if not _GTE_DELAY_SLOT.match(l)]
    hazards=[l for l in lines if l.startswith('hazard ')]
    report={'exe_sha256':hashlib.file_digest(Path(exe).open('rb'),'sha256').hexdigest(),'code_start':lo,'code_end':hi,'extra_code':[list(r) for r in extra],'symbols':symbols,'branch_hazards':hazards,'warnings':'\n'.join(warnings)+('\n' if warnings else ''),'post_link_patch':patched,'scope':'final EXE .text, exact link-map bounds, after hazard-patch; its trampolines are in .data and hazard-free by construction'}
    (Path(report_path) if report_path is not None else ROOT/'.hkpsx/hazards.json').write_text(json.dumps(report,indent=2))
    if status!=0 or hazards or fatal:raise ValueError(f'Final code hazard scan failed (exit {status}):\n{log}')
    for l in warnings:print(l,flush=True)
    print(f'0 hazards in final EXE code {lo:#x}..{hi:#x}'+''.join(f' and {l:#x}..{h:#x}' for l,h in extra),flush=True)
    return report

def stack_guard(exe,map_path):
    """Prove every psx-rt scratchpad stack call tree in the final EXE fits its
    region, with the pinned SDK's `stack-guard`. The guard bounds each switch
    table to its own function by the link map and follows hazard trampolines to
    their targets; an unresolved register jump, recursion, a call through a
    pointer or a BIOS call still fails the build. Returns the guard's report
    lines; raises on any failure.

    The Python walker this replaced also refused a tree that reaches psx-rt's
    I-cache flush, which runs with the scratchpad unmapped. The Rust guard has no
    such check."""
    status,log=_run('stack-guard',exe,map_path)
    print(log,end='',flush=True)
    if status!=0:raise ValueError('Scratchpad stack guard failed:\n'+log)
    return log.splitlines()
