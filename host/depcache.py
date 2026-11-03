"""Dependency tracking for cache units: what code and which files a unit really used.

A cache unit (the scenery of the regions of one scene, a postpass, an asset script) is only
reusable while every input that decided its output is unchanged. Listing those inputs by hand is
how host/cook_inputs.txt came to miss false_knight.py. This module records them instead: while a
unit runs, it notes every Python function that started, every file that was opened for reading,
every directory that was listed and every program that was run. The record is the unit's
dependency set; the unit is fresh while each recorded function still has the same bytecode and
each recorded file the same bytes. A function body the unit never executed cannot change its
output, so editing one invalidates nothing.

What stands in for what a function cannot show:

  * Module level code (constants, tables, imports, decorators, class bodies) is hashed as the
    module's "skeleton": the module's code with every function body removed. Every host module
    the process had loaded when the unit ended contributes its skeleton, so editing a table at
    module level invalidates every unit that could have read it, and editing a function body
    invalidates only the units that ran it.
  * Functions that module level code runs while the module imports are recorded too, because the
    Tracker is armed before the unit's modules are imported (`Tracker` must be entered first).
  * The record is cumulative over the process: a result one unit computed and a later one reads
    from an in-process cache still shows the code that computed it. The price is precision, so a
    process should run units of one kind (all the scenery of a scene, say), never two kinds whose
    code must not be mixed.
  * Libraries (UnityPy, PIL, numpy) are pinned by host/requirements.lock and the interpreter
    version, which are part of `stamp`. So is every HK_ environment variable that is not a path.
  * The retail source files a unit opened are recorded by name with the hash the build driver's
    checked manifest holds for them.
  * Anything this cannot see (os.system, an exec, a native module outside the pinned libraries)
    makes the record "opaque": it is never fresh.

Unit identity (region spec, scene id, script name) is the caller's, and goes in the receipt next
to the record. A record is stored once under .hkpsx/cookdeps/<digest>.json and receipts name it.
"""
from __future__ import annotations

import hashlib
import json
import os
import sys
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
# Bump when this file changes how a record is made or judged: every earlier record becomes stale.
VERSION = 5
# Code under these trees is tracked. Everything else is a pinned library or the interpreter.
CODE_DIRS = (ROOT / 'host', ROOT / 'tools')
# Files read from these places are never dependencies: build products, caches, the virtual
# environment and version control.
SKIP_FILE_DIRS = tuple(str(ROOT / name) + os.sep for name in
                       ('.venv', '.git', 'host/target', 'target', '.hkpsx/cookdeps', '.hkpsx/stagecache',
                        '.hkpsx/precook-logs', 'build', 'dist', '.psoxide', '.cargo'))
# Single files that are lane state or caches that are a pure function of something already recorded.
SKIP_FILES = tuple(str(ROOT / name) for name in
                   ('.hkpsx/doctor.json', '.hkpsx/typetree-cache.json', '.hkpsx/typetree-cache.json.lock',
                    '.hkpsx/alpha-covers-cache.json'))
# HK_ variables that name a place or a tool, not a choice the cooked bytes depend on.
PATH_ENV = {'HK_COOK_STORE', 'HK_PSX_FRONTEND', 'HK_PSX_FRONTEND_SHA256', 'HK_OVERLAY_SDK',
            'HK_SOURCE_FINGERPRINT_CHECKED', 'HK_REGION_CACHE_HELPER', 'HK_DISC_LIBRARY', 'HK_FORCE_COOK'}
# The judging machinery is not a dependency of what it judges: a fix to it bumps VERSION when it must void records.
UNTRACKED = ('host/depcache.py', 'host/stagecache.py')
CO_FUNCTION = 0x1 | 0x2  # CO_OPTIMIZED | CO_NEWLOCALS: set for functions, clear for module and class bodies
DEPS_DIR = ROOT / '.hkpsx/cookdeps'

_MON = sys.monitoring
TOOL = _MON.COVERAGE_ID


def force_cook() -> bool:
    """True when the caller asked for every receipt to be ignored and every unit cooked again."""
    return bool(os.environ.get('HK_FORCE_COOK'))


def _canon(value) -> object:
    """A repr-stable form of a code constant."""
    if isinstance(value, frozenset):
        return ('frozenset', sorted(map(repr, value)))
    if isinstance(value, tuple):
        return tuple(_canon(v) for v in value)
    if hasattr(value, 'co_code'):
        return ('code', value.co_qualname)
    return repr(value)


