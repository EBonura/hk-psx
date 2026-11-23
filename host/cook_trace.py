"""Which cooker code a region's cook ran, kept as a reuse key that edits elsewhere leave alone.

host/regions.py used to key every region on the whole of every file above the
postpass divider in host/cook_inputs.txt, so a seven-line change to one hazard
rule recooked all 1,078 regions. This module records, per cook process, every
line of repo Python that ran (sys.monitoring LINE events, each location
reported once and then disabled, so the cost is one callback per distinct line).
A region's record is, for every repo file that ran, the file's syntax tree with
each statement block that never ran replaced by a placeholder, hashed, plus the
shape of those choices (the mask). A later cook reuses the region when every
recorded file still hashes the same under its recorded mask.

Why that is enough. The cook is deterministic in its inputs, which the key holds
separately (retail source, region spec, the non-Python inputs, the budget
constants). Old and new code agree on every statement the old run executed, and
on every statement that decides whether a block runs, so the new run takes the
same path, never enters a block whose text changed, and writes the same bytes.

Where it is careful:
- The line set is cumulative for the process, never reset between regions: a
  region can reuse what an earlier region of the same worker computed (scene
  caches, lru_cache, the shared pixel and geometry dicts), so the code that
  computed it has to be in the region's record too.
- A block counts as not entered only when it holds a statement that always
  reports a line when it runs; a block of only `pass`, docstrings and
  global/nonlocal is kept in the hash whether it ran or not.
- A statement's span includes its decorators; a header that shares a line with
  its block makes the block count as entered (kept, never elided).
- Any repo source file the cook opens and reads (a module hashing its own
  bytes into a report, a table beside a module) is recorded byte for byte, from
  an audit hook on `open`; imports are told apart by their importlib frames.
- A file that changed on disk after it first ran in this process makes the
  record refuse (no record, so the region recooks next time).
- A new host/ or tools/ module named like a module the cook loaded from
  elsewhere (stdlib, site-packages) would shadow it, so the record lists those
  names and a later check refuses if such a file appears.
- Tracing starts before any repo module is imported, or the record refuses.
Comments and formatting are not in the hash; docstrings are.
"""
import ast
import base64
import bisect
import hashlib
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEME = 'lines-1'
TOOL = sys.monitoring.COVERAGE_ID

_lines = {}       # resolved path -> set of executed line numbers
_first_sha = {}   # resolved path -> sha256 of its bytes when it first ran
_resolved = {}    # co_filename -> resolved path, or None for code outside the repo
_read = {}        # resolved repo source files (to sha256 at first open) the cook opened itself (not by importing them)
_state = {'active': False, 'refused': None, 'busy': False}
# Generated output and caches: never source. Reads from these are the cook's own state.
_OUTPUT_DIRS = ('.hkpsx', 'data', 'build', 'dist', 'target', '.venv', '.git', '.psoxide', '.cargo')
_excluded = set()  # repo-relative paths, set by start()
_record_memo = {}  # (path, executed line count) -> per-file record
_tree_memo = {}    # (path, sha) -> (tree, source bytes)
_check_memo = {}   # (path, sha, mask) -> digest or None


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _tracked(path):
    """Repo Python, not the virtualenv or anything installed into the repo."""
    try:
        rel = path.relative_to(ROOT)
    except ValueError:
        return False
    return path.suffix == '.py' and not any(p in ('.venv', 'site-packages', 'target') for p in rel.parts)


def _resolve(name):
    try:
        return _resolved[name]
    except KeyError:
        pass
    path = None
    if not name.startswith('<'):
        try:
            candidate = Path(name).resolve()
            if _tracked(candidate) and candidate.is_file():
                path = str(candidate)
                _first_sha[path] = _sha(candidate.read_bytes())
                _lines[path] = set()
        except OSError:
            path = None
    _resolved[name] = path
    return path


def _on_line(code, line):
    # The callbacks run pathlib themselves; those lines are reported later, outside a callback.
    if _state['busy']:
        return None
    _state['busy'] = True
    try:
        path = _resolve(code.co_filename)
        if path is not None:
            _lines[path].add(line)
    finally:
        _state['busy'] = False
    return sys.monitoring.DISABLE


