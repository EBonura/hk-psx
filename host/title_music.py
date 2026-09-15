"""Cook the source Title music to full-quality stereo Red Book track02."""
import hashlib,json,subprocess,wave
from pathlib import Path
from source import ROOT,Source,dump
from cook_music import decoded_source,sha,source_snapshot

def sector_pad(data):
    if not data or len(data)%4:raise ValueError('Expected nonempty stereo PCM16 frames')
    return data+bytes((-len(data))%2352)

def main():
    out=ROOT/'.hkpsx/title-music';out.mkdir(parents=True,exist_ok=True)
    s=Source();level=s.file('level1');manager=s.read(level.objects[11613]);cue=s.ref(level,manager['musicCue']);t=s.read(cue)
    if t['m_Name']!='Title' or t['alternatives'] or manager['musicDelayTime']!=1 or manager['musicTransitionTime']!=0:raise ValueError('Title cue changed')
    channels=t['channelInfos'];clip=s.ref(cue.assets_file,channels[0]['clip'])
    if s.sid(clip)!='sharedassets1.assets:94' or any(c['clip']['m_PathID']for c in channels[1:]):raise ValueError('Title channel mapping changed')
    resource=s.file('resources.assets');audio=s.read(resource.objects[13887]);group=s.ref(resource,audio['OutputAudioMixerGroup']);snapshot=source_snapshot(s,s.ref(cue.assets_file,t['snapshot']),group)
    if not audio['Loop'] or audio['m_Pitch']!=1 or audio['m_Volume']!=0 or abs(snapshot['internal_volume_db'])>1e-6:raise ValueError('unsupported title configuration')
    # Serialized gain0 is the inactive voice. Original BeginApplyMusicCue sets
    # it to1 before Play, confirmed by the managed original playback trace.
    wav,identity=decoded_source(s,clip,out)
    inputs={n:sha(s.directory/n)for n in ('level1','sharedassets1.assets','resources.assets','Managed/Assembly-CSharp.dll')}
    code={n:sha(ROOT/'host'/n)for n in ('title_music.py','cook_music.py','source.py')}
    report_path=ROOT/'.hkpsx/title-music.json';payload_path=ROOT/'data/title_music.cdda';manifest=ROOT/'data/title_music.rs'
    if report_path.exists():
        old=json.loads(report_path.read_text())
        if old.get('inputs')==inputs and old.get('code')==code and old.get('identity')==identity and payload_path.exists() and manifest.exists() and sha(payload_path)==old['sha256'] and sha(manifest)==old['manifest_sha256']:
            print('Title CDDA cache verified');return
    raw=out/'stereo-44100.s16le';subprocess.run(['ffmpeg','-v','error','-y','-i',str(wav),'-ar','44100','-ac','2','-f','s16le',str(raw)],check=True)
    pcm=raw.read_bytes();payload=sector_pad(pcm);frames=len(pcm)//4
    payload_path.write_bytes(payload)
    manifest.write_text('// Generated Title CDDA descriptor; no song bytes in guest RAM or SPU.\n'+f'pub const TITLE_TRACK:u8=2;\npub const TITLE_DELAY:u32=60;\npub const TITLE_GAIN:i16=10922;\npub const TITLE_SECTORS:u32={len(payload)//2352};\n')
    dump(report_path,dict(inputs=inputs,code=code,identity=identity,path=str(payload_path),byte_len=len(payload),sha256=sha(payload_path),manifest_sha256=sha(manifest),frames=frames,padding_frames=(len(payload)-len(pcm))//4,rate=44100,channels=2,source=s.sid(clip),snapshot=snapshot,delay=1,loop=True,track=2,source_gain=1,mix_headroom=1/3,limitations='Full source resampled48kHz to44.1kHz stereo PCM16. Original mixer effects not reproduced. CDDA repeat entails physical seek; no gapless-loop claim.'))
    print('Title CDDA:',len(payload),'bytes,',frames/44100,'seconds, padding',(len(payload)-len(pcm))//4,'frames')
if __name__=='__main__':main()
