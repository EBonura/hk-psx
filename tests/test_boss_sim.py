"""The offline False Knight simulator must still predict the fight the disc runs.

Nothing else exercises tools/boss_sim.rs, so this is what catches it drifting out
of step with the guest. That matters more here than for most tools: a simulator
that has quietly drifted is worse than no simulator, because it approves tapes
that then fail on hardware at 85 seconds a replay, and it does so confidently.

The disc is not available to a unit test, so what is compared against is a
capture. `tools/tapes/<tape>.capture.json` holds every HK_FK_HP, HK_FK_HEAD_HP
and HK_HEALTH transition a real tools/replay_cue.py run of that tape produced
against the disc, plus that run's final telemetry. capture() in
tools/boss_sim.py regenerates all three, and its docstring carries the commands.
If the disc changes, refresh the captures from replays rather than relaxing
anything here.

Four tapes, because one cannot cover the paths that break a tape silently:

  * boss-fight is the kill, and the only one that reaches a stagger, an exposed
    Head, a rage, the barrels and the death sequence. Its hero is never hit.
  * boss-death walks in and stands still, so it is the boss's whole attack
    pattern against a stationary hero and the only one that reaches a death.
  * boss-wave backs away to the arena's left end, so the boss slams from
    range and sends its Shockwave Wave across the floor: the only tape that
    reaches a wave, one of which lands and three of which are jumped.
  * boss-sim-brawl is a fixture rather than a route: the first thousand polls of
    the old cheat-assisted kill tape with its pause-menu events dropped. It is
    the only one that both swings and takes damage, which is what exercises
    main.rs::respond_to_hurt cancelling a swing in flight. Losing that one made
    an earlier simulator invent a nail hit across every recoil.

What the three do not reach is a gate: none of them walks into either end of
the sealed floor, so the terrain tools/boss_sim.py lifts for a gate the source
loads open is exercised only by the world-table test below, not by a replay. A
tape that pushes against x 10.75 or x 46.25 would close that gap.

One poll of slack is allowed on each transition and no more. The guest catches
its simulation up in bursts of two ticks, so route.csv can report a state one
tick either side of the poll it belongs to; that is log sampling, not a
different simulation. Two polls of drift is a real difference.
"""
import hashlib
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TAPES = ('boss-fight', 'boss-death', 'boss-sim-brawl', 'boss-wave')
sys.path.insert(0, str(Path(__file__).resolve().parent))
from generated_data import stale_generated_data


def cooked():
    """Whether there is a finished catalogue to simulate against.

    The build runs this suite ahead of the cook, so an interrupted cook leaves a
    catalogue that is internally consistent and simply unfinished, which
    tools/boss_sim.py refuses by design. Failing on that would deadlock the
    build, the way tests/test_route_probe.py explains at more length, so it skips
    loudly instead.
    """
    report = ROOT/'data/regions.json'
    if not report.is_file():
        return False
    with report.open() as stream:
        catalogue = json.load(stream)
    if catalogue.get('complete') is not True:
        return False
    for region in catalogue['regions']:
        if region['scene_name'] != 'Crossroads_10':
            continue
        path = ROOT/region['path']
        if not path.is_file():
            return False
        if hashlib.sha256(path.read_bytes()).hexdigest() != region['sha256']:
            return False
    return True


