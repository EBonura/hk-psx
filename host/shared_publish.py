"""Canonical shared publication plumbing; relocation remains in the SDK/Rust tools."""
import hashlib, json, os, subprocess
from pathlib import Path
import code_modules as cm

SDK_TOOL_REVISION = '00847214ebe37f6837c1c6e74bacca8584976ac2'

def tool_source():
    value = os.environ.get('HK_OVERLAY_SDK')
    if not value:
        raise ValueError('HK_OVERLAY_SDK must name the complete SDK host-tool checkout at '+SDK_TOOL_REVISION)
    source = Path(value).resolve()
    revision = subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()
    if revision != SDK_TOOL_REVISION:
        raise ValueError('shared overlay host-tool SDK revision mismatch')
    if subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],cwd=source,text=True).strip():
        raise ValueError('shared overlay host-tool SDK must have clean tracked sources')
    return source

def publish(root, elf, link_map, out, scene_code, art):
    sdk = tool_source()
    target = Path(os.environ.get('CARGO_TARGET_DIR', str(out/'host-target'))).resolve()
    sdk_target = target/'overlay-sdk-tools'
    env = dict(os.environ, CARGO_TARGET_DIR=str(sdk_target))
    subprocess.run(['cargo','build','--locked','--release','--manifest-path',str(sdk/'tools/psoxide-hazard/Cargo.toml'),
                    '--bin','overlay-compose','--bin','hazard-patch','--bin','hazard-scan','--bin','stack-guard'],check=True,env=env)
    subprocess.run(['cargo','build','--locked','--release','--manifest-path',str(root/'host/hk-build/Cargo.toml'),
                    '--bin','hk-module-publish'],check=True,env=dict(os.environ,CARGO_TARGET_DIR=str(target)))
    publisher = target/'release/hk-module-publish'
    declarations = out/'module-declarations.json'
    declarations.write_text(json.dumps({'modules':[{'name':name,'id':i,'linked':cm.link_address(i)} for i,name in enumerate(cm.MODULES)]}))
    # Every invocation owns a fresh publication, preserving earlier guard evidence.
    import tempfile
    publication = Path(tempfile.mkdtemp(prefix='shared-publication-',dir=out))/'images'
    def run(*args): subprocess.run([str(publisher),*map(str,args)],check=True)
    # A stored form from an earlier build binds that build's raw bytes (module ids move when a module is added): drop it, it is rewritten below.
    for stale in (out/'chunks').glob('*.stored'): stale.unlink()
    for stale in (out/'chunks').glob('*.binding.json'): stale.unlink()
    run('pipeline',elf,link_map,declarations,publication,sdk_target/'release')
    inputs=out/'transport-inputs'; inputs.mkdir(exist_ok=True)
    blobs={**art,**cm.carried_blobs(root)}
    for name,blob in blobs.items(): (inputs/(name+'.blob')).write_bytes(blob)
    chunks=[{'kind':kind,'scene_index':scene,'modules':mods,'stem':f'{kind}_{scene}' if scene is not None else f'{kind}_{cm.all_names()[mods[0]]}'} for kind,scene,mods in cm.package_chunks(scene_code)]
    config=out/'transport-config.json'
    config.write_text(json.dumps({'names':cm.all_names(),'code_count':len(cm.MODULES),'data':{name:str(inputs/(name+'.blob')) for name in blobs},'chunks':chunks,'rooms':list(scene_code.values()),'room_scenes':list(scene_code.keys()),'carried':[cm.all_names().index(n) for n in cm.CARRIED if n not in cm.OPTIONAL],'optional':[cm.all_names().index(n) for n in cm.OPTIONAL],'pool':cm.POOL_BYTES,'pool_base':cm.pool_base()}))
    run('assemble',elf,config,publication/'final',out)
    import lz4.block
    for chunk in chunks:
        if chunk['kind'] in ('code','boot'): continue
        raw=out/'chunks'/(chunk['stem']+'.raw'); payload=raw.read_bytes()
        stored=cm.art_stored(payload)
        if stored is not None:
            if lz4.block.decompress(stored[8:],uncompressed_size=len(payload))!=payload:
                raise ValueError('stored art differs from exact raw package')
            raw.with_suffix('.stored').write_bytes(stored)
            raw.with_suffix('.binding.json').write_text(json.dumps({'raw_sha256':hashlib.sha256(payload).hexdigest(),'stored_sha256':hashlib.sha256(stored).hexdigest()}))
        elif raw.with_suffix('.stored').exists():
            raise ValueError('stale stored art in build directory')
    run('assemble',elf,config,publication/'final',out)
    for required in ('runtime.exe','modules.json','transport.json','chunks'):
        if not (out/required).exists():
            raise ValueError('shared publisher missing required output: '+required)
    (out/'resident.map').write_text(cm.map_with_only(link_map.read_text()))
