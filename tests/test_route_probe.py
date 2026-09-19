"""The native route probe must build against the current guest and report contacts.

Nothing else exercises tools/route_probe.rs, so this is what catches it drifting
out of step with the guest API or with the modules world.rs declares.
"""
import json,subprocess,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def probe(route,ticks=None):
    # Probe the live cook report, not `.hkpsx/selected-regions.json`: that one is
    # a copy frozen at the last guest build, so any cook since then makes the
    # probe's pack-hash guard fire on staleness rather than on a half-written
    # pack. These tests run inside the build, ahead of the cook that would
    # refresh the copy.
    args=['--route',route,'--metadata',str(ROOT/'data/regions.json')]+(['--ticks',str(ticks)] if ticks else [])
    run=subprocess.run([str(ROOT/'.venv/bin/python'),str(ROOT/'tools/route_probe.py')]+args,
        cwd=ROOT,capture_output=True,text=True)
    return run
class RouteProbeTests(unittest.TestCase):
    def test_named_fixtures_traverse_and_report_contacts(self):
        fixtures=json.loads((ROOT/'tools/route_fixtures.json').read_text())['routes']
        for name in ('doors','crawler','lower_staircase','middle_staircase'):
            with self.subTest(route=name):
                run=probe(name)
                self.assertEqual(run.returncode,0,run.stdout+run.stderr)
                rows=[line for line in run.stdout.splitlines() if line and line[0].isdigit()]
                self.assertGreater(len(rows),1,'probe produced no trajectory')
                self.assertIn('final ',run.stderr)
        self.assertIn('kings_pass_spikes',fixtures)
    def test_the_hazard_route_still_reaches_the_spikes(self):
        fixture=json.loads((ROOT/'tools/route_fixtures.json').read_text())['routes']['kings_pass_spikes']
        run=probe('kings_pass_spikes')
        self.assertEqual(run.returncode,0,run.stdout+run.stderr)
        contact=[line for line in run.stderr.splitlines() if line.startswith('final ')]
        self.assertTrue(contact,run.stderr)
        ticks=int(contact[-1].split('hazard_contact_ticks=')[1])
        self.assertGreater(ticks,0,'the probe no longer reports King\'s Pass hazard contact')
        self.assertEqual(fixture['ticks'],1450)
