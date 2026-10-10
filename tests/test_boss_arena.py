"""The arena the False Knight fights in: its gates, and what survives a reload.

Two halves. The first checks the generated gate table against the cooked region
report it was joined from, because an edge index that names the wrong edge moves
a room's terrain and nothing in a build log would say so. The second checks the
PlayerData reserve the fight persists through, because the guest's own guard is
a `const` assertion that only fires on a guest build, and the host suite runs
first.

Nothing here touches the installed game: `host/battle_gates.py` is what reads
it, and this runs against what that generated.
"""
import json
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
import sys
sys.path.insert(0, str(ROOT / 'host'))
import rustsrc
TABLE = ROOT / 'data/battle_gates.rs'
REPORT = ROOT / '.hkpsx/battle-gates.json'
REGIONS = ROOT / 'data/regions.json'


def rows():
    """(slot, gate, edges) out of the generated table."""
    text = TABLE.read_text()
    body = re.search(r'pub static REGIONS:&\[\(u16,u8,&\[u16\]\)\]=&\[(.*)\];', text, re.S)
    found = []
    for slot, gate, edges in re.findall(r'\((\d+),(\d+),&\[([^\]]*)\]\)', body.group(1)):
        found.append((int(slot), int(gate), [int(e) for e in edges.split(',') if e.strip()]))
    return found


def constant(name):
    found = re.search(rf'pub const {name}:u16=0b([01]+);', TABLE.read_text())
    return int(found.group(1), 2)


@unittest.skipUnless(TABLE.is_file() and REPORT.is_file(), 'run host/battle_gates.py first')
class GateTableTests(unittest.TestCase):
    def setUp(self):
        self.report = json.loads(REPORT.read_text())
        self.gates = self.report['gates']

    def test_every_binding_names_its_own_gate_collider(self):
        """The join, in the direction that matters: a wrong index is a wall."""
        regions = json.loads(REGIONS.read_text())['regions']
        for slot, gate, edges in rows():
            region = regions[slot]
            self.assertEqual(region['scene_name'], self.gates[gate]['scene_name'])
            for edge in edges:
                self.assertEqual(region['edge_sources'][edge], self.gates[gate]['collider_source'],
                                 f'slot {slot} edge {edge} is not gate {gate}')

    def test_no_cooked_gate_edge_is_left_unbound(self):
        """The other direction: an edge the table forgets never lifts."""
        regions = json.loads(REGIONS.read_text())['regions']
        owner = {gate['collider_source']: index for index, gate in enumerate(self.gates)}
        expected = {}
        for slot, region in enumerate(regions):
            for edge, source in enumerate(region['edge_sources']):
                if source in owner:
                    expected.setdefault((slot, owner[source]), []).append(edge)
        self.assertEqual({(slot, gate): edges for slot, gate, edges in rows()}, expected)

    def test_the_placement_state_is_read_off_the_source_and_not_off_the_cook(self):
        """`PLACEMENT_CLOSED` is the FSM's verdict; `COOKED` is the pack's.

        These used to be the same set, because `host/cook.py` baked only the
        gates the source loads closed. It bakes every gate now, so the two are
        independent and the guest's initial exclusion set is the difference
        between them: `COOKED & ~PLACEMENT_CLOSED`, the gates that are terrain
        in the pack and open on the first frame. Conflating them again in either
        direction is a wall or a hole, so each is checked against its own source.
        """
        closed = sum(1 << i for i, gate in enumerate(self.gates) if gate['solid_on_load'])
        self.assertEqual(constant('PLACEMENT_CLOSED'), closed)
        bound = sum(1 << gate for gate in {gate for _, gate, _ in rows()})
        self.assertEqual(constant('COOKED'), bound)

    def test_every_gate_the_cook_baked_carries_a_binding(self):
        """A baked gate with no row is terrain nothing can lift.

        This is the failure the cook's old skip made impossible and the recook
        makes the one to watch: the gate is in the pack, the runtime has no
        edges for it, and because no gate cooks a sprite the player walks into
        an invisible wall. `bind` derives the rows from `edge_sources`, so this
        checks the two halves of that join have not drifted apart.
        """
        regions = json.loads(REGIONS.read_text())['regions']
        owner = {gate['collider_source']: index for index, gate in enumerate(self.gates)}
        baked = {owner[source] for region in regions
                 for source in set(region['edge_sources']) if source in owner}
        self.assertEqual(baked, {gate for _, gate, _ in rows()})

    def test_the_shared_exclusion_scratch_holds_every_slot(self):
        """Gates, the Great Door and the Lifeblood cocoons share one list."""
        import sys
        sys.path.insert(0, str(ROOT / 'host'))
        import battle_gates
        budget = battle_gates.edge_scratch_slots()
        neighbours = battle_gates.neighbour_edges()
        spent = dict(neighbours)
        for slot, _, edges in rows():
            spent[slot] = spent.get(slot, 0) + len(edges)
        worst = max(spent.items(), key=lambda item: item[1])
        self.assertLessEqual(worst[1], budget, f'catalogue slot {worst[0]} needs {worst[1]}')

    def test_the_guest_module_against_the_real_table(self):
        with tempfile.TemporaryDirectory(prefix='hk-arena-gates-') as temp:
            binary = Path(temp) / 'run'
            compiled = subprocess.run(
                ['rustc', '--edition=2021', '-Awarnings',
                 str(ROOT / 'tests/battle_gates_runtime.rs'), '-o', str(binary)],
                env=dict(os.environ, CARGO_MANIFEST_DIR=str(ROOT / 'game')),
                capture_output=True, text=True)
            self.assertEqual(compiled.returncode, 0, compiled.stdout + compiled.stderr)
            ran = subprocess.run([str(binary)], capture_output=True, text=True)
            self.assertEqual(ran.returncode, 0, ran.stdout + ran.stderr)


