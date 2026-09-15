"""Check a long actual-CUE Title replay against the cooked stereo PCM.

Usage: .venv/bin/python tools/check_title_capture.py REPLAY_DIR --output REPORT
The replay must span at least two complete title repeats (10500 neutral polls).
"""
from pathlib import Path
import argparse,hashlib,wave,json,numpy as np
args=argparse.ArgumentParser(description=__doc__)
args.add_argument('replay',type=Path);args.add_argument('--output',type=Path,required=True);args=args.parse_args()
root=Path(__file__).resolve().parents[1]
report=json.loads((args.replay/'command.json').read_text())
assert report['completed'] and report['inputs_unchanged'] and not report['faults'], 'Replay failed'
assert report['final_ram']['HK_MUSIC_WRAPS']>=2 and report['final_ram']['HK_MUSIC_PLAYING']==1
build=json.loads((args.replay/'build-report.json').read_text())
payload=root/'data/title_music.cdda'
assert hashlib.sha256(payload.read_bytes()).hexdigest()==build['title_music']['sha256'], 'Source changed since replay'
src=np.fromfile(payload,dtype='<i2').reshape(-1,2)
with wave.open(str(args.replay/'audio.wav')) as w:
 assert w.getnchannels()==2 and w.getframerate()==44100
 pcm=np.frombuffer(w.readframes(w.getnframes()),dtype='<i2').reshape(-1,2)
# Find the same distinctive one-second phrase on each play, then refine alignment
# and compare the actual stereo mix over ten seconds, independently per repeat.
start=5*44100;length=44100;stride=8
a=pcm[::stride,0].astype(float);b=src[start:start+length:stride,0].astype(float)
n=1<<int((len(a)+len(b)-1).bit_length())
c=np.fft.irfft(np.fft.rfft(a,n)*np.fft.rfft(b[::-1],n),n)[:len(a)+len(b)-1]
checks=[]
for ordinal in range(3):
 lo=int((ordinal*len(src)/44100+4)*44100/stride)+len(b)-1;hi=min(len(c),int((ordinal*len(src)/44100+12)*44100/stride)+len(b)-1)
 coarse=(lo+int(np.argmax(c[lo:hi]))-len(b)+1)*stride
 best=max(range(coarse-8,coarse+9),key=lambda p:float(np.dot(pcm[p:p+length,0].astype(float),src[start:start+length,0].astype(float))))
 size=min(10*44100,len(pcm)-best,len(src)-start)
 x=src[start:start+size].astype(float);y=pcm[best:best+size].astype(float)
 gain=np.sum(x*y)/np.sum(x*x);residual=y-gain*x
 checks.append(dict(play=ordinal+1,offset_frames=best-start,offset_seconds=(best-start)/44100,gain=float(gain),correlation=float(np.corrcoef(x.ravel(),y.ravel())[0,1]),rms_error=float(np.sqrt(np.mean(residual**2))),frames_compared=size))
assert all(c['correlation']>.999 for c in checks),checks
r=dict(replay_report_sha256=hashlib.sha256((args.replay/'command.json').read_bytes()).hexdigest(),source_sha256=build['title_music']['sha256'],audio_sha256=hashlib.sha256((args.replay/'audio.wav').read_bytes()).hexdigest(),source_frames=len(src),capture_frames=len(pcm),stereo=True,checks=checks,repeat_gaps_seconds=[checks[i]['offset_seconds']-checks[i-1]['offset_seconds']-len(src)/44100 for i in [1,2]])
args.output.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r,indent=2))
