"""Local-only CIL inspection of selected installed types (no execution)."""
import argparse,json
from pathlib import Path
import dnfile
from dncil.cil.body.reader import read_method_body_from_bytes
from dncil.clr.token import Token
from source import ROOT

def inspect(assembly,types):
    pe=dnfile.dnPE(str(assembly)); tables=pe.net.mdtables
    def operand(v):
        if not isinstance(v,Token):return str(v)
        if v.table==0x70:return repr(pe.net.user_strings.get(v.rid).value)
        table=tables.tables.get(v.table)
        if table:
            row=table.rows[v.rid-1]
            return str(getattr(row,'Name',getattr(row,'TypeName',v)))
        return str(v)
    lines=[]
    for t in tables.TypeDef.rows:
        if str(t.TypeName) not in types:continue
        for m in t.MethodList:
            method=m.row
            if not method.Rva:continue
            body=read_method_body_from_bytes(pe.get_data(method.Rva,100000))
            lines.append(f'\n{t.TypeName}::{method.Name} RVA={method.Rva:x}')
            lines.extend(f'{i.offset:04x} {i.opcode.name:16} {operand(i.operand)}' for i in body.instructions)
    return '\n'.join(lines)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('types',nargs='+');p.add_argument('--assembly',default='Assembly-CSharp.dll');a=p.parse_args()
    d=Path(json.load(open(ROOT/'.hkpsx/doctor.json'))['installs'][0]['data_directory'])
    out=ROOT/'.hkpsx'/('il-'+a.assembly+'.txt');out.write_text(inspect(d/'Managed'/a.assembly,a.types));print(out)