class PlayerDataReserveTests(unittest.TestCase):
    """Where the arena's `Activated` bool and `falseKnightFirstPlop` live.

    Until HKS5 they rode the top two slots of the script bank's PlayerData
    reserve, which meant a recook that changed the bank's field list dropped a
    won boss fight along with the script values. They are `persist` items now:
    `Activated` a SceneData item keyed by the arena's scene, the plop a
    PlayerData bit. The reserve is the cooked bank's alone.
    """
    def test_the_reserve_is_the_cooked_banks_and_the_boss_is_not_in_it(self):
        slots = rustsrc.const_int(ROOT / 'game/src/save.rs', 'SCRIPT_FIELD_SLOTS')
        script = rustsrc.source(ROOT / 'game/src/script.rs')
        self.assertEqual(re.findall(r'pub const FIELD_\w+', script), [])
        enemies = rustsrc.source(ROOT / 'game/src/enemies.rs')
        self.assertIn('persist::Kind::BattleScene', enemies)
        self.assertIn('persist::FALSE_KNIGHT_FIRST_PLOP', enemies)
        self.assertNotIn('script::player_data', enemies)
        table = ROOT / 'data/scripts.rs'
        if not table.is_file():
            self.skipTest('run host/cook_scripts.py first')
        names = re.search(r'pub static SCRIPT_FIELD_NAMES:&\[&str\]=&\[([^\]]*)\];',
                          table.read_text()).group(1)
        cooked = len([n for n in names.split(',') if n.strip()])
        self.assertLessEqual(cooked, slots, 'the cooked script bank has outgrown the reserve')

    def test_the_whole_reserve_reaches_the_record(self):
        """`record`/`boot` must carry every slot, not just the cooked ones."""
        script = ROOT / 'game/src/script.rs'
        self.assertTrue(rustsrc.contains(script, 'static mut STORE: [i32; crate::save::SCRIPT_FIELD_SLOTS]'))
        self.assertTrue(rustsrc.contains(script, 'STORE = *values;'))


if __name__ == '__main__':
    unittest.main()
