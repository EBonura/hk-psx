"""Read facts out of Rust source without depending on how it is formatted.

The cook and the tests state some numbers once, in Rust, and read them from
Python (a capacity, a table size, a glyph budget). Reading them with a regex over
the raw text breaks the day `cargo fmt` moves a space, a comma or a line break,
which is how the crate-wide format pass broke a dozen of them at once.

Two tools, both indifferent to layout:

* `const_int` / `const_expr` answer "what is constant NAME in this file" and
  evaluate integer arithmetic (`4 * 1024`, `1 << 20`, other constants of the same
  file, `as` casts, `_` separators, hex).
* `compact` returns the source with comments dropped and every space that is not
  between two word characters removed, plus trailing commas before a closing
  bracket, so `pub const A: u16 = 1;` and the same declaration split over three
  lines both read `pub const A:u16=1;`. A regex written against that one shape
  matches however the source is laid out. Write patterns in compact form.
"""
import ast
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

_TOKEN = re.compile(
    r'''
      (?P<comment>//[^\n]*|/\*.*?\*/)
    | (?P<string>b?r(?P<hashes>\#*)".*?"(?P=hashes)|b?"(?:\\.|[^"\\])*")
    | (?P<char>b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'\n])')
    | (?P<word>[A-Za-z0-9_]+)
    | (?P<space>\s+)
    | (?P<other>.)
    ''',
    re.S | re.X,
)
_WORD_END = re.compile(r'[A-Za-z0-9_]$')
_WORD_START = re.compile(r'^[A-Za-z0-9_]')


def compact(text):
    """The source minus comments, in the one layout-free spelling described above."""
    out = []
    pending_space = False
    for match in _TOKEN.finditer(text):
        kind = match.lastgroup
        if kind == 'hashes':
            kind = 'string'
        token = match.group()
        if kind == 'comment':
            # A comment separates tokens like whitespace does.
            pending_space = True
            continue
        if kind == 'space':
            pending_space = True
            continue
        if pending_space and out and _WORD_END.search(out[-1]) and _WORD_START.match(token):
            out.append(' ')
        pending_space = False
        # rustfmt adds a trailing comma before a closing bracket; hand-written
        # source often has none, so neither spelling is the canonical one.
        if token in ')]}' and out and out[-1] == ',':
            out.pop()
        out.append(token)
    return ''.join(out)


def source(path):
    """`compact` of the file at `path` (absolute, or relative to the repo root)."""
    path = Path(path)
    return compact((path if path.is_absolute() else ROOT / path).read_text())


def const_expr(text, name):
    """The initialiser of `const NAME` / `static [mut] NAME`, as compact source.

    It runs to the `;` that closes the declaration, so array and struct
    initialisers (which contain `;` and `,` of their own) come back whole.
    """
    found = re.search(rf'\b(?:const|static(?: mut)?) {re.escape(name)}:', text)
    if not found:
        raise KeyError(name)
    # The type may hold a `;` (`[u8;95]`) but never an `=`.
    start = text.index('=', found.end()) + 1
    depth = 0
    for at in range(start, len(text)):
        char = text[at]
        if char in '([{':
            depth += 1
        elif char in ')]}':
            depth -= 1
        elif char == ';' and depth == 0:
            return text[start:at]
    raise ValueError('unterminated constant ' + name)


_NUMBER = re.compile(r'\b(0x[0-9a-fA-F_]+|0b[01_]+|0o[0-7_]+|\d[\d_]*)(?:[iu](?:8|16|32|64|128|size))?\b')
_CAST = re.compile(r'\bas ?[iu](?:8|16|32|64|128|size)\b')


def _evaluate(expr, text, depth):
    expr = _CAST.sub('', expr)
    expr = _NUMBER.sub(lambda m: str(int(m.group(1).replace('_', ''), 0)), expr)
    expr = expr.replace('/', '//')

    def walk(node):
        if isinstance(node, ast.Expression):
            return walk(node.body)
        if isinstance(node, ast.Constant) and isinstance(node.value, int):
            return node.value
        if isinstance(node, ast.Name):
            if depth > 16:
                raise ValueError('constant cycle at ' + node.id)
            return _evaluate(const_expr(text, node.id), text, depth + 1)
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.USub, ast.UAdd, ast.Invert)):
            value = walk(node.operand)
            return {ast.USub: -value, ast.UAdd: value, ast.Invert: ~value}[type(node.op)]
        if isinstance(node, ast.BinOp):
            ops = {
                ast.Add: lambda a, b: a + b, ast.Sub: lambda a, b: a - b,
                ast.Mult: lambda a, b: a * b, ast.FloorDiv: lambda a, b: a // b,
                ast.Mod: lambda a, b: a % b, ast.LShift: lambda a, b: a << b,
                ast.RShift: lambda a, b: a >> b, ast.BitAnd: lambda a, b: a & b,
                ast.BitOr: lambda a, b: a | b, ast.BitXor: lambda a, b: a ^ b,
            }
            return ops[type(node.op)](walk(node.left), walk(node.right))
        raise ValueError('not a plain integer expression: ' + expr)

    return walk(ast.parse(expr, mode='eval'))


