"""Source clip conversion shared with the Runner cook (runner_audio.py).

The music and ambience inventory, the Focus, Geo, ambience and area-music cooks
and the conversions behind them are Rust now (host/hk-cook/src/music_report.rs,
music.rs, ambience.rs, area_music.rs). This file keeps the clip helpers
runner_audio.py still imports until that cook moves too.
"""
import array,hashlib,json,math,struct,subprocess,sys,wave
from pathlib import Path
from source import ROOT,dump


def sha(path):
    with Path(path).open('rb')as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
def source_audio_ref(tree):
    """Unity6 uses AudioResource while legacy m_audioClip is often null."""
    for name in ('m_Resource','m_audioClip'):
        value=tree.get(name)
        if isinstance(value,dict)and value.get('m_PathID'):return value
    raise ValueError('AudioSource has no direct audio resource')
def compile_encoder(out):
    """The ADPCM encoder command (host/spu_encode.py over the SDK's
    psx-audio-cook). `out` is kept for callers; nothing is written there."""
    import spu_cook
    spu_cook.binary()
    return ROOT/'host/spu_encode.py'

def decoded_source(source,obj,folder):
    tree=source.read(obj);ref=tree['m_Resource'];resource=(source.directory/ref['m_Source']).resolve()
    if not resource.is_relative_to(source.directory.resolve()):raise ValueError('audio resource escapes Windows source')
    with resource.open('rb')as reader:
        reader.seek(ref['m_Offset']);encoded=reader.read(ref['m_Size'])
    if len(encoded)!=ref['m_Size']:raise ValueError('truncated source audio resource')
    identity={'clip_metadata_sha256':hashlib.sha256(json.dumps(tree,sort_keys=True).encode()).hexdigest(),
              'encoded_resource_sha256':hashlib.sha256(encoded).hexdigest()}
    wav=folder/'source.wav';stamp=folder/'source-wav.json';valid=False
    if wav.exists()and stamp.exists():
        previous=json.loads(stamp.read_text())
        valid=all(previous.get(k)==v for k,v in identity.items())and previous.get('wav_sha256')==sha(wav)
    if not valid:
        values=obj.read().samples
        if len(values)!=1:raise ValueError('expected one decoded sample resource')
        wav.write_bytes(next(iter(values.values())))
        dump(stamp,dict(identity,wav_sha256=sha(wav)))
    return wav,identity


def cook_clip(source,obj,out,encoder,rate,channels,resampler='ffmpeg'):
    sid=source.sid(obj);tree=source.read(obj);folder=out/sid.replace(':','-');folder.mkdir(exist_ok=True)
    wav,source_identity=decoded_source(source,obj,folder)
    with wave.open(str(wav),'rb')as reader:
        source_frames=reader.getnframes();source_rate=reader.getframerate();source_channels=reader.getnchannels()
        if reader.getsampwidth()!=2:raise ValueError('expected source PCM16')
    version=subprocess.check_output(['ffmpeg','-version'],text=True).splitlines()[0]
    raw=folder/f'{rate}-{channels}.s16le'
    if resampler=='sdk':
        # The SDK's shared resampler (host/spu_cook.py resample); mono only.
        import spu_cook
        if channels!=1:raise ValueError('the SDK resampler path is mono')
        raw.write_bytes(array.array('h',spu_cook.resample(wav.read_bytes(),rate)).tobytes())
        version='psx_audio_cook::resample::Sinc (SDK shared resampler)'
    else:
        subprocess.run(['ffmpeg','-v','error','-y','-i',str(wav),'-ar',str(rate),'-ac',str(channels),'-f','s16le',str(raw)],check=True)
    pcm=array.array('h');pcm.frombytes(raw.read_bytes())
    if sys.byteorder!='little':pcm.byteswap()
    if len(pcm)%channels:raise ValueError('partial PCM frame')
    frames=len(pcm)//channels;planes=[]
    for channel in range(channels):
        mono=pcm[channel::channels];mono_path=folder/f'{rate}-{channels}-ch{channel}.s16le'
        if sys.byteorder!='little':mono.byteswap()
        mono_path.write_bytes(mono.tobytes());encoded=mono_path.with_suffix('.adpcm')
        metric=json.loads(subprocess.check_output([str(encoder),str(mono_path),str(encoded)],text=True))
        data=encoded.read_bytes()
        if len(data)!=math.ceil(frames/28)*16 or any(data[i+1]for i in range(0,len(data),16)):raise ValueError('invalid encoded payload')
        # Independent FFmpeg decode confirms valid framing and records an external
        # quality metric. Its history rounding differs slightly from the SPU's,
        # so sample equality is not claimed.
        vag=encoded.with_suffix('.vag');vag.write_bytes(b'VAGp'+struct.pack('>4I',0x20,0,len(data),rate)+bytes(28)+data)
        decoded=encoded.with_suffix('.decoded.s16le')
        subprocess.run(['ffmpeg','-v','error','-y','-i',str(vag),'-f','s16le',str(decoded)],check=True)
        recon=array.array('h');recon.frombytes(decoded.read_bytes())
        if sys.byteorder!='little':recon.byteswap();mono.byteswap()
        if len(recon)!=math.ceil(frames/28)*28:raise ValueError('external decode frame mismatch')
        error=sum((int(a)-b)**2 for a,b in zip(mono,recon));signal=sum(int(a)**2 for a in mono)
        planes.append({'path':str(encoded.relative_to(ROOT)),'bytes':len(data),'sha256':sha(encoded),
            'encoder_metric':metric,'ffmpeg_snr_db':10*math.log10(signal/error)if signal and error else None})
    return {'source':sid,'name':tree['m_Name'],'source_metadata':tree,'source_identity':source_identity,'source_wav_sha256':sha(wav),
        'source_pcm_frames':source_frames,'source_pcm_rate':source_rate,'source_pcm_channels':source_channels,
        'rate':rate,'spu_pitch':round(rate*4096/44100),'spu_actual_rate':round(rate*4096/44100)*44100/4096,'channels':channels,'frames':frames,'seconds':frames/rate,'padding_samples':(-frames)%28,
        'bytes':sum(p['bytes']for p in planes),'bytes_per_second':rate*channels*16/28,'planes':planes,
        'conversion':version,'flags':'All payload flags zero; runtime transport must install loop/chunk boundaries',
        'loop_boundary':'Full-clip sample count preserved in metadata; final ADPCM block has at most27 zero samples'}
