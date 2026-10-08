#!/usr/bin/env python3
"""Build the source-order-independent whole-world identity registry."""
import argparse
from collections import Counter
import gzip
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import sqlite3

from world_metadata import persistent_object

ROOT = Path(__file__).resolve().parents[1]
FORMAT = 'HKWORLDIDENTITY01'
SCHEMA_VERSION = 1
SOURCE_REF = re.compile(r'^([^:]+):(\d+)$')


def normalized_path(value):
    return PurePosixPath(value.replace('\\','/')).as_posix().casefold()


def stable_id(canonical_key):
    """Versioned 64-bit ID; collisions are fatal in the generated registry."""
    payload = f'hk-psx-id-v{SCHEMA_VERSION}\0{canonical_key}'.encode()
    return hashlib.blake2b(payload,digest_size=8,person=b'hkpsx-id').digest()


def scene_key(path):
    return 'scene|' + normalized_path(path)


def object_key(scene_path,path_id,component_type):
    return f'object|{normalized_path(scene_path)}|{int(path_id)}|{component_type}'


def gate_key(scene_path,path_id):
    return f'gate|{normalized_path(scene_path)}|{int(path_id)}'


def player_data_key(name):
    return 'playerdata|' + name


def asset_key(file_sha256,path_id):
    return f'asset|sha256:{file_sha256}|{int(path_id)}'


def builtin_asset_key(file_name,path_id):
    return f'asset|builtin:{normalized_path(file_name)}|{int(path_id)}'


def spawned_key(prefab_id,owner_id,slot):
    return 'spawn|' + prefab_id.hex() + '|' + owner_id.hex() + '|' + str(int(slot))


def source_ref(value):
    match = SOURCE_REF.fullmatch(value) if isinstance(value,str) else None
    return (match.group(1),int(match.group(2))) if match else None


def iter_source_refs(value):
    if isinstance(value,str):
        parsed=source_ref(value)
        if parsed: yield parsed
    elif isinstance(value,list):
        for item in value: yield from iter_source_refs(item)
    elif isinstance(value,dict):
        for item in value.values(): yield from iter_source_refs(item)


class Registry:
    def __init__(self,path):
        self.path=path; self.connection=sqlite3.connect(path)
        self.connection.execute('PRAGMA foreign_keys=ON')
        self.connection.executescript('''
          PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA temp_store=MEMORY;
          CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
          CREATE TABLE entity(
            stable_id BLOB PRIMARY KEY, canonical_key TEXT NOT NULL UNIQUE,
            kind TEXT NOT NULL, source TEXT NOT NULL, owner TEXT NOT NULL,
            state_scope TEXT NOT NULL, source_version_scoped INTEGER NOT NULL);
          CREATE INDEX entity_kind ON entity(kind);
          CREATE TABLE legacy_alias(
            kind TEXT NOT NULL, legacy_id INTEGER NOT NULL, stable_id BLOB NOT NULL,
            source TEXT NOT NULL, PRIMARY KEY(kind,legacy_id), UNIQUE(kind,stable_id),
            FOREIGN KEY(stable_id) REFERENCES entity(stable_id));
        ''')
        self.counts=Counter(); self.scopes=Counter(); self.aliases=Counter();

    def add(self,kind,canonical,source,owner,state_scope='none',version_scoped=False):
        ident=stable_id(canonical)
        try:
            self.connection.execute('INSERT INTO entity VALUES(?,?,?,?,?,?,?)',
                (ident,canonical,kind,source,owner,state_scope,int(version_scoped)))
        except sqlite3.IntegrityError as error:
            prior=self.connection.execute(
                'SELECT canonical_key,source FROM entity WHERE stable_id=?',(ident,)).fetchone()
            if prior and prior[0]!=canonical:
                raise ValueError(f'stable ID collision: {canonical} / {prior[0]}') from error
            # Repeated external references are the same canonical asset owner.
            if prior and prior[0]==canonical:
                return ident
            raise
        self.counts[kind]+=1
        self.scopes[state_scope]+=1
        return ident

    def add_alias(self,kind,legacy_id,ident,source):
        try:self.connection.execute('INSERT INTO legacy_alias VALUES(?,?,?,?)',
                                    (kind,legacy_id,ident,source))
        except sqlite3.IntegrityError:
            prior=self.connection.execute('SELECT stable_id,source FROM legacy_alias '
                'WHERE kind=? AND legacy_id=?',(kind,legacy_id)).fetchone()
            if prior!=(ident,source):raise ValueError(f'legacy {kind} ID {legacy_id} aliases multiple owners')
            return
        self.aliases[kind]+=1

    def finish(self,metadata):
        digest=hashlib.sha256()
        for kind,canonical,ident in self.connection.execute(
                'SELECT kind,canonical_key,stable_id FROM entity ORDER BY canonical_key,kind'):
            digest.update(kind.encode()+b'\0'+canonical.encode()+b'\0'+ident)
        alias_digest=hashlib.sha256()
        for kind,legacy_id,ident in self.connection.execute(
                'SELECT kind,legacy_id,stable_id FROM legacy_alias ORDER BY kind,legacy_id'):
            alias_digest.update(kind.encode()+b'\0'+str(legacy_id).encode()+b'\0'+ident)
        metadata={**metadata,'schema_version':SCHEMA_VERSION,
                  'registry_digest':digest.hexdigest(),
                  'legacy_alias_digest':alias_digest.hexdigest(),
                  'entity_counts':dict(sorted(self.counts.items())),
                  'state_scope_counts':dict(sorted(self.scopes.items())),
                  'legacy_alias_counts':dict(sorted(self.aliases.items()))}
        self.connection.executemany('INSERT INTO metadata VALUES(?,?)',
            [(key,json.dumps(value,sort_keys=True)) for key,value in metadata.items()])
        self.connection.commit(); self.connection.execute('VACUUM'); self.connection.close()
        return metadata