def digest_code(code) -> str:
    """Digest of one code object's behaviour: bytecode, constants and names, not line numbers.

    A skeleton digest of a module or class body replaces each nested function code object by its
    qualified name, so the function's body is not part of the container's identity.
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
        # (qualname, first line) of a code object as the interpreter loads it -> its occurrence indices (two
        # generator expressions on one line share both).
        self.index_of: dict[tuple[str, int], list[int]] = {}
        parts: list[str] = []
        seen: dict[str, int] = {}

        def walk(code):
            qual = code.co_qualname
            index = seen.get(qual, 0)
            seen[qual] = index + 1
            self.index_of.setdefault((qual, code.co_firstlineno), []).append(index)
            digest = digest_code(code)
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
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def code_rel(filename: str) -> str | None:
    """Repo-relative path of a tracked source file, or None."""
    for base in CODE_DIRS:
        prefix = str(base) + os.sep
        if filename.startswith(prefix) and filename.endswith('.py'):
            return filename[len(str(ROOT)) + 1:]
    return None


def stamp() -> str:
    """What every unit depends on besides its own code: interpreter, pinned libraries, switches."""
    lock = ROOT / 'host/requirements.lock'
    env = sorted((k, v) for k, v in os.environ.items() if k.startswith('HK_') and k not in PATH_ENV)
    return hashlib.sha256(
        f'{VERSION}|{sys.version}|{sys.platform}|{sha256_file(lock) if lock.exists() else ""}|{env}'.encode()
    ).hexdigest()


def listing_digest(path: str) -> str | None:
    try:
        return hashlib.sha256('\n'.join(sorted(os.listdir(path))).encode()).hexdigest()
    except OSError:
        return None


class Retail:
    """The retail source files, by the hashes of the build driver's checked manifest."""

    def __init__(self, directory: str, shas: dict[str, str]):
        self.directory = os.path.realpath(directory)
        self.prefix = self.directory + os.sep
        self.shas = shas

    @classmethod
    def checked(cls) -> 'Retail | None':
        """The manifest the build driver hashed this run, or None when it did not (no reuse)."""
        manifest = ROOT / '.hkpsx/regions-source-inputs.json'
        checked = os.environ.get('HK_SOURCE_FINGERPRINT_CHECKED')
        if not checked or not manifest.is_file() or sha256_file(manifest) != checked:
            return None
        value = json.loads(manifest.read_text())
        return cls(value['source'], value['inputs'])

    def name_of(self, full: str) -> str | None:
        real = os.path.realpath(full)
        return real[len(self.prefix):] if real.startswith(self.prefix) else None

    def sha(self, name: str) -> str | None:
        known = self.shas.get(name)
        if known is not None:
            return known
        path = Path(self.directory) / name
        return sha256_file(path) if path.is_file() else None


