#!/usr/bin/env python3
"""Open the matching normal build in the PSoXide GUI with its library disc."""
import argparse, os, subprocess, sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'host'))
from paths import artifacts

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--emulator',default=str(ROOT.parent/'PSoXide-emulator/target/release/frontend'))
    a=p.parse_args();emulator=Path(a.emulator).resolve();paths=artifacts(False)
    for path in (emulator,paths['exe'],paths['cue'],paths['bin']):
        if not path.is_file():raise SystemExit(f'Missing {path}; run make build first.')
    # Verified frontend GUI hook: HLE-side-load the exact built EXE and mount
    # its matching disc. This requires no BIOS or user's emulator setting edit.
    env=dict(os.environ,PSOXIDE_EXE=str(paths['exe']),PSOXIDE_DISC=str(paths['cue']),PSOXIDE_AUTORUN='1')
    print('Playing:',paths['cue'],flush=True)
    subprocess.run([str(emulator),'--windowed'],env=env,check=True)
if __name__=='__main__':main()