def start(excluded):
    """Begin tracing; called by host/regions.py, run as a script, before it imports any cooker
    module. `excluded` holds the repo-relative paths whose code a region's bytes do not depend
    on by declaration (the postpass section of host/cook_inputs.txt, and this file)."""
    if _state['active'] or _state['refused']:
        return
    _excluded.update(excluded)
    if Path(__file__).resolve().is_relative_to(ROOT):
        _excluded.add(str(Path(__file__).resolve().relative_to(ROOT)))
    early = sorted(name for name, module in list(sys.modules.items())
                   if name != '__main__' and getattr(module, '__file__', None)
                   and _tracked(Path(module.__file__).resolve())
                   and Path(module.__file__).resolve() != Path(__file__).resolve())
    if early:
        _state['refused'] = 'repo modules imported before tracing started: ' + ', '.join(early[:5])
        return
    try:
        sys.monitoring.use_tool_id(TOOL, 'hk-cook-trace')
    except ValueError:
        _state['refused'] = 'the coverage monitoring slot is taken'
        return
    sys.monitoring.register_callback(TOOL, sys.monitoring.events.LINE, _on_line)
    sys.monitoring.set_events(TOOL, sys.monitoring.events.LINE)
    sys.addaudithook(_on_audit)
    _state['active'] = True


def _on_audit(event, args):
    if event != 'open' or not _state['active'] or _state['busy']:
        return
    _state['busy'] = True
    try:
        _audit_open(args)
    finally:
        _state['busy'] = False


def _audit_open(args):
    name = args[0]
    if isinstance(name, int):
        return
    try:
        path = Path(name.decode() if isinstance(name, bytes) else name).resolve()
        rel = path.relative_to(ROOT)
    except (ValueError, OSError, TypeError, UnicodeDecodeError):
        return
    if not rel.parts or rel.parts[0] in _OUTPUT_DIRS:
        return
    # Who opened it: an import runs through importlib's frames before any repo frame does.
    frame = sys._getframe(1)
    while frame is not None:
        origin = frame.f_code.co_filename
        if origin.startswith('<frozen importlib'):
            return
        reader = _resolve(origin)
        if reader is not None:
            # A read by declared postpass code (or by this file) is not a region input.
            if str(Path(reader).relative_to(ROOT)) in _excluded:
                return
            if str(path) not in _read:
                try:
                    _read[str(path)] = _sha(path.read_bytes())
                except OSError:
                    _read[str(path)] = None
            return
        frame = frame.f_back


# --- the masked syntax tree ---

_SILENT = (ast.Pass, ast.Global, ast.Nonlocal)


def _reports(stmt):
    """Whether running this statement always reports at least one line inside its span."""
    if isinstance(stmt, _SILENT):
        return False
    if isinstance(stmt, ast.Expr) and isinstance(stmt.value, ast.Constant):
        return False
    if isinstance(stmt, (ast.Try, ast.TryStar)):
        # No header expression: only its body can report.
        return any(_reports(s) for s in stmt.body)
    return True


def _span(block):
    first = min(min([s.lineno] + [d.lineno for d in getattr(s, 'decorator_list', [])]) for s in block)
    return first, max(s.end_lineno for s in block)


def _is_block(value):
    return isinstance(value, list) and value and all(isinstance(v, ast.stmt) for v in value)


def _const(value):
    return f'{type(value).__name__}:{value!r}'


def _walk(node, decide, out):
    out.append(type(node).__name__)
    for name, value in ast.iter_fields(node):
        out.append(name)
        if _is_block(value):
            if decide(value):
                out.append('[')
                for stmt in value:
                    _walk(stmt, decide, out)
                out.append(']')
            else:
                out.append('[elided]')
        elif isinstance(value, list):
            out.append('(')
            for item in value:
                if isinstance(item, ast.AST):
                    _walk(item, decide, out)
                else:
                    out.append(_const(item))
            out.append(')')
        elif isinstance(value, ast.AST):
            _walk(value, decide, out)
        else:
            out.append(_const(value))


def _digest(tokens):
    return _sha('\x00'.join(tokens).encode())


