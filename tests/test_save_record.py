"""Compile and run the real save record, which nothing used to.

`game/src/save.rs` has a `#[cfg(test)] mod tests` that no cargo target reaches,
because the guest is a `no_std` PSX binary and the file pulls in `psx_mc` for
card I/O. Its assertions never executed. This compiles it against a stubbed card
surface the same way `tests/test_charms.py` compiles the charm board, so the
encode and decode halves are actually run against each other.

The record grew twice in one day and every growth invalidates every committed
card fixture, so the offsets in it are worth executing rather than reading.
"""
import os, re, subprocess, sys, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
import sys
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc
SAVE_RS = ROOT / 'game/src/save.rs'
HARNESS = ROOT / 'tests/save_runtime.rs'


class SaveRecordTests(unittest.TestCase):
    def test_the_record_round_trips_and_refuses_what_it_should(self):
        with tempfile.TemporaryDirectory(prefix='hk-save-test-') as temp:
            binary = Path(temp) / 'save-tests'
            compiled = subprocess.run(
                ['rustc', '--edition=2021', '-Awarnings', '--test', str(HARNESS), '-o', str(binary)],
                capture_output=True, text=True)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            run = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(run.returncode, 0, run.stdout + run.stderr)

    def test_the_harness_covers_every_field_the_record_carries(self):
        # A field added to `Save` without a line here would round trip untested,
        # which is exactly how a wrong offset survives.
        fields = set(rustsrc.struct_fields(SAVE_RS, 'Save'))
        harness = HARNESS.read_text()
        missing = sorted(f for f in fields if f not in harness)
        self.assertEqual(missing, [], f'tests/save_runtime.rs does not mention {missing}')

    def test_a_format_change_is_visible_here(self):
        # The magic and length live in one place and the migrator reads them
        # from it; this pins that they are still readable in that shape, because
        # tools/migrate_cards.py silently cannot run if they are not.
        sys.path.insert(0, str(ROOT / 'tools'))
        from migrate_cards import layout
        magic, length = layout()
        self.assertEqual(len(magic), 4)
        self.assertGreater(length, 4)
        self.assertEqual(rustsrc.const_int(SAVE_RS, 'LEN'), length)


if __name__ == '__main__':
    unittest.main()
