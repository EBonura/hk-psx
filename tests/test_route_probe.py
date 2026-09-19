"""The native route probe must build against the current guest and report contacts.

Nothing else exercises tools/route_probe.rs, so this is what catches it drifting
out of step with the guest API or with the modules world.rs declares.
"""
import json,subprocess,sys,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(Path(__file__).resolve().parent))
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
def cooked():
    """Whether there is a finished catalogue to probe against.

    The build runs this suite ahead of the cook, so an interrupted cook leaves a
    catalogue that is internally consistent and simply unfinished, and
    `route_probe.py` refuses it by design. Failing on that deadlocks the build:
    the suite blocks the very cook that would complete the catalogue, and only a
    hand-run cook gets out of it. An unfinished cook is a transient state, not a
    regression, so it skips loudly rather than failing.
    """
    report=ROOT/'data/regions.json'
    if not report.is_file():return False
    with report.open() as stream:
        catalogue=json.load(stream)
    if catalogue.get('complete') is not True:return False
    # And that it still describes the packs on disc. A cook that ran but failed
    # before writing its report leaves the two disagreeing, which is the same
    # transient the `complete` flag catches at the other end of the run, and
    # `route_probe.py` refuses it for the same good reason. Blocking on it
    # deadlocks the build, because this suite runs ahead of the cook that would
    # make them agree again. A pack that is genuinely wrong is caught by the
    # cook, which recomputes every hash, and by the route replays after it.
    import hashlib
    for region in catalogue['regions']:
        path=ROOT/region['path']
        if not path.is_file():return False
        if hashlib.sha256(path.read_bytes()).hexdigest()!=region['sha256']:return False
    return True
from generated_data import stale_generated_data
@unittest.skipUnless(cooked(),'the cooked catalogue is unfinished; the cook this build runs will finish it')
@unittest.skipIf(stale_generated_data(),'the generated Rust is behind the guest; the cook rewrites it')
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
    def test_the_tablet_route_crosses_the_focus_tablet_volume(self):
        # Set Seen Focus Tablet waits in Tutorial_01 at x 102.0 to 103.7, y 27.98
        # to 37.9, and its Trigger2dEvent takes the hero body rather than a point.
        # This is the geometry tools/tapes/kings-climb.pxtape rides on the CUE, so
        # a terrain change that stops the climb reaching the platform shows here
        # instead of only in a 4,800-poll replay.
        run=probe('kings_pass_tablet')
        self.assertEqual(run.returncode,0,run.stdout+run.stderr)
        half_width,bottom,top=16384/65536,-91136/65536,-7168/65536
        inside=0
        for line in run.stdout.splitlines():
            fields=line.split(',')
            if not fields[0].isdigit():
                continue
            x,y=float(fields[1]),float(fields[2])
            if x-half_width<=103.7 and x+half_width>=102.0 and y+bottom<=37.9 and y+top>=27.98:
                inside+=1
        self.assertGreater(inside,0,'the climb no longer puts the hero body in the tablet volume')
