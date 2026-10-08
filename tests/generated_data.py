"""Whether the generated Rust under `data/` is behind the code that reads it.

The build runs the host suite before the cook, so any test that compiles guest
code sits ahead of the step that rewrites `data/regions.rs`. Add a field to a
cooked struct and the tree lands here every time: the generated literal is short,
the guest will not compile, and a test that fails on it blocks the very cook that
would fix it. Three tests deadlocked the build that way in one day.

Staleness is not a regression, so those tests stand down for it and say so. A
compile failure anywhere in hand-written code is still a failure.
"""
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_ANSWER = None


def scene_count_disagrees():
    """A reason the cooked tables disagree with each other, or None.

    Asked before compiling anything, because the compile cannot answer it. The
    guest asserts `SCENES == disc::SCENE_COUNT`; `SCENES` comes from
    `data/regions.rs` and `SCENE_COUNT` is the length of `SCENE_MANIFEST` in
    `data/scene_manifest.rs`. Both are generated, and different steps of the
    cook write them, so a build that stops in between leaves them disagreeing.

    A const assert reports its error in the hand-written file that holds it, so
    the `/data/` rule below never sees a generated path and stands nothing down.
    That deadlocked a build: the suite refused the cook that would have made the
    two agree, and only a hand-run cook got out of it.
    """
    import re
    regions, manifest = ROOT / 'data/regions.rs', ROOT / 'data/scene_manifest.rs'
    if not regions.is_file() or not manifest.is_file():
        return 'the scene tables are not cooked yet; the cook writes them'
    found = re.search(r'\bSCENES\s*:\s*usize\s*=\s*(\d+)\s*;', regions.read_text())
    text = manifest.read_text()
    start = text.find('SCENE_MANIFEST')
    if not found or start < 0:
        return None
    # Count entries after the declaration, so the struct definition above it
    # does not read as a forty-seventh scene. It did, which is why this took
    # three attempts to see.
    entries = len(re.findall(r'SceneDesc\s*\{', text[start:]))
    if entries and entries != int(found.group(1)):
        return (f'data/regions.rs carries {found.group(1)} scenes and '
                f'SCENE_MANIFEST carries {entries}; the cook rewrites both')
    return None


def stale_generated_data():
    """A reason to skip, or None when the guest compiles.

    Cached: the check costs about a second and several tests ask.
    """
    global _ANSWER
    if _ANSWER is not None:
        return _ANSWER[0]
    behind = scene_count_disagrees()
    if behind:
        _ANSWER = (behind,)
        return behind
    check = subprocess.run(
        ['cargo', 'check', '--quiet', '--message-format=short', '--target', 'mipsel-sony-psx'],
        cwd=ROOT / 'game', capture_output=True, text=True)
    if check.returncode == 0:
        _ANSWER = (None,)
        return None
    errors = [line for line in (check.stdout + check.stderr).splitlines() if ': error' in line]
    # rustc prints an absolute path and `data` is reached from the crate as
    # `../data`, so match the file rather than a prefix.
    generated = [line for line in errors if '/data/' in line.split(':', 1)[0]]
    if errors and len(generated) == len(errors):
        _ANSWER = (f'{len(errors)} guest errors, all in generated data; the cook rewrites it',)
    else:
        # Not staleness. Let the test report it rather than swallowing it here.
        _ANSWER = (None,)
    return _ANSWER[0]
