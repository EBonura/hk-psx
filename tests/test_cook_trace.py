"""host/cook_trace.py: a region stays reusable through edits to code its cook never ran, and
only those.

Each case cooks a toy module in a fresh interpreter (tracing has to start before the module is
imported), edits it, and asks whether the recorded trace is still fresh.
"""
import json, subprocess, sys, tempfile, textwrap, unittest
from pathlib import Path

HOST = Path(__file__).resolve().parents[1] / 'host'

TOY = '''\
import json

def kind(x):
    if x > 0:
        return 'positive'   # the branch the cook takes
    else:
        return 'other'

def unused():
    return 1

def quiet(x):
    if x < 0:
        pass
    return x

def reads_self():
    return len(open(__file__).read())
'''

CHILD = '''
import sys, json
sys.path.insert(0, {host!r})
import cook_trace
from pathlib import Path
cook_trace.ROOT = Path({root!r})
cook_trace.start([])
sys.path.insert(0, {root!r})
import toy
toy.kind(1); toy.quiet(1)
if {self_read}: toy.reads_self()
print(json.dumps(cook_trace.snapshot()))
'''


def cook(root, self_read=False):
    code = CHILD.format(host=str(HOST), root=str(root), self_read=self_read)
    out = subprocess.run([sys.executable, '-c', code], capture_output=True, text=True, check=True)
    return json.loads(out.stdout)


def stale(root, trace):
    code = (f'import sys,json;sys.path.insert(0,{str(HOST)!r});import cook_trace;from pathlib import Path;'
            f'cook_trace.ROOT=Path({str(root)!r});print(json.dumps(cook_trace.stale(json.loads(sys.stdin.read()))))')
    out = subprocess.run([sys.executable, '-c', code], input=json.dumps(trace), capture_output=True,
                         text=True, check=True)
    return json.loads(out.stdout)


class CookTraceTests(unittest.TestCase):
    def check(self, edit, fresh, self_read=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / 'toy.py').write_text(TOY)
            trace = cook(root, self_read)
            self.assertIsNotNone(trace)
            self.assertIn('toy.py', trace['files'])
            self.assertIsNone(stale(root, trace), 'unchanged code must stay fresh')
            (root / 'toy.py').write_text(edit(TOY))
            reason = stale(root, trace)
            if fresh:
                self.assertIsNone(reason)
            else:
                self.assertIsNotNone(reason)

    def test_edit_in_a_branch_the_cook_never_took_keeps_it(self):
        self.check(lambda s: s.replace("return 'other'", "return 'changed'"), fresh=True)

    def test_edit_in_a_function_the_cook_never_called_keeps_it(self):
        self.check(lambda s: s.replace('return 1', 'return 2'), fresh=True)

    def test_comment_and_blank_lines_keep_it(self):
        self.check(lambda s: s.replace('# the branch the cook takes', '# reworded').replace('def unused', '\n\ndef unused'),
                   fresh=True)

    def test_new_function_recooks_because_its_def_runs_at_import(self):
        # A new top-level def binds a name when the module loads, and code that looks names up
        # dynamically (globals().get, getattr on the module) can see it: not provably inert.
        self.check(lambda s: s + '\ndef added():\n    return 3\n', fresh=False)

    def test_edit_in_the_branch_the_cook_took_recooks(self):
        self.check(lambda s: s.replace("return 'positive'", "return 'plus'"), fresh=False)

    def test_changed_condition_recooks(self):
        self.check(lambda s: s.replace('if x > 0:', 'if x > 1:'), fresh=False)

    def test_statement_added_on_the_taken_path_recooks(self):
        self.check(lambda s: s.replace("    if x > 0:", "    x = x - 1\n    if x > 0:"), fresh=False)

    def test_module_level_edit_recooks(self):
        self.check(lambda s: s.replace('import json', 'import json\nLIMIT = 3'), fresh=False)

    def test_silent_block_counts_as_run(self):
        # `pass` reports no line of its own, so the untaken branch it fills is kept in the hash.
        self.check(lambda s: s.replace('        pass', '        x = 0'), fresh=False)

    def test_new_default_argument_recooks(self):
        self.check(lambda s: s.replace('def unused():', 'def unused(y=print):'), fresh=False)

    def test_file_that_reads_its_own_bytes_recooks_on_any_change(self):
        self.check(lambda s: s + '# trailing comment\n', fresh=False, self_read=True)

    def test_module_that_would_shadow_a_loaded_one_recooks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / 'toy.py').write_text(TOY)
            (root / 'host').mkdir()
            trace = cook(root)
            self.assertIn('json', trace['shadow'])
            (root / 'host' / 'json.py').write_text('')
            self.assertIsNotNone(stale(root, trace))

    def test_tracing_after_a_repo_import_refuses_to_record(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / 'toy.py').write_text(TOY)
            code = (f'import sys,json;sys.path.insert(0,{str(HOST)!r});import cook_trace;from pathlib import Path;'
                    f'cook_trace.ROOT=Path({str(root)!r});sys.path.insert(0,{str(root)!r});import toy;'
                    f'cook_trace.start([]);toy.kind(1);print(json.dumps(cook_trace.snapshot()))')
            out = subprocess.run([sys.executable, '-c', code], capture_output=True, text=True, check=True)
            self.assertIsNone(json.loads(out.stdout))


if __name__ == '__main__':
    unittest.main()
