"""Inventory serialization, scripts, audio and text without executing Unity."""
from source import *
import collections,hashlib

def main():
    s=Source();g=s.file('globalgamemanagers');report={'unity':g.unity_version,'serialization':g.header.version,'type_trees':g._enable_type_tree,'native_errors':[]}
    for o in g.objects.values():
        if o.type.name not in ('BuildSettings','MonoManager','PlayerSettings','TimeManager','Physics2DSettings'):continue
        try:t=s.read(o)
        except Exception as ex:
            report['native_errors'].append({'type':o.type.name,'error':str(ex)})
            if o.type.name!='PlayerSettings':continue
            t=o.read_typetree(check_read=False)
        if o.type.name=='BuildSettings':report['scenes']=[{'index':i,'file':f'level{i}','path':name} for i,name in enumerate(t['scenes'])]
        elif o.type.name=='PlayerSettings':report['game_version_partial_schema']=t.get('bundleVersion')
        elif o.type.name=='MonoManager':report['mono_manager_fields']={k:v for k,v in t.items() if k!='m_ScriptHashes'}
        else:report[o.type.name]=t
    rf=s.file('resources.assets');counts=collections.Counter(o.type.name for o in rf.objects.values());report['resources_types']=dict(counts)
    audio=[];text=[];fonts=[]
    for o in rf.objects.values():
        if o.type.name=='AudioClip' and len(audio)<8:
            t=s.read(o);audio.append({'source':s.sid(o),**{k:v for k,v in t.items() if k not in ('m_AudioData',)}})
        if o.type.name=='TextAsset':
            t=s.read(o);data=t.get('m_Script',b'');text.append({'source':s.sid(o),'name':t['m_Name'],'bytes':len(data),'encoding_note':'content stays local; not exported in shared docs'})
        if o.type.name=='Font':fonts.append({'source':s.sid(o),'name':o.peek_name()})
    report.update(audio_samples=audio,text_assets=text,fonts=fonts)
    dump(ROOT/'.hkpsx/source-inspection.json',report)
    print('Source inspection:',len(report.get('scenes',[])),'scenes;',dict(counts));print('Audio sample',str(audio[0])[:1100] if audio else 'none');print('Text asset names',[x['name'] for x in text[:15]])
if __name__=='__main__':main()
