"""Read-only Unity source adapter. Retail structures are never written back."""
import json
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

class Generator(TypeTreeGenerator):
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