def _pack(bits):
    return base64.b64encode(zlib.compress(bits.encode(), 9)).decode()


def _unpack(mask):
    return zlib.decompress(base64.b64decode(mask)).decode()


def _tree(path, sha, data=None):
    memo = _tree_memo.get((path, sha))
    if memo is None:
        data = data if data is not None else Path(path).read_bytes()
        memo = (ast.parse(data, filename=path), data)
        _tree_memo[(path, sha)] = memo
    return memo


def _record_file(path, lines):
    data = Path(path).read_bytes()
    sha = _sha(data)
    if sha != _first_sha[path]:
        raise ValueError(f'{path} changed on disk during the cook')
    tree, _ = _tree(path, sha, data)
    executed = sorted(lines)
    bits = []

    def decide(block):
        lo, hi = _span(block)
        at = bisect.bisect_left(executed, lo)
        entered = at < len(executed) and executed[at] <= hi
        keep = entered or not any(_reports(s) for s in block)
        bits.append('1' if keep else '0')
        return keep

    tokens = []
    _walk(tree, decide, tokens)
    return {'sha': sha, 'mask': _pack(''.join(bits)), 'digest': _digest(tokens)}


def snapshot():
    """The record for a region whose cook just finished in this process, or None when this
    process cannot vouch for one (tracing off, refused, or a file changed under it)."""
    if not _state['active'] or _state['refused']:
        return None
    files = {}
    try:
        for path in sorted(_lines):
            rel = str(Path(path).relative_to(ROOT))
            if rel in _excluded or path in _read:
                continue
            lines = _lines[path]
            memo = _record_memo.get((path, len(lines)))
            if memo is None:
                memo = _record_file(path, lines)
                _record_memo[(path, len(lines))] = memo
            files[rel] = memo
        for path in sorted(_read):
            rel = str(Path(path).relative_to(ROOT))
            if rel in _excluded:
                continue
            # Its bytes may be in the output: any change at all counts.
            sha = _sha(Path(path).read_bytes())
            if sha != _read[path] or (path in _first_sha and sha != _first_sha[path]):
                raise ValueError(f'{path} changed on disk during the cook')
            files[rel] = {'sha': sha, 'mask': None, 'digest': sha}
    except (OSError, ValueError, SyntaxError):
        return None
    shadow = set()
    for name, module in list(sys.modules.items()):
        top = name.partition('.')[0]
        origin = getattr(module, '__file__', None)
        if origin is None or not _tracked(Path(origin).resolve()):
            shadow.add(top)
    return {'scheme': SCHEME, 'files': files, 'shadow': sorted(shadow)}


def _shadowed(names):
    for folder in (ROOT / 'host', ROOT / 'tools'):
        for name in names:
            if (folder / f'{name}.py').is_file() or (folder / name / '__init__.py').is_file():
                return f'{folder.name}/{name} would shadow the module the cook loaded'
    return None


def stale(trace):
    """None when the code a recorded region ran is unchanged in effect, else the reason."""
    if not isinstance(trace, dict) or trace.get('scheme') != SCHEME:
        return 'no code trace of this scheme'
    reason = _shadowed(trace.get('shadow', []))
    if reason:
        return reason
    for rel, record in trace['files'].items():
        path = ROOT / rel
        try:
            data = path.read_bytes()
        except OSError:
            return f'{rel} is gone'
        sha = _sha(data)
        if sha == record['sha']:
            continue
        if record['mask'] is None:
            return f'{rel} changed'
        key = (str(path), sha, record['mask'])
        if key not in _check_memo:
            _check_memo[key] = _masked_digest(str(path), sha, data, record['mask'])
        if _check_memo[key] != record['digest']:
            return f'{rel} changed in code this region ran'
    return None


def _masked_digest(path, sha, data, mask):
    try:
        tree, _ = _tree(path, sha, data)
    except SyntaxError:
        return None
    bits = iter(_unpack(mask))
    short = []

    def decide(block):
        bit = next(bits, None)
        if bit is None:
            short.append(True)
            return True
        return bit == '1'

    tokens = []
    _walk(tree, decide, tokens)
    if short or next(bits, None) is not None:
        return None
    return _digest(tokens)
