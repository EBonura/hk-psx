"""Read-only Unity source adapter. Retail structures are never written back."""
import hashlib,json,os
from pathlib import Path
import UnityPy
from UnityPy.helpers.TypeTreeGenerator import TypeTreeGenerator
from UnityPy.helpers import TypeTreeHelper
ROOT=Path(__file__).resolve().parents[1]


def rel(path):
    """A path as reports record it: relative to this checkout when it lies
    inside it, so a report never points into another clone (readers resolve it
    against their own ROOT)."""
    p=Path(path).resolve()
    return str(p.relative_to(ROOT)) if p.is_relative_to(ROOT) else str(path)


def source_path(directory, name):
    """Resolve one Unity external without discarding its serialized path."""
    directory = Path(directory).resolve()
    relative = Path(str(name).replace('\\', '/'))
    if relative.is_absolute() or '..' in relative.parts:
        raise ValueError(f'Unsafe Unity external path: {name}')
    candidates = [directory/relative]
    # Player builds serialize this editor pseudo-path even though the shipped
    # file lives in the data directory's Resources folder.
    if relative.parts and relative.parts[0] == 'Library' and relative.name.startswith('unity '):
        candidates.append(directory/'Resources'/relative.name)
    for candidate in candidates:
        resolved = candidate.resolve()
        if resolved.is_relative_to(directory) and resolved.is_file():
            return resolved
    raise FileNotFoundError(f'Unity external not found: {name}')


def repair_string_arrays(node):
    """Disambiguate generated string[] containers from scalar Unity strings.

    UnityPy dispatches primitive string readers before its array branch. The
    generated type tree can name a string[] container `string`, whereas its
    Array's data child is itself a string. A scalar string instead has char data.
    Return changed nodes so callers can restore shared generated trees afterward.
    """
    changed=[]
    def visit(current):
        children=current.m_Children
        if current.m_Type=='string' and len(children)==1 and children[0].m_Type=='Array':
            data=children[0].m_Children
            if len(data)==2 and data[1].m_Type=='string':
                current.m_Type='vector';changed.append(current)
        for child in children:visit(child)
    visit(node)
    return changed

TYPETREE_CACHE=ROOT/'.hkpsx/typetree-cache.json'


class Generator(TypeTreeGenerator):
    """The UnityPy generator with its answers kept on disk.

    The native library (libTypeTreeGeneratorAPI, a .NET NativeAOT build) crashed
    with SIGSEGV, a call through a null pointer in its own code, in about one
    cook worker in six when many started together under load: at start, inside
    `loadDLL`, or a moment later in a node query (crash reports of 2026-10-03,
    -08 and -09, all in the library's frames and none in Python's). A node list
    is a pure function of the Managed assemblies, so each is recorded by the
    assemblies' content hash the first time it is asked for and served from
    `.hkpsx/typetree-cache.json` afterwards. The library is neither loaded nor
    initialised until a type the file lacks is asked for, so a warm cache runs
    no native code at all.
    """
    def __init__(self,unity_version):
        # Deliberately not the base __init__: it initialises the native library.
        self.unity_version=unity_version;self.cache={};self.dlls=[];self.native=False
        self.recorded=None;self.fresh={}
    def load_local_dll_folder(self,dll_dir):
        self.dlls=sorted(p for p in Path(dll_dir).iterdir() if p.suffix=='.dll')
    def key(self):
        if self.recorded is None:
            digest=hashlib.sha256(self.unity_version.encode())
            for path in self.dlls:digest.update(path.name.encode());digest.update(hashlib.sha256(path.read_bytes()).digest())
            self.digest=digest.hexdigest()
            try:self.recorded=json.loads(TYPETREE_CACHE.read_text()).get(self.digest,{})
            except (OSError,ValueError):self.recorded={}
        return self.digest
    def start(self):
        if not self.native:
            super().__init__(self.unity_version)
            for path in self.dlls:self.load_dll(path.read_bytes())
            self.native=True
    def get_nodes(self,assembly,fullname):
        from TypeTreeGeneratorAPI import TypeTreeNode
        self.key();name=f'{assembly}|{fullname}'
        rows=self.recorded.get(name)
        if rows is None:
            self.start()
            rows=[[n.m_Type,n.m_Name,n.m_Level,n.m_MetaFlag] for n in super().get_nodes(assembly,fullname)]
            self.recorded[name]=rows;self.fresh[name]=rows;self.save()
        return [TypeTreeNode(m_Type=a,m_Name=b,m_Level=c,m_MetaFlag=d) for a,b,c,d in rows]
    def save(self):
        """Merge what this process learnt into the file; atomic, and safe beside other workers."""
        import fcntl
        TYPETREE_CACHE.parent.mkdir(parents=True,exist_ok=True)
        with open(str(TYPETREE_CACHE)+'.lock','w') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            try:data=json.loads(TYPETREE_CACHE.read_text())
            except (OSError,ValueError):data={}
            data.setdefault(self.digest,{}).update(self.fresh)
            temp=TYPETREE_CACHE.with_suffix(f'.{os.getpid()}.tmp')
            temp.write_text(json.dumps(data,sort_keys=True));temp.replace(TYPETREE_CACHE)
        self.fresh={}
    def __del__(self):
        if self.native:super().__del__()
    def get_nodes_up(self,*args):
        node=super().get_nodes_up(*args)
        # Generated MonoBehaviour headers must align the byte flag before PPtr.
        children=node.m_Children
        for child in children:
            if child.m_Name=='m_Enabled': child.m_MetaFlag |= 0x4000
        node.m_Children=children
        return node
