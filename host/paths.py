"""Single local disc-library destination. Never stage disc images in the repo."""
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
# The disc's name in the library, as in host/hk-build/disc.rs (DISC_NAME).
DISC_NAME = 'Hollow Knight (hk-psx)'
DISC_LIBRARY = Path(os.environ['HK_DISC_LIBRARY']) if 'HK_DISC_LIBRARY' in os.environ else Path.home() / 'Downloads' / 'ps1 games'


def build_directory(telemetry=False, candidate=False):
    if candidate:
        return ROOT / 'build' / 'candidate' / ('telemetry' if telemetry else 'normal')
    return ROOT / 'build'


def link_map_path(telemetry=False, candidate=False):
    return build_directory(telemetry, candidate) / ('hk-psx-telemetry.map' if telemetry else 'hk-psx-normal.map')


def build_report_path(telemetry=False, candidate=False, pgo=False):
    variant = 'telemetry' if telemetry else 'normal'
    prefix = 'build-candidate' if candidate else 'build-pgo' if pgo else 'build'
    return ROOT / '.hkpsx' / f'{prefix}-{variant}.json'


def hazard_report_path(telemetry=False, candidate=False):
    if candidate:
        return build_directory(telemetry, True) / 'hazards.json'
    return ROOT / '.hkpsx' / 'hazards.json'


def artifacts(telemetry=False, candidate=False):
    # User-facing delivery is always one pair. Variant maps and compiler caches
    # may remain internal; neither flag creates another selectable game disc.
    return {'exe': ROOT / 'dist/hk-psx.exe',
            'bin': DISC_LIBRARY / f'{DISC_NAME}.bin',
            'cue': DISC_LIBRARY / f'{DISC_NAME}.cue'}