def _text(path_or_text):
    """Compact text for a `Path`, a repo-relative `*.rs` name, or text already compacted."""
    if isinstance(path_or_text, Path) or (
            isinstance(path_or_text, str) and '\n' not in path_or_text and path_or_text.endswith('.rs')):
        return source(path_or_text)
    return path_or_text


def const_int(path_or_text, name):
    """Integer value of constant `name`; accepts a path (see `source`) or compact text."""
    text = _text(path_or_text)
    return _evaluate(const_expr(text, name), text, 0)


def _python(expr, one):
    """Evaluate a plain initialiser: integers, lists, tuples, booleans, `ONE`."""
    expr = _CAST.sub('', expr)
    expr = _NUMBER.sub(lambda m: str(int(m.group(1).replace('_', ''), 0)), expr)
    expr = re.sub(r'\btrue\b', 'True', re.sub(r'\bfalse\b', 'False', expr))
    return eval(expr, {'__builtins__': {}}, {'ONE': one})


def const_ints(path_or_text, name, one=65536):
    """The integers of an array initialiser (`[1, -2, 3]`), in order."""
    return list(_python(const_expr(_text(path_or_text), name), one))


def consts(path_or_text, one=65536, private=False):
    """{NAME: value} for every `pub const` whose initialiser is plain Python once
    `as` casts and literal suffixes are gone: integers, lists, tuples, booleans.
    `ONE` stands for the fixed-point unit (65536 unless told otherwise). Constants
    that are not of that shape (structs, references) are left out.
    `private=True` also reads the ones without `pub`."""
    text = _text(path_or_text)
    found = {}
    for match in re.finditer(r'\bconst (\w+):' if private else r'\bpub const (\w+):', text):
        name = match.group(1)
        try:
            found[name] = _python(const_expr(text, name), one)
        except Exception:
            continue
    return found


def enum_variants(path_or_text, name):
    """Variant names of `enum NAME`, in declaration order (payloads and attributes dropped)."""
    text = _text(path_or_text)
    found = re.search(rf'\benum {re.escape(name)}(?:<[^>]*>)?{{', text)
    if not found:
        raise KeyError(name)
    depth, at = 1, found.end()
    start = at
    while depth:
        char = text[at]
        depth += char in '([{'
        depth -= char in ')]}'
        at += 1
    body = re.sub(r'#\[[^\]]*\]', '', text[start:at - 1])
    names, depth, current = [], 0, ''
    for char in body + ',':
        if char in '([{':
            depth += 1
        elif char in ')]}':
            depth -= 1
        if char == ',' and depth == 0:
            variant = re.match(r'\w+', current)
            if variant:
                names.append(variant.group())
            current = ''
        else:
            current += char
    return names


def contains(path_or_text, snippet):
    """Whether the source holds `snippet`, whatever the layout of either."""
    return compact(snippet) in _text(path_or_text)


def struct_fields(path_or_text, name):
    """Names of the fields of `struct NAME { ... }`, in declaration order."""
    text = _text(path_or_text)
    found = re.search(rf'\bstruct {re.escape(name)}{{', text)
    if not found:
        raise KeyError(name)
    depth, at = 1, found.end()
    start = at
    while depth:
        char = text[at]
        depth += char in '([{<'
        depth -= char in ')]}>'
        at += 1
    names, depth, current = [], 0, ''
    for char in re.sub(r'#\[[^\]]*\]', '', text[start:at - 1]) + ',':
        if char in '([{<':
            depth += 1
        elif char in ')]}>':
            depth -= 1
        if char == ',' and depth == 0:
            field = re.match(r'(?:pub(?:\([^)]*\))? ?)?(\w+):', current)
            if field:
                names.append(field.group(1))
            current = ''
        else:
            current += char
    return names
