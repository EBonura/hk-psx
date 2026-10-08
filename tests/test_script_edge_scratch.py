"""Every consumer of the shared scripted-edge scratch survives a rebuild of it.

`world::State` keeps one bounded list for scripted terrain exclusions. One
module sets it, which discards whatever is there, and the others append to it.
So a caller that sets it mid-tick silently drops every appender's edges until
the next frame rebuilds them, and the symptom is a piece of terrain that exists
for a single simulation tick. That is close to undiagnosable from a bug report:
it is a wall the player walks into once.

It happened. `frame.rs` refreshed the cocoons when one was struck and did not
re-apply the Great Door's exclusions or the arena gates', while the other three
call sites in the guest did. It was unreachable only because no catalogue slot
carries both a cocoon and a gate, which is a fact about today's world rather
than about the code.

This finds the setter and the appenders by what they call rather than by name,
so a third consumer of the scratch is covered the day it is written instead of
the day someone remembers to add it here.
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GUEST = ROOT / 'game/src'
# The two halves of the contract, named by the methods on world::State rather
# than by the modules that happen to use them today.
SETTER = 'set_lifeblood_edges'
APPENDER = 'append_script_edges'
# Call sites that rebuild the scratch are a short run of statements, not a
# scope, so the check reads forward a few statements rather than parsing Rust.
# Comments and blanks do not count towards it: the render path explains itself
# at length between two of these calls, and a comment must not be able to break
# the rule it is explaining.
WINDOW = 4


def modules_calling(method):
    """Guest modules whose `apply` calls the given State method.

    Matched on the call rather than the name, so `world.rs`, which defines both
    methods and has an `apply` of its own, is not counted as a consumer of the
    scratch it owns.
    """
    found = set()
    for path in sorted(GUEST.rglob('*.rs')):
        text = path.read_text()
        if f'.{method}(' in text and re.search(r'\bfn apply\b', text):
            found.add(path.stem)
    return found


class ScriptEdgeScratchTests(unittest.TestCase):
    def setUp(self):
        self.setters = modules_calling(SETTER)
        self.appenders = modules_calling(APPENDER)
        self.assertTrue(self.setters, f'no guest module calls {SETTER}')
        self.assertTrue(self.appenders, f'no guest module calls {APPENDER}')
        # A module that both sets and appends would make the ordering rule
        # ambiguous, and none does today.
        self.assertFalse(self.setters & self.appenders,
                         'a module both sets and appends the scratch')

    def test_every_rebuild_of_the_scratch_reapplies_every_appender(self):
        missing = []
        for path in sorted(GUEST.rglob('*.rs')):
            lines = path.read_text().splitlines()
            for number, line in enumerate(lines):
                setter = next((m for m in self.setters if f'{m}::apply(' in line), None)
                if setter is None:
                    continue
                code = [after for after in lines[number:]
                        if after.strip() and not after.strip().startswith('//')]
                window = '\n'.join(code[:WINDOW])
                for appender in sorted(self.appenders):
                    if f'{appender}::apply(' not in window:
                        missing.append(f'{path.name}:{number + 1} calls {setter}::apply, '
                                       f'which discards the scratch, and does not re-apply '
                                       f'{appender}::apply within {WINDOW} lines')
        self.assertFalse(missing, 'the shared scripted-edge scratch is rebuilt without '
                                  'every consumer:\n  ' + '\n  '.join(missing))

    def test_the_appenders_skip_an_edge_the_scratch_already_holds(self):
        """Re-applying must be safe, or the fix above would overflow the scratch.

        `SCRIPT_EDGE_SLOTS` is 20 and the worst real slot needs 17, so an
        appender that added a duplicate would overflow a room that fits today.
        """
        world = (GUEST / 'world.rs').read_text()
        body = world[world.index(f'pub fn {APPENDER}'):]
        body = body[:body.index('\n    }')]
        self.assertIn('contains(&edge)', body,
                      f'{APPENDER} no longer skips edges it already holds, so re-applying '
                      'after a rebuild would consume a slot per call')


if __name__ == '__main__':
    unittest.main()