class Tracker:
    """Records the functions executed, files read, directories listed and programs run in a `with`.

    Enter it before importing the modules the unit uses, and call the unit's entry point inside it.
    """

    def __init__(self, retail: Retail | None = None, skip_skeleton=(), skip_file=None, extra_skeletons=()):
        self.retail = retail
        self.skip_skeleton = set(skip_skeleton)
        self.skip_file = skip_file
        self.extra_skeletons = list(extra_skeletons)
        self.codes: set = set()
        self.files: dict[str, str] = {}
        self.retail_names: set[str] = set()
        self.dirs: set[str] = set()
        self.programs: set[str] = set()
        self.opaque: list[str] = []
        self.written: set[str] = set()
        self._active = False
        self._busy = False
        self._lock = threading.Lock()

    # -- recording -----------------------------------------------------------------
    def _on_start(self, code, offset):
        self.codes.add(code)
        return _MON.DISABLE

    def _note_read(self, full: str):
        if self.retail is not None:
            name = self.retail.name_of(full)
            if name is not None:
                self.retail_names.add(name)
                return
        if not full.startswith(str(ROOT) + os.sep) or full.startswith(SKIP_FILE_DIRS) or full in SKIP_FILES:
            return
        # Module sources are the function and skeleton digests' business, not whole-file dependencies.
        if full.endswith(('.py', '.pyc')) and (code_rel(full) is not None or os.sep + '__pycache__' + os.sep in full):
            return
        if full in self.written or full in self.files:
            return
        if self.skip_file is not None and self.skip_file(full):
            return
        try:
            self.files[full] = sha256_file(Path(full))
        except OSError:
            return

    def _on_audit(self, event, args):
        if not self._active or self._busy:
            return
        self._busy = True
        try:
            self._audit(event, args)
        finally:
            self._busy = False

    def _audit(self, event, args):
        if event == 'open':
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
            else:
                self._note_read(full)
        elif event in ('os.rename', 'shutil.copyfile', 'shutil.copymode', 'shutil.copystat', 'shutil.move'):
            # A file put in place by a rename or copy is written by this process (the atomic-write idiom).
            target = args[1] if len(args) > 1 else None
            if isinstance(target, (str, os.PathLike)):
                self.written.add(os.path.abspath(os.fspath(target)))
        elif event in ('os.remove', 'os.unlink', 'os.truncate'):
            target = args[0] if args else None
            if isinstance(target, (str, os.PathLike)):
                self.written.add(os.path.abspath(os.fspath(target)))
        elif event in ('os.listdir', 'os.scandir'):
            path = args[0] if args else None
            if path is None:
                path = '.'
            if isinstance(path, (str, os.PathLike)):
                self.dirs.add(os.path.abspath(os.fspath(path)))
        elif event in ('glob.glob', 'glob.glob/2'):
            # The pattern's directory part is listed one level at a time by os.scandir/listdir, which is
            # audited; nothing more to note here.
            return
        elif event == 'subprocess.Popen':
            executable = args[0]
            if isinstance(executable, (str, os.PathLike)):
                self.programs.add(os.path.realpath(os.fspath(executable)))
            else:
                self.opaque.append('subprocess without an executable path')
        elif event in ('os.system', 'os.exec', 'os.posix_spawn', 'os.spawn', 'os.fork', 'os.forkpty'):
            self.opaque.append(event)
        elif event == 'socket.connect':
            self.opaque.append(event)

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
        """The dependency record: function digests, module skeletons, input files and the rest."""
        functions: dict[str, str] = {}
        for code in list(self.codes):
            rel = code_rel(code.co_filename)
            if rel is None or rel in UNTRACKED or not code.co_flags & CO_FUNCTION:
                continue
            fc = file_code(rel)
            live = digest_code(code)
            indices = fc.index_of.get((code.co_qualname, code.co_firstlineno), [])
            if not indices:
                functions[f'{rel}::{code.co_qualname}#?'] = '?'
                continue
            # Code objects that share a name and a line cannot be told apart from the running one: all of them
            # are recorded. The file on disk must still hold the code that ran, or the entry vouches for the
            # wrong text and is marked unusable.
            matched = any(fc.functions.get((code.co_qualname, i)) == live for i in indices)
            for i in indices:
                parsed = fc.functions.get((code.co_qualname, i))
                functions[f'{rel}::{code.co_qualname}#{i}'] = parsed if matched and parsed is not None else '?'
        skeletons: dict[str, str] = {}
        files: dict[str, str] = {}
        root_prefix = str(ROOT) + os.sep
        for module in list(sys.modules.values()):
            filename = getattr(module, '__file__', None)
            if not filename or not filename.startswith(root_prefix):
                continue
            rel = code_rel(filename)
            if rel is not None:
                if rel not in self.skip_skeleton and rel not in UNTRACKED:
                    skeletons[rel] = file_code(rel).skeleton
            elif filename.endswith('.py') and not filename.startswith(SKIP_FILE_DIRS):
                files[os.path.relpath(filename, ROOT)] = sha256_file(Path(filename))
        for rel in self.extra_skeletons:
            if rel not in UNTRACKED:
                skeletons[rel] = file_code(rel).skeleton
        # A file the unit also wrote is one of its outputs, not an input: the unit makes it from other things
        # (and a later step may rewrite it again, as the Rust scene cookers do the chunk files). Its bytes are
        # vouched for by whoever records the stage's outputs. A unit must not update an input in place.
        for path, digest in self.files.items():
            if path not in self.written:
                files[os.path.relpath(path, ROOT)] = digest
        retail = {}
        if self.retail is not None:
            for name in sorted(self.retail_names):
                retail[name] = self.retail.sha(name)
        # Directories the unit listed, unless it also put files in them (its own output directory).
        written_dirs = {os.path.dirname(p) for p in self.written}
        dirs = {}
        code_dirs = {str(base) for base in CODE_DIRS} | {str(ROOT)}
        for path in sorted(self.dirs):
            # The import system lists the code directories (and the checkout) to find modules; a module's
            # presence is judged by its skeleton, not by who else lives in the directory.
            if (path in written_dirs or path.startswith(SKIP_FILE_DIRS) or path + os.sep in SKIP_FILE_DIRS
                    or path in code_dirs or os.path.basename(path) == '__pycache__'):
                continue
            if self.retail is not None and self.retail.name_of(path + os.sep + 'x') is not None:
                dirs[path] = listing_digest(path)
            elif path.startswith(root_prefix):
                dirs[os.path.relpath(path, ROOT)] = listing_digest(path)
        programs = {}
        for path in sorted(self.programs):
            if os.path.isfile(path):
                key = os.path.relpath(path, ROOT) if path.startswith(root_prefix) else path
                programs[key] = sha256_file(Path(path))
        return {'stamp': stamp(), 'functions': dict(sorted(functions.items())),
                'skeletons': dict(sorted(skeletons.items())), 'files': dict(sorted(files.items())),
                'retail': retail, 'dirs': dirs, 'programs': programs, 'opaque': sorted(set(self.opaque))}