class Source:
    def __init__(self, directory=None):
        self.directory=Path(directory or json.load(open(ROOT/'.hkpsx/doctor.json'))['installs'][0]['data_directory'])
        if not (self.directory.parent/'hollow_knight.exe').is_file():
            raise ValueError('Windows source with hollow_knight.exe required')
        self.env=UnityPy.Environment()
        self.env.path=str(self.directory)
        self.generator=Generator('6000.0.61f1')
        self.generator.load_local_dll_folder(str(self.directory/'Managed'))
        self.env.typetree_generator=self.generator
        self.files={}
        header=self.file('globalgamemanagers')
        if header.unity_version!='6000.0.61f1' or header.header.version!=22:
            raise ValueError(f'Unvalidated source serialization: {header.unity_version}/{header.header.version}')
    def file(self,name):
        path = source_path(self.directory, name)
        key = str(path.relative_to(self.directory))
        if key not in self.files:
            self.files[key]=self.env.load_file(str(path))
        return self.files[key]
    def ref(self,file,ref):
        if not ref['m_PathID']:raise ValueError('null source reference')
        if ref['m_FileID']:
            file=self.file(file.externals[ref['m_FileID']-1].path)
        return file.objects[ref['m_PathID']]
    def read(self,o):
        if o.type.name!='MonoBehaviour':return o.read_typetree()
        # UnityPy 1.25.3 C reader misreads generated aligned byte fields.
        # Pure reader gives the correct header and consumes exact object size.
        old=TypeTreeHelper.read_typetree_boost
        node=o._get_typetree_node();changed=repair_string_arrays(node)
        try:
            TypeTreeHelper.read_typetree_boost=None
            tree=o.read_typetree(nodes=node)
            head=o.parse_monobehaviour_head()
            if tree['m_Script']!={'m_FileID':head.m_Script.m_FileID,'m_PathID':head.m_Script.m_PathID}:
                raise ValueError('generated header differs from native header')
            return tree
        finally:
            for item in changed:item.m_Type="string"
            TypeTreeHelper.read_typetree_boost=old
    def typename(self,o):
        if o.type.name!='MonoBehaviour':return o.type.name
        return o.parse_monobehaviour_head().m_Script.read().m_ClassName
    def sid(self,o):return f'{Path(o.assets_file.name).name}:{o.path_id}'
def dump(path,data):
    Path(path).write_text(json.dumps(data,indent=2,default=lambda v:list(v)))