@unittest.skipUnless(cooked(), 'the cooked catalogue is unfinished; the cook this build runs will finish it')
@unittest.skipIf(stale_generated_data(), 'the generated Rust is behind the guest; the cook rewrites it')
class BossSimTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.captures, cls.summaries, cls.runs = {}, {}, {}
        with tempfile.TemporaryDirectory(prefix='hk-boss-sim-') as work:
            for name in TAPES:
                cls.captures[name] = json.loads((ROOT/f'tools/tapes/{name}.capture.json').read_text())
                summary = Path(work)/f'{name}.json'
                cls.runs[name] = subprocess.run(
                    [str(ROOT/'.venv/bin/python'), str(ROOT/'tools/boss_sim.py'), 'replay',
                     str(ROOT/f'tools/tapes/{name}.pxtape'), '--summary', str(summary)],
                    cwd=ROOT, capture_output=True, text=True)
                cls.summaries[name] = json.loads(summary.read_text()) if summary.is_file() else None

    def test_the_simulator_still_builds_and_replays_every_committed_tape(self):
        for name in TAPES:
            with self.subTest(tape=name):
                run = self.runs[name]
                self.assertEqual(run.returncode, 0, run.stdout+run.stderr)
                self.assertIsNotNone(self.summaries[name], run.stdout+run.stderr)

    def test_every_capture_describes_the_tape_beside_it(self):
        for name in TAPES:
            with self.subTest(tape=name):
                capture = self.captures[name]
                tape = ROOT/f'tools/tapes/{name}.pxtape'
                self.assertEqual(capture['tape'], f'tools/tapes/{name}.pxtape')
                self.assertEqual(capture['tape_sha256'],
                                 hashlib.sha256(tape.read_bytes()).hexdigest(),
                                 'the capture was taken from a different tape than the one committed')
                self.assertTrue(capture['completed'])
                self.assertEqual(capture['faults'], [])

    def test_the_hero_starts_where_the_card_leaves_it(self):
        # The simulator has no memory card, so where the save puts the hero is a
        # constant in tools/boss_sim.py. This is what notices the card, the bench
        # or the spawn moving underneath it.
        source = (ROOT/'tools/boss_sim.py').read_text()
        start = re.search(r'^START = \((-?\d+), (-?\d+), (-?\d+)\)', source, re.MULTILINE)
        self.assertIsNotNone(start, 'tools/boss_sim.py no longer declares START')
        for name in TAPES:
            with self.subTest(tape=name):
                self.assertEqual((int(start[1]), int(start[2])),
                                 (self.captures[name]['hero_start']['x'],
                                  self.captures[name]['hero_start']['y']))

    def test_the_predicted_fight_is_the_fight_the_disc_ran(self):
        for name in TAPES:
            capture, summary = self.captures[name], self.summaries[name]
            for counter in ('fk_hp', 'head_hp', 'health'):
                expected = capture['transitions'][counter]
                actual = summary['transitions'][counter]
                with self.subTest(tape=name, counter=counter):
                    self.assertEqual(len(actual), len(expected),
                                     f'{name} {counter}:\ndisc      {expected}\nsimulator {actual}')
                    for (ep, ea, eb), (ap, aa, ab) in zip(expected, actual):
                        self.assertEqual((ea, eb), (aa, ab),
                                         f'{name} {counter} near poll {ep}: '
                                         f'disc {ea}->{eb}, simulator {aa}->{ab}')
                        self.assertLessEqual(abs(ep-ap), 1,
                                             f'{name} {counter} {ea}->{eb}: '
                                             f'disc at poll {ep}, simulator at {ap}')

    def test_the_kill_leaves_the_hero_untouched(self):
        # The committed route's whole claim, and the cheapest thing to lose: an
        # empty health series is a fight the Knight walked out of without a hit.
        capture, summary = self.captures['boss-fight'], self.summaries['boss-fight']
        self.assertEqual(capture['transitions']['health'], [])
        self.assertEqual(summary['transitions']['health'], [])
        self.assertEqual(summary['final']['health'], capture['final']['HK_HEALTH'])
        self.assertEqual(summary['final']['hero_deaths'], capture['final']['HK_DEATHS'])
        # And that it is still the mortal kill. The simulator cannot model a
        # cheat at all, so this reads the disc's own word.
        self.assertEqual(capture['final']['HK_CHEATS'], 0)
        self.assertEqual(capture['final']['HK_INPUT_FAULT'], 0)

    def test_the_predicted_ending_is_the_kill(self):
        summary, disc = self.summaries['boss-fight'], self.captures['boss-fight']['final']
        self.assertEqual(summary['final']['staggers'], disc['HK_FK_STAGGERS'])
        self.assertEqual(summary['final']['conversions'], disc['HK_FK_CONVERSIONS'])
        self.assertEqual(summary['final']['fk_deaths'], disc['HK_FK_DEATHS'])
        self.assertEqual(summary['final']['arena'], 'Open')
        self.assertFalse(summary['final']['left_arena'])

    def test_the_cooked_world_the_wrapper_derives_is_still_there(self):
        # The wrapper reads the slots, the gate terrain and the boss's ActorSpec
        # out of generated data rather than restating them, so a table that
        # changes shape shows up as an empty derivation rather than a wrong
        # fight. Nothing else notices an empty one.
        sys.path.insert(0, str(ROOT/'tools'))
        import boss_sim
        _, regions = boss_sim.cooked_regions()
        self.assertGreater(len(regions), 1, 'no cooked boss-scene slots')
        gates = boss_sim.open_gate_edges()
        # `cooked_regions` yields chunk ids; the gate table is keyed by the
        # runtime catalogue slot, which is one less.
        slots = {slot - 1 for slot, _, _, _ in regions}
        self.assertTrue(slots & set(gates), 'no slot of the boss scene carries gate terrain')
        floor = boss_sim.broken_floor_edges()
        self.assertTrue(slots >= set(floor), 'the broken floor names a slot outside the boss scene')
        spec = boss_sim.boss_spec()
        self.assertEqual(spec['health'], 65)
        self.assertNotEqual(spec['bounds'], [0, 0, 0, 0])
        self.assertNotEqual(spec['trigger'], [0, 0, 0, 0])

    def test_the_waves_the_simulator_throws_are_the_disc_s(self):
        # hk_sim::shockwave is shared by the guest and the simulator, but the
        # spawn point, the terrain it runs on and the hero it meets are not.
        for name in TAPES:
            capture, summary = self.captures[name]['final'], self.summaries[name]['final']
            if 'HK_FK_WAVES' not in capture:
                continue
            with self.subTest(tape=name):
                self.assertEqual(summary['waves'], capture['HK_FK_WAVES'])
                self.assertEqual(summary['wave_hits'], capture['HK_FK_WAVE_HITS'])
        self.assertGreater(self.captures['boss-wave']['final']['HK_FK_WAVE_HITS'], 0)

    def test_the_losing_tape_still_loses_at_the_same_poll(self):
        # boss-death is the only capture that reaches a death, so it is the only
        # one that pins the boss's attack pattern against a hero who does not
        # move: five hits, and the fifth at the poll the disc took it.
        capture, summary = self.captures['boss-death'], self.summaries['boss-death']
        self.assertEqual(len(capture['transitions']['health']), 5)
        self.assertEqual(summary['final']['hero_deaths'], 1)
        died = capture['hero_died_at_poll']
        self.assertIsNotNone(died)
        self.assertLessEqual(abs(capture['transitions']['health'][-1][0]-died), 1)
