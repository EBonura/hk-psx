"""Hierarchy transforms and original scene records, independent of PS1 packing."""
import math
from functools import lru_cache
from activation import Gates
class Scene:
    # A fixture built through Scene.__new__ carries no gates and hides nothing.
    gated_off=frozenset()
    def __init__(self,source,name):
        self.source=source;self.file=source.file(name);self.objects={};self.errors=[]
        for o in list(self.file.objects.values()):
            # Standalone Mesh payloads are resolved from their renderer/filter
            # references. Particle payloads are safe now that each scene runs in
            # a fresh isolated worker and must remain available to system passes.
            if o.type.name == 'Mesh':continue
            try:
                typename=source.typename(o)
                self.objects[o.path_id]=(typename,source.read(o))
            except Exception as ex:self.errors.append({'id':source.sid(o),'type':source.typename(o),'error':str(ex)})
        self.gos={i:d for i,(t,d) in self.objects.items() if t=='GameObject'}
        self.transforms={i:d for i,(t,d) in self.objects.items() if t=='Transform'}
        self.go_transform={d['m_GameObject']['m_PathID']:i for i,d in self.transforms.items()}
        self.gates=Gates(self);self.gated_off=self.gates.off
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