def component_scope(component_type,tree):
    if component_type in {'GameManager','HeroController','AudioManager','UIManager',
                          'InputHandler','CameraController'}:
        return 'global_runtime_non_save'
    if component_type=='GrassCut': return 'active_scene'
    # `dontSave` and `semiPersistent` are fields of PersistentBoolItem, not of
    # Breakable: a Breakable typetree carries neither, so asking it for them
    # returned None twice and scoped every breakable in the world
    # `global_persistent`. world_metadata.persistent_object is the authority and
    # says the opposite for a Breakable with no PersistentBoolItem beside it,
    # because `any()` over an empty persistence list is False. Each
    # PersistentBoolItem gets its own registry row, so the durable state is
    # scoped there and the Breakable itself resets with the scene.
    if component_type=='PersistentBoolItem':
        record={'persistence':[{'dont_save':bool(tree.get('dontSave')),
                                'semi_persistent':bool(tree.get('semiPersistent'))}]}
        if persistent_object(record): return 'global_persistent'
        return 'active_scene' if tree.get('dontSave') else 'mode_session'
    # Unity scene components are reconstructed from source on scene admission.
    # Durable values they read/write are separate PlayerData registry entries.
    return 'active_scene'


def build(report,world_catalog,room_inventory,component_root,database_path,regions=None):
    if report['fingerprint']!=world_catalog['source_world_fingerprint']:
        raise ValueError('World catalog belongs to another import')
    if database_path.exists(): database_path.unlink()
    registry=Registry(database_path)
    scene_paths={scene['file']:scene['path'] for scene in report['scenes']}
    type_owners={row['type']:'/'.join(row['owner_tasks'])
                 for row in world_catalog['component_type_catalog']}
    actor_types={row['type'] for row in world_catalog['component_type_catalog']
                 if any(category in ('actors','bosses') for category in row['categories'])}
    asset_refs=set(); actor_objects=0
    for scene in sorted(report['scenes'],key=lambda row:normalized_path(row['path'])):
        path=scene['path']; file=scene['file']
        registry.add('scene',scene_key(path),path,'P05-P09 scene runtime')
        with gzip.open(component_root/file/'components.json.gz','rt') as stream:
            document=json.load(stream)
        for obj in sorted(document['objects'],key=lambda row:(
                int(row['source'].rsplit(':',1)[1]),row['type'])):
            path_id=int(obj['source'].rsplit(':',1)[1]); typ=obj['type']
            registry.add('component',object_key(path,path_id,typ),obj['source'],
                type_owners.get(typ,'P12-P26 source-system implementation triage'),
                component_scope(typ,obj['data']))
            if typ in actor_types: actor_objects+=1
        with gzip.open(component_root/file/'geometry.json.gz','rt') as stream:
            geometry=json.load(stream)
        for ref in iter_source_refs(geometry):
            if ref[0]!=file: asset_refs.add(ref)
    for gate in sorted(world_catalog['transition_points'],key=lambda row:(
            normalized_path(scene_paths[row['source_file']]),row['source'])):
        pid=int(gate['source'].rsplit(':',1)[1])
        registry.add('gate',gate_key(scene_paths[gate['source_file']],pid),
            gate['source'],'P09 scene-transition runtime')
    for row in sorted(world_catalog['player_data']['keys'],key=lambda row:row['key']):
        registry.add('player_data',player_data_key(row['key']),row['key'],
            'P13-P18 persistent adventure','global_persistent')
    hashes=room_inventory['source_hashes']; unresolved_assets=[]
    for file,pid in sorted(asset_refs):
        source_hash=hashes.get(file,{}).get('sha256')
        if not source_hash and normalized_path(file) in {
                'unity default resources','library/unity default resources',
                'resources/unity default resources'}:
            registry.add('asset',builtin_asset_key(file,pid),f'{file}:{pid}',
                'P06-P08 built-in asset cooking',version_scoped=False)
            continue
        if not source_hash:
            unresolved_assets.append(f'{file}:{pid}'); continue
        registry.add('asset',asset_key(source_hash,pid),f'{file}:{pid}',
            'P06-P08/P25 asset cooking and residency',version_scoped=True)
    if regions:
        for region in regions['regions']:
            scene_path=scene_paths[region['scene_file']]; scene_id=region['scene_id']
            for row in region.get('breakables',[]):
                pid=int(row['source'].rsplit(':',1)[1]); ident=stable_id(object_key(scene_path,pid,'Breakable'))
                registry.add_alias('breakable_scene_x128',scene_id*128+row['state_index'],ident,row['source'])
            for row in region.get('grass',[]):
                pid=int(row['source'].rsplit(':',1)[1]); ident=stable_id(object_key(scene_path,pid,'GrassCut'))
                registry.add_alias('grass_scene_x1024',scene_id*1024+row['state_index'],ident,row['source'])
    metadata=registry.finish({'format':FORMAT,
        'source_world_fingerprint':report['fingerprint'],
        'source_inventory_fingerprint':room_inventory['fingerprint'],
        'identity_contract':{
            'scene':'normalized source scene path',
            'component':'scene path + Unity PathID + component type',
            'gate':'scene path + TransitionPoint PathID',
            'player_data':'exact case-sensitive PlayerData field name',
            'asset':'source-file SHA256 + PathID; source-version scoped',
            'builtin_asset':'normalized Unity built-in pseudo-file + PathID',
            'spawned_instance':'prefab stable ID + authored owner stable ID + stable slot'},
        'actor_component_count':actor_objects,
        'unresolved_asset_references':unresolved_assets,
        'migration_policy':{
            'same_canonical_key':'ID is unchanged regardless of input/build ordering',
            'changed_scene_path_or_path_id':'requires an explicit old-ID to new-ID alias',
            'changed_external_asset_file_hash':'recook; assets are not save-state keys',
            'unknown_schema_version':'reject rather than silently reinterpret state'},
        'reset_contract':{
            'scene_leave/bench/death/quit_load/dream_return':'clear active-scene owner only',
            'challenge_reset':'clear mode-session and active-scene owners',
            'mode_reset/new_game':'clear global, mode-session and active-scene owners',
            'global_reload':'replace the global snapshot and clear active-scene owners',
            'breakable':'active scene; a Breakable with no PersistentBoolItem beside it resets, '
                        'per world_metadata.persistent_object',
            'persistent_bool_item':'dontSave -> active scene; semiPersistent -> mode session; '
                                   'otherwise global persistent',
            'grass':'active scene',
            'ordinary_actor':'active scene; persistent defeat flags remain separate PlayerData owners'}})
    metadata['database_bytes']=database_path.stat().st_size
    return metadata


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,default=ROOT/'.hkpsx/world-import/report.json')
    parser.add_argument('--world-catalog',type=Path,
                        default=ROOT/'.hkpsx/world-import/world-catalog.json')
    parser.add_argument('--room-inventory',type=Path,default=ROOT/'.hkpsx/room-inventory.json')
    parser.add_argument('--components',type=Path,default=ROOT/'.hkpsx/world-import')
    parser.add_argument('--database',type=Path,
                        default=ROOT/'.hkpsx/world-import/world-identities.sqlite')
    parser.add_argument('--regions',type=Path,default=ROOT/'data/regions.json')
    parser.add_argument('--output',type=Path,
                        default=ROOT/'.hkpsx/world-import/world-identities.json')
    args=parser.parse_args(); output=args.output.resolve(); database=args.database.resolve()
    root=(ROOT/'.hkpsx').resolve()
    if not output.is_relative_to(root) or not database.is_relative_to(root):
        parser.error('Outputs must be inside .hkpsx')
    result=build(json.loads(args.report.read_text()),
        json.loads(args.world_catalog.read_text()),json.loads(args.room_inventory.read_text()),
        args.components,database,json.loads(args.regions.read_text()) if args.regions.is_file() else None)
    output.write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':
    main()
