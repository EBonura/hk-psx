"""Dependency tracking for cache units: what code and which files a unit really used.

A cache unit (one region, one scene, one asset script) is only reusable while every
input that decided its output is unchanged. Listing those inputs by hand is how
host/cook_inputs.txt came to miss false_knight.py. This module records them instead:
while a unit runs, it notes every Python function that started and every repository
file that was opened for reading. The record is the unit's dependency set; the unit is
fresh while each recorded function still has the same bytecode and each recorded file
the same bytes. A function body the unit never executed cannot change its output, so
editing one invalidates nothing.

What stands in for what a function cannot show:

  * Module level code (constants, tables, imports, decorators, class bodies) is hashed
    as the module's "skeleton": the module's code with every function body removed.
    Every host module the process had loaded when the unit ended contributes its
    skeleton, so editing a table at module level invalidates every unit that could
    have read it, and editing a function body invalidates only units that ran it.
  * Libraries (UnityPy, PIL, numpy) are pinned by host/requirements.lock and the
    interpreter version, which are part of every unit's stamp.
  * The retail source is one input of every unit through the source manifest the build
    driver already hashes; it is passed in as `base`.
  * Files read through open() are recorded with the hash they had when opened.

Unit identity (region spec, scene id, script name) and `base` are the caller's.
"""
from __future__ import annotations

import hashlib
import json
import os
import sys
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
# Code under these trees is tracked. Everything else is a pinned library or the
# interpreter (see `stamp`).
CODE_DIRS = (ROOT / 'host', ROOT / 'tools')
# Files read from these places are never dependencies: build products, caches, the
# virtual environment and version control.
SKIP_FILE_DIRS = tuple(str(ROOT / name) + os.sep for name in
                       ('.venv', '.git', 'host/target', 'target', '.hkpsx/stagecache', '.hkpsx/precook-logs',
                        'build', 'dist'))
CO_FUNCTION = 0x1 | 0x2  # CO_OPTIMIZED | CO_NEWLOCALS: set for functions, clear for module and class bodies

_MON = sys.monitoring
TOOL = _MON.COVERAGE_ID


def _canon(value) -> object:
    """A repr-stable form of a code constant."""
    if isinstance(value, frozenset):
        return ('frozenset', sorted(map(repr, value)))
    if isinstance(value, tuple):
        return tuple(_canon(v) for v in value)
    if hasattr(value, 'co_code'):
        return ('code', value.co_qualname)
    return repr(value)


def _digest(code) -> str:
    """Digest of one code object's behaviour: bytecode, constants and names, not line numbers.

    A skeleton digest of a module or class body replaces each nested function code object
    by its qualified name, so the function's body is not part of the container's identity.
    """
    h = hashlib.sha256()
    h.update(repr((code.co_qualname, code.co_argcount, code.co_posonlyargcount, code.co_kwonlyargcount,
                   code.co_flags, code.co_names, code.co_varnames, code.co_freevars, code.co_cellvars)).encode())
    h.update(code.co_code)
    h.update(code.co_exceptiontable)
    for const in code.co_consts:
        h.update(repr(_canon(const)).encode())
    return h.hexdigest()


class FileCode:
    """The digests of every code object in one source file, keyed by (qualname, occurrence)."""

    def __init__(self, path: Path):
        source = path.read_bytes()
        self.sha = hashlib.sha256(source).hexdigest()
        self.functions: dict[tuple[str, int], str] = {}
        # (qualname, first line) of a code object as the interpreter loads it -> its occurrence index.
        self.index_of: dict[tuple[str, int], int] = {}
        parts: list[str] = []
        seen: dict[str, int] = {}

        def walk(code):
            qual = code.co_qualname
            index = seen.get(qual, 0)
            seen[qual] = index + 1
            self.index_of[(qual, code.co_firstlineno)] = index
            digest = _digest(code)
            if code.co_flags & CO_FUNCTION:
                self.functions[(qual, index)] = digest
            else:
                parts.append(f'{qual}#{index}:{digest}')
            for const in code.co_consts:
                if hasattr(const, 'co_code'):
                    walk(const)

        walk(compile(source, str(path), 'exec', dont_inherit=True))
        self.skeleton = hashlib.sha256('\n'.join(parts).encode()).hexdigest()


_FILE_CODE: dict[str, FileCode] = {}


def file_code(rel: str) -> FileCode:
    """Parsed digests of `rel` (repo relative), reparsed when its bytes changed."""
    path = ROOT / rel
    cached = _FILE_CODE.get(rel)
    stat = path.stat()
    key = (stat.st_mtime_ns, stat.st_size)
    if cached is not None and getattr(cached, 'stat_key', None) == key:
        return cached
    fresh = FileCode(path)
    fresh.stat_key = key
    _FILE_CODE[rel] = fresh
    return fresh