def _abs(rel: str) -> Path:
    return Path(rel) if os.path.isabs(rel) else ROOT / rel


def stale(record: dict, retail: Retail | None) -> list[str]:
    """Reasons `record` no longer describes the code and files now on disk (empty means fresh)."""
    if record.get('opaque'):
        return ['the unit ran something the tracker cannot see: ' + ', '.join(record['opaque'])]
    if record['stamp'] != stamp():
        return ['interpreter, pinned libraries or an HK_ switch changed']
    reasons = []
    for rel, digest in record['skeletons'].items():
        if not (ROOT / rel).exists() or file_code(rel).skeleton != digest:
            reasons.append(f'module level of {rel}')
    for key, digest in record['functions'].items():
        rel, rest = key.split('::', 1)
        qual, index = rest.rsplit('#', 1)
        if digest == '?' or not (ROOT / rel).exists() or file_code(rel).functions.get((qual, int(index))) != digest:
            reasons.append(f'function {rel}:{qual}')
    for rel, digest in record['files'].items():
        path = ROOT / rel
        if not path.is_file() or sha256_file(path) != digest:
            reasons.append(f'file {rel}')
    for rel, digest in record['programs'].items():
        path = _abs(rel)
        if not path.is_file() or sha256_file(path) != digest:
            reasons.append(f'program {rel}')
    for rel, digest in record['dirs'].items():
        if listing_digest(str(_abs(rel))) != digest:
            reasons.append(f'directory {rel}')
    if record['retail']:
        if retail is None:
            reasons.append('retail source not checked by the build driver')
        else:
            for name, digest in record['retail'].items():
                if retail.sha(name) != digest:
                    reasons.append(f'retail file {name}')
    return reasons


def merge(*records: dict) -> dict:
    """The union of several dependency records (the dependencies of a stage made of units)."""
    out = {'stamp': stamp(), 'functions': {}, 'skeletons': {}, 'files': {}, 'retail': {}, 'dirs': {},
           'programs': {}, 'opaque': []}
    for record in records:
        for part in ('functions', 'skeletons', 'files', 'retail', 'dirs', 'programs'):
            out[part].update(record.get(part, {}))
        out['opaque'] = sorted(set(out['opaque']) | set(record.get('opaque', [])))
    for part in ('functions', 'skeletons', 'files', 'retail', 'dirs', 'programs'):
        out[part] = dict(sorted(out[part].items()))
    return out


def digest(record: dict) -> str:
    return hashlib.sha256(json.dumps(record, sort_keys=True).encode()).hexdigest()


def save(record: dict) -> str:
    """Store `record` under its digest and return the digest (receipts name records, not repeat them)."""
    name = digest(record)
    path = DEPS_DIR / f'{name}.json'
    if not path.exists():
        DEPS_DIR.mkdir(parents=True, exist_ok=True)
        temp = path.with_name(path.name + f'.{os.getpid()}.tmp')
        temp.write_text(json.dumps(record, sort_keys=True))
        temp.replace(path)
    return name


# Dependency records of the units whose results this process used (host/stagecache.py names them in the
# stage record, so the stage is stale whenever one of them is).
STAGE_UNITS: set[str] = set()

_LOADED: dict[str, dict | None] = {}


def load(name: str) -> dict | None:
    if name not in _LOADED:
        try:
            _LOADED[name] = json.loads((DEPS_DIR / f'{name}.json').read_text())
        except (OSError, ValueError):
            _LOADED[name] = None
    return _LOADED[name]


_VERDICT: dict[str, tuple[str, ...]] = {}


def stale_by_name(name: str, retail: Retail | None) -> list[str]:
    """`stale` for a stored record, judged once per process however many receipts name it."""
    if name not in _VERDICT:
        record = load(name)
        if record is None:
            _VERDICT[name] = ('dependency record missing',)
        else:
            _VERDICT[name] = tuple(stale(record, retail))
    return list(_VERDICT[name])


def collect_garbage(referenced: set[str]) -> int:
    """Delete stored records no receipt names any more; returns how many."""
    removed = 0
    if DEPS_DIR.is_dir():
        for path in DEPS_DIR.glob('*.json'):
            if path.stem not in referenced:
                path.unlink()
                removed += 1
    return removed
