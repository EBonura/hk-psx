"""Build a local, searchable coverage page from a hash-bound world import report."""
import argparse
from collections import Counter
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def render(report):
    records = []
    errors = Counter()
    for row in report['scenes']:
        for item in row.get('errors', []) + row.get('unsupported', []):
            errors[(item.get('type', 'unknown'), item.get('error', item.get('reason', 'unknown')))] += 1
        records.append({key:row.get(key) for key in
                        ('index', 'file', 'scene_name', 'status', 'counts', 'component_types', 'errors', 'unsupported', 'failure')})
    payload = json.dumps({'coverage':report['coverage'], 'scenes':records,
        'errors':[{'type':kind,'reason':reason,'instances':count} for (kind,reason),count in errors.most_common()],
        'inputs_unchanged':report.get('inputs_unchanged', False)}, separators=(',', ':')).replace('<', '\\u003c')
    return '''<!doctype html><meta charset="utf-8"><title>Hollow Knight world import coverage</title>
<style>
body{font:15px system-ui;background:#111720;color:#dfe7ef;margin:32px auto;max-width:1400px;padding:0 24px}
h1{font-size:30px}p{line-height:1.6;max-width:1000px;color:#b8c8da}input,select{font:inherit;padding:10px;background:#223044;color:white;border:1px solid #405878;border-radius:6px;margin:4px}table{border-collapse:collapse;width:100%;font-size:13px}td,th{padding:9px;border-bottom:1px solid #314155;text-align:left;vertical-align:top}th{position:sticky;top:0;background:#192433}summary{cursor:pointer}pre{white-space:pre-wrap;max-width:700px}#stats{display:flex;flex-wrap:wrap;gap:16px;margin:20px 0}.stat{padding:16px;background:#1c2a3b;border-radius:8px}.stat strong{display:block;font-size:26px}.partial{color:#edc671}.failed{color:#ff9b98}.imported{color:#91d5b2}button{font:inherit;margin:4px;padding:10px;cursor:pointer}
</style>
<h1>Hollow Knight · whole-world source import</h1>
<p>Every source scene is included, including menus, cinematics and alternate versions. These are host geometry and component records. “Imported” means this extractor reported no unresolved geometry; it does not mean a scene is packed for PS1 or playable. Unsupported components are retained for shared system development.</p>
<div id="stats"></div><p id="integrity"></p>
<button id="scenesBtn">Scenes</button><button id="systemsBtn">Component inventory</button><button id="errorsBtn">Extraction gaps</button><br>
<input id="search" placeholder="Search scene, component or error" size="46"><select id="status"><option value="">All states</option><option>imported</option><option>partial</option><option>failed</option></select><p id="count"></p><table><thead id="head"></thead><tbody id="body"></tbody></table>
<script type="application/json" id="data">''' + payload + '''</script><script>
const data=JSON.parse(document.getElementById('data').textContent);let mode='scenes';const $=id=>document.getElementById(id);
for(const [label,key] of [['Source scenes','catalog_scenes'],['Processed','processed_scenes'],['Geometry unresolved','partial_scenes'],['Failed imports','failed_scenes']]){let e=document.createElement('div');e.className='stat';let n=document.createElement('strong');n.textContent=data.coverage[key];e.append(n,document.createTextNode(label));$('stats').append(e)}
$('integrity').textContent=data.inputs_unchanged?'Source hashes verified before and after import. PS1 packing and gameplay validation: not run.':'Source hash verification after the run is pending. PS1 packing and gameplay validation: not run.';
function cell(row,text){const e=document.createElement('td');e.textContent=text??'';row.append(e);return e}
function draw(){const query=$('search').value.toLowerCase(),state=$('status').value;const titles=mode==='scenes'?['ID','Scene','Import','Sprites / terrain edges','Meshes / exits','Details']:mode==='systems'?['Component type','Instances','Scenes','Gameplay support']:['Type','Instances','Reason'];$('head').replaceChildren();const h=document.createElement('tr');for(const title of titles){let e=document.createElement('th');e.textContent=title;h.append(e)}$('head').append(h);$('body').replaceChildren();let rows=mode==='scenes'?data.scenes:mode==='systems'?Object.entries(data.coverage.systems).map(([name,v])=>({name,...v})).sort((a,b)=>b.instances-a.instances):data.errors;rows=rows.filter(r=>JSON.stringify(r).toLowerCase().includes(query)&&(mode!=='scenes'||!state||r.status===state));$('count').textContent=rows.length+' matching records';for(const r of rows){const tr=document.createElement('tr');if(mode==='scenes'){const c=r.counts||{};cell(tr,r.file);cell(tr,r.scene_name);cell(tr,r.status).className=r.status;cell(tr,(c.sprites??0)+' / '+(c.terrain_edges??0));cell(tr,(c.meshes??0)+' / '+(c.gates??0));const td=cell(tr,'');const d=document.createElement('details'),s=document.createElement('summary'),p=document.createElement('pre');s.textContent=(c.errors??0)+' errors · '+(c.unsupported??0)+' unsupported';p.textContent=JSON.stringify({components:r.component_types,errors:r.errors,unsupported:r.unsupported,failure:r.failure},null,2);d.append(s,p);td.append(d)}else if(mode==='systems'){cell(tr,r.name);cell(tr,r.instances);cell(tr,r.scene_indices.length);cell(tr,r.gameplay_support)}else{cell(tr,r.type);cell(tr,r.instances);cell(tr,r.reason)}$('body').append(tr)}}
for(const name of ['scenes','systems','errors'])$(name+'Btn').onclick=()=>{mode=name;draw()};$('search').oninput=draw;$('status').onchange=draw;draw();</script>'''


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,default=ROOT/'.hkpsx/world-import/report.json')
    args=parser.parse_args()
    path=args.report.resolve()
    if not path.is_relative_to((ROOT/'.hkpsx').resolve()):
        parser.error('Report must be in .hkpsx')
    output=path.with_name('coverage.html')
    output.write_text(render(json.loads(path.read_text())))
    print(output)

if __name__=='__main__':main()
