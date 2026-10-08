"""Hierarchy transforms and original scene records, independent of PS1 packing.

A room the original additively loads a second scene into is read as one scene
here. `SceneAdditiveLoadConditional.Start` picks that scene from a PlayerData
bool, and the port answers the bool once from `SetupNewPlayerData`, the same way
activation.py answers the load-time gates. Crossroads_10 is the only admitted
scene with such a loader today, and what it loads (`Crossroads_10_boss`) is not
a room: it has no tilemap, so tools/cook_scene_pack.py derives no envelope for
it, and its objects, the arena floor included, stand inside Crossroads_10's own
envelope. It is content merged into the room, not a scene of its own.
"""
import math
from functools import lru_cache
from pathlib import Path
from activation import Gates, fresh_save_bool, _fresh_save
# Both files number their serialized objects from 1, so a merged scene shifts
# the additive file's ids by this to get one id space. `Scene.sid` still reports
# an object under the file it was really serialized in, so a record points at
# something a source tool can open.
ADDITIVE_ID_BASE=100000

@lru_cache(None)
def _build_settings(source):
    """BuildSettings scene name to level file, with its order assertion."""
    settings=next(o for o in source.file('globalgamemanagers').objects.values() if o.type.name=='BuildSettings')
    files={p.rsplit('/',1)[-1][:-len('.unity')]:f'level{i}' for i,p in enumerate(source.read(settings)['scenes'])}
    if files.get('Town')!='level7':raise ValueError('BuildSettings scene order changed')
    return files

def retarget(tree,id_base,externals):
    """Move one additive object's references into the merged id space.

    A PPtr whose m_FileID is 0 names an object in its own file, so it takes the
    same shift that object's id takes. A PPtr into an external names the file by
    index into *that file's* externals list, and the merged list is a different
    order, so the index is remapped rather than carried over.
    """
    if isinstance(tree,dict):
        if len(tree)==2 and 'm_PathID' in tree and 'm_FileID' in tree:
            if tree['m_PathID']:
                if tree['m_FileID']:tree['m_FileID']=externals[tree['m_FileID']]
                else:tree['m_PathID']+=id_base
            return
        for value in tree.values():retarget(value,id_base,externals)
    elif isinstance(tree,list):
        for value in tree:retarget(value,id_base,externals)

class MergedFile:
    """A room's assets file with an additively loaded scene folded into it.

    `Source.ref` reads exactly two things from a file: `objects[path_id]` and
    `externals[file_id-1].path`. This provides both over the merged id space and
    a merged externals list; it is not a UnityPy SerializedFile and does not
    pretend to be one. Objects handed back are the real ones, so `Source.sid`
    and `assets_file` still name the file an object came from.
    """
    def __init__(self,base,objects,externals):
        self.base=base;self.name=base.name;self.objects=objects;self.externals=externals

