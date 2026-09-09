"""Read-only Unity source adapter. Retail structures are never written back."""
import json
from pathlib import Path
import UnityPy
from UnityPy.helpers.TypeTreeGenerator import TypeTreeGenerator
from UnityPy.helpers import TypeTreeHelper
ROOT=Path(__file__).resolve().parents[1]
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
        if name not in self.files:
            self.files[name]=self.env.load_file(str(self.directory/name))
        return self.files[name]
    def ref(self,file,ref):
        if not ref['m_PathID']:raise ValueError('null source reference')
        if ref['m_FileID']:
            file=self.file(Path(file.externals[ref['m_FileID']-1].path).name)
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
