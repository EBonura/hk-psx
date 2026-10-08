"""The milestone table must agree with the package rows underneath it.

Both tables are hand-written prose in the same document, and the milestone one
is a summary of the other. It had drifted: three milestones read "pending" while
every package inside them was in progress, which reads as no work started on
more than half the project.

That matters because this document is what gets read before work is planned. A
row saying a milestone has not started is the kind of thing that sends someone
to build what already exists, which happened twice in one day from package rows
that lagged their commits.

Deriving the milestone state here rather than trusting it is the same rule the
rest of this suite applies to budgets: compute the number, do not restate it.
"""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOC = ROOT / 'docs/plans/COMPLETE_GAME_PROGRESS.md'

MILESTONE = re.compile(r'^\|\s*(M\d[^|]*?)\s*\|\s*P(\d+)-P(\d+)\s*\|\s*([^|]+?)\s*\|$', re.M)
# A row may name several packages, as `P12, P22-P31` does, so the class has to
# carry the P of the second one too.
PACKAGE = re.compile(r'^\|\s*(P[\dP/, -]+?)\s*\|\s*([^|]+?)\s*\|', re.M)


def package_states():
    """Every package id to its stated state, expanding rows that name several."""
    states = {}
    for ids, state in PACKAGE.findall(DOC.read_text()):
        if ids.startswith('Package'):
            continue
        state = state.strip().strip('*')
        # Rows join ids three ways: `P12, P22-P31` by comma, `P19/P20` by slash
        # for two packages worked as one, and `P22-P31` as a range.
        for part in ids.replace(',', ' ').replace('/', ' ').split():
            found = re.fullmatch(r'P(\d+)(?:-P(\d+))?', part.strip())
            if not found:
                continue
            first = int(found.group(1))
            last = int(found.group(2)) if found.group(2) else first
            for number in range(first, last + 1):
                states[number] = state
    return states


def rollup(states):
    """A milestone is as far along as its least finished package."""
    if not states:
        return 'pending'
    if all(state == 'validated' for state in states):
        return 'validated'
    if any(state != 'pending' for state in states):
        return 'in progress'
    return 'pending'


class MilestoneTests(unittest.TestCase):
    def test_every_milestone_matches_the_packages_it_covers(self):
        packages = package_states()
        self.assertTrue(packages, 'no package rows found; the table shape changed')
        milestones = MILESTONE.findall(DOC.read_text())
        self.assertTrue(milestones, 'no milestone rows found; the table shape changed')
        for name, first, last, stated in milestones:
            covered = [packages[n] for n in range(int(first), int(last) + 1) if n in packages]
            self.assertEqual(len(covered), int(last) - int(first) + 1,
                             f'{name} covers P{first} to P{last} and some have no row')
            expected = rollup(covered)
            self.assertEqual(stated.strip().strip('*'), expected,
                             f'{name} says "{stated}" and its packages say "{expected}": {covered}')


if __name__ == '__main__':
    unittest.main()
