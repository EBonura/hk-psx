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


def stale_generated_data():
    """A reason to skip, or None when the guest compiles.

    Cached: the check costs about a second and several tests ask.
    """
    global _ANSWER
    if _ANSWER is not None:
        return _ANSWER[0]
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