def sha256_file(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def _in_code_dirs(filename: str) -> str | None:
    for base in CODE_DIRS:
        prefix = str(base) + os.sep
        if filename.startswith(prefix) and filename.endswith('.py'):
            return filename[len(str(ROOT)) + 1:]
    return None


def stamp() -> str:
    """What every unit depends on besides its own code: interpreter, pinned libraries, platform."""
    lock = ROOT / 'host/requirements.lock'
    return hashlib.sha256(
        f'{sys.version}|{sys.platform}|{sha256_file(lock) if lock.exists() else ""}'.encode()).hexdigest()


class Tracker:
    """Records the functions executed and files read inside a `with` block."""

    def __init__(self, outputs=(), skip_skeleton=(), skip_file=None):
        self.skip_skeleton = set(skip_skeleton)
        self.skip_file = skip_file
        self.codes: set = set()
        self.files: dict[str, str] = {}
        self.written: set[str] = set()
        self._outputs = {str(Path(p).resolve()) for p in outputs}
        self._active = False
        self._lock = threading.Lock()

    # -- recording -----------------------------------------------------------------
    def _on_start(self, code, offset):
        self.codes.add(code)
        return _MON.DISABLE

    def _on_audit(self, event, args):
        if not self._active or event != 'open':
            return
        path, mode, flags = args
        if not isinstance(path, (str, os.PathLike)):
            return
        try:
            full = os.path.abspath(os.fspath(path))
        except (TypeError, ValueError):
            return
        writing = (isinstance(mode, str) and any(c in mode for c in 'wa+x')) or (
            isinstance(flags, int) and flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND))
        if writing:
            self.written.add(full)
            return
        if not full.startswith(str(ROOT) + os.sep) or full.startswith(SKIP_FILE_DIRS):
            return
        if full in self.written or full in self._outputs or full in self.files:
            return
        if self.skip_file is not None and self.skip_file(full):
            return
        try:
            self.files[full] = sha256_file(Path(full))
        except OSError:
            return

    def __enter__(self):
        _MON.use_tool_id(TOOL, 'hk-depcache')
        _MON.register_callback(TOOL, _MON.events.PY_START, self._on_start)
        _MON.restart_events()
        _MON.set_events(TOOL, _MON.events.PY_START)
        sys.addaudithook(self._on_audit)
        self._active = True
        return self

    def __exit__(self, *exc):
        self._active = False
        _MON.set_events(TOOL, 0)
        _MON.register_callback(TOOL, _MON.events.PY_START, None)
        _MON.free_tool_id(TOOL)
        return False

    # -- result ---------------------------------------------------------------------
    def record(self) -> dict:
        """The dependency record: function digests, module skeletons, input files."""
        functions: dict[str, str] = {}
        for code in self.codes:
            rel = _in_code_dirs(code.co_filename)
            if rel is None or not code.co_flags & CO_FUNCTION:
                continue
            fc = file_code(rel)
            index = fc.index_of.get((code.co_qualname, code.co_firstlineno))
            digest = fc.functions.get((code.co_qualname, index)) if index is not None else None
            functions[f'{rel}::{code.co_qualname}#{index}'] = digest or '?'
        skeletons: dict[str, str] = {}
        for module in list(sys.modules.values()):
            filename = getattr(module, '__file__', None)
            rel = _in_code_dirs(filename) if filename else None
            if rel is not None and rel not in self.skip_skeleton:
                skeletons[rel] = file_code(rel).skeleton
        # A file the unit read and also rewrote is judged by the state it left: the pre-state it read
        # is gone, and a rerun over the new state must find the unit fresh.
        rel_files = {}
        for path, digest in self.files.items():
            if path in self.written and os.path.isfile(path):
                digest = sha256_file(Path(path))
            rel_files[os.path.relpath(path, ROOT)] = digest
        return {'stamp': stamp(), 'functions': dict(sorted(functions.items())),
                'skeletons': dict(sorted(skeletons.items())), 'files': dict(sorted(rel_files.items()))}


def stale(record: dict) -> list[str]:
    """Reasons `record` no longer describes the code and files now on disk (empty means fresh)."""
    reasons = []
    if record['stamp'] != stamp():
        return ['interpreter or pinned libraries changed']
    for rel, digest in record['skeletons'].items():
        if not (ROOT / rel).exists() or file_code(rel).skeleton != digest:
            reasons.append(f'module level of {rel}')
    for key, digest in record['functions'].items():
        rel, rest = key.split('::', 1)
        qual, index = rest.rsplit('#', 1)
        if not (ROOT / rel).exists() or file_code(rel).functions.get((qual, int(index))) != digest:
            reasons.append(f'function {rel}:{qual}')
    for rel, digest in record['files'].items():
        path = ROOT / rel
        if not path.is_file() or sha256_file(path) != digest:
            reasons.append(f'file {rel}')
    return reasons


def merge(*records: dict) -> dict:
    """The union of several dependency records (the dependencies of a stage made of units)."""
    out = {'stamp': stamp(), 'functions': {}, 'skeletons': {}, 'files': {}}
    for record in records:
        for part in ('functions', 'skeletons', 'files'):
            out[part].update(record[part])
    for part in ('functions', 'skeletons', 'files'):
        out[part] = dict(sorted(out[part].items()))
    return out


def digest(record: dict) -> str:
    return hashlib.sha256(json.dumps(record, sort_keys=True).encode()).hexdigest()