class Scene:
    # A fixture built through Scene.__new__ carries no gates and hides nothing.
    gated_off=frozenset()
    # (id base, file name) per merged file, highest base first; empty on a fixture.
    _origins=()
    additive=()
    def __init__(self,source,name,merge_additive=True):
        self.source=source;base=source.file(name);self.file=base;self.objects={};self.errors=[]
        self._read(base,0,None)
        self._index()
        merged=self._merge(base) if merge_additive else []
        if merged:
            objects=dict(base.objects);externals=list(base.externals)
            for id_base,file in merged:
                objects.update({i+id_base:o for i,o in file.objects.items()})
                externals.extend(e for e in file.externals if e.path not in {x.path for x in externals})
            self.file=MergedFile(base,objects,externals)
            self._index()
        self._origins=tuple(sorted([(0,Path(base.name).name)]
                                   +[(id_base,Path(file.name).name) for id_base,file in merged],reverse=True))
        self.additive=tuple(Path(file.name).name for _,file in merged)
        self.gates=Gates(self);self.gated_off=self.gates.off
    def _read(self,file,id_base,externals):
        source=self.source
        for o in list(file.objects.values()):
            # Standalone Mesh payloads are resolved from their renderer/filter
            # references. Particle payloads are safe now that each scene runs in
            # a fresh isolated worker and must remain available to system passes.
            if o.type.name=='Mesh':continue
            try:
                typename=source.typename(o);tree=source.read(o)
                if id_base:retarget(tree,id_base,externals)
                self.objects[o.path_id+id_base]=(typename,tree)
            except Exception as ex:self.errors.append({'id':source.sid(o),'type':source.typename(o),'error':str(ex)})
    def _index(self):
        self.gos={i:d for i,(t,d) in self.objects.items() if t=='GameObject'}
        self.transforms={i:d for i,(t,d) in self.objects.items() if t=='Transform'}
        self.go_transform={d['m_GameObject']['m_PathID']:i for i,d in self.transforms.items()}
    def _authored_active(self,gid):
        """`active` without the load-time gates, which are not answered yet.

        `self.active` memoises, and its answer depends on `gated_off`, which
        `Gates` only fills at the end of construction. The merge runs before
        that, so it walks the authored hierarchy itself rather than seeding the
        cache with answers that a gate would later change.
        """
        while True:
            if gid not in self.gos or gid not in self.go_transform or not self.gos[gid]['m_IsActive']:return False
            father=self.transforms[self.go_transform[gid]]['m_Father']['m_PathID']
            if not father:return True
            if father not in self.transforms:return False
            gid=self.transforms[father]['m_GameObject']['m_PathID']
    def _merge(self,base):
        """Read every scene this room additively loads on a fresh save.

        `SceneAdditiveLoadConditional` tests one PlayerData bool and loads
        `sceneNameToLoad` when it already holds `playerDataBoolValue`, else
        `altSceneNameToLoad`. The extra int/bool tests and the PersistentBoolItem
        branch are refused rather than guessed, because a loader that used them
        would pick a different scene than this answers. Only the room's own
        loaders are read: an additive scene that carried one of its own would be
        a nested merge, and no source scene does that today.
        """
        source=self.source;merged=[]
        loaders=sorted(i for i,(kind,_) in self.objects.items() if kind=='SceneAdditiveLoadConditional')
        for sid in loaders:
            tree=self.objects[sid][1]
            if not tree['m_Enabled'] or not self._authored_active(tree['m_GameObject']['m_PathID']):continue
            if tree['needsPlayerDataInt'] or tree['extraBoolTests'] or tree['extraIntTests'] \
                    or tree['isIntValue'] or tree['usePersistentBoolItem'] or tree['doorTrigger']:
                raise ValueError(f'unsupported SceneAdditiveLoadConditional test set: {source.sid(base.objects[sid])}')
            playerdata=_fresh_save(str(source.directory/'Managed'/'Assembly-CSharp.dll'))
            value=fresh_save_bool(playerdata,tree['needsPlayerDataBool'])
            wanted=tree['sceneNameToLoad'] if value==bool(tree['playerDataBoolValue']) else tree['altSceneNameToLoad']
            if not wanted:continue
            file=source.file(_build_settings(source)[wanted])
            id_base=ADDITIVE_ID_BASE*(len(merged)+1)
            if max(base.objects)>=id_base or max(file.objects)>=ADDITIVE_ID_BASE:
                raise ValueError('additive merge id base overlaps a source file')
            paths=[e.path for e in base.externals]
            for other in merged:
                paths.extend(e.path for e in other[1].externals if e.path not in paths)
            externals={}
            for index,external in enumerate(file.externals,1):
                if external.path not in paths:paths.append(external.path)
                externals[index]=paths.index(external.path)+1
            self._read(file,id_base,externals)
            merged.append((id_base,file))
        return merged
    def sid(self,i):
        """`file:id` of one object, under the file it was serialized in."""
        for id_base,name in self._origins:
            if i>id_base:return f'{name}:{i-id_base}'
        return f'{Path(self.file.name).name}:{i}'
    @lru_cache(None)
    def world(self,tid):
        t=self.transforms[tid];q=t['m_LocalRotation'];x,y,z,w=[q[k] for k in 'xyzw'];sc=t['m_LocalScale'];p=t['m_LocalPosition']
        r=[[1-2*(y*y+z*z),2*(x*y-z*w),2*(x*z+y*w),p['x']], [2*(x*y+z*w),1-2*(x*x+z*z),2*(y*z-x*w),p['y']], [2*(x*z-y*w),2*(y*z+x*w),1-2*(x*x+y*y),p['z']], [0,0,0,1]]
        for row in range(3):
            for col,k in enumerate('xyz'):r[row][col]*=sc[k]
        father=t['m_Father']['m_PathID']
        if father:
            a=self.world(father);r=[[sum(a[i][k]*r[k][j] for k in range(4)) for j in range(4)] for i in range(4)]
        return r
    def point(self,gid,x=0,y=0,z=0):
        m=self.world(self.go_transform[gid]);return tuple(sum(m[i][j]*v for j,v in enumerate([x,y,z,1])) for i in range(3))
    @lru_cache(None)
    def active(self,gid):
        # An object whose GameObject could not be read (see self.errors) or that
        # lives in another file is never active in this scene.
        if gid not in self.gos or gid not in self.go_transform:return False
        if not self.gos[gid]['m_IsActive']:return False
        # A PlayerData gate the original evaluates on load and the port does
        # not run (see activation.py) leaves the object out of the world.
        if gid in self.gated_off:return False
        t=self.transforms[self.go_transform[gid]];f=t['m_Father']['m_PathID']
        if f and f not in self.transforms:return False
        return not f or self.active(self.transforms[f]['m_GameObject']['m_PathID'])
