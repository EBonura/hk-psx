"""Cook the lines one NPC speaks on a fresh save into the guest's panel table.

The guest already renders a bounded dialogue panel for the Tutorial tablets
(host/read_points.py, game/src/dialogue.rs). An NPC conversation is that same
panel with a different page list, so only the list is cooked here: the entries
`Conversation Control` reaches on a fresh save, decrypted and wrapped to the
same Perpetua cell advances the tablet pages use.

Only the fresh-save branch is cooked. The other branches, the said-once flags,
the yes/no box, the NPC name plate and the conversation's own audio are not.
"""
import hashlib, io, math, re
from PIL import ImageFont
from source import ROOT, dump
from language import sheet as language_sheet
from npcs import fresh_save_conversation
from read_points import wrap_page

# The one PlayerData bool the guest carries for an NPC. A conversation that
# writes anything else is refused rather than cooked with its write dropped.
MET_FIELD = 'metElderbug'
# Same panel as the tablets, so the same page and glyph bounds hold.
MAX_PAGES = 8
MAX_PAGE_GLYPHS = 384
FOOTER = 'X:NextO:Close'


def font_advances(source):
    """The panel's Perpetua cell advances, recomputed the way read_points does.

    read_points.py builds the font strip and the ADVANCES table the guest links;
    wrapping to a different width here would overflow the same panel, so the
    recomputed widths are checked against that generated table instead of two
    copies being trusted to stay equal.
    """
    obj = next(o for o in source.file('resources.assets').objects.values()
               if o.type.name == 'Font' and o.read().m_Name == 'Perpetua')
    font = ImageFont.truetype(io.BytesIO(bytes(source.read(obj)['m_FontData'])), 12)
    advances = [max(1, math.ceil(font.getlength(chr(c)))) for c in range(32, 127)]
    generated = ROOT / 'data/read_points.rs'
    if generated.is_file():
        found = re.search(r'ADVANCES:\[u8;95\]=\[([^\]]*)\]', generated.read_text())
        if not found or [int(v) for v in found.group(1).split(',')] != advances:
            raise ValueError('the tablet and NPC panels disagree on the font advances')
    return advances


def _pages(text, advances):
    """One localization entry as panel pages; `<page>` is the authored break."""
    return [wrapped for page in text.split('<page>') for wrapped in wrap_page(page, advances)]


def conversation(source, scene, npc, advances, sheets):
    """One NPC's fresh-save pages, and the page its met bool is written on."""
    pages, met_page = [], None
    entries = fresh_save_conversation(source, scene, npc['game_object'])
    if not entries:
        raise ValueError(f'{npc["name"]} speaks nothing on a fresh save')
    for entry in entries:
        key, sheet = entry.get('key'), entry.get('sheet')
        if not isinstance(key, str) or not isinstance(sheet, str) or not key or not sheet:
            raise ValueError(f'{npc["name"]} chooses its key or sheet at runtime')
        if sheet not in sheets:
            sheets[sheet] = language_sheet(source, sheet)
        if key not in sheets[sheet]:
            raise ValueError(f'EN_{sheet} has no {key}')
        written = [write['field'] for write in entry['writes']]
        if any(field != MET_FIELD for field in written):
            raise ValueError(f'{npc["name"]} writes {written} and the guest carries only {MET_FIELD}')
        if written and met_page is None:
            met_page = len(pages)
        pages.extend(_pages(sheets[sheet][key], advances))
    if not 1 <= len(pages) <= MAX_PAGES:
        raise ValueError(f'{npc["name"]} needs {len(pages)} pages; the panel budget is {MAX_PAGES}')
    for page in pages:
        if sum(len(line.replace(' ', '')) for line in page) + len(FOOTER) > MAX_PAGE_GLYPHS:
            raise ValueError(f'{npc["name"]} exceeds the panel glyph budget')
    return pages, (len(pages) if met_page is None else met_page), entries


def cook(source, scenes, npcs):
    """Write data/npc_lines.rs for the cooked NPC placements."""
    advances = font_advances(source)
    sheets, records, audit = {}, [], []
    for npc in sorted(npcs, key=lambda n: (n['scene_id'], n['game_object'])):
        pages, met_page, entries = conversation(source, scenes[npc['scene_id']], npc, advances, sheets)
        records.append({'scene': npc['scene_id'], 'source_id': npc['game_object'],
                        'label': npc['prompt'], 'marker': npc['marker'],
                        'pages': pages, 'met_page': met_page})
        audit.append({'npc': npc['name'], 'source': npc['source'], 'scene': npc['scene_id'],
                      'entries': [{'key': e['key'], 'sheet': e['sheet'], 'state': e['state'],
                                   'writes': e['writes']} for e in entries],
                      'pages': len(pages), 'met_page': met_page, 'met_field': MET_FIELD})
    out = ['// Generated from the local Windows source; no retail payload is embedded.',
           'pub static NPC_LINES:&[NpcLines]=&[']
    for record in records:
        pages = ','.join('&[' + ','.join(_quote(line) for line in page) + ']' for page in record['pages'])
        out.append('NpcLines{scene:%d,source_id:%d,label:%s,marker:[%d,%d],pages:&[%s],met_page:%d},'
                   % (record['scene'], record['source_id'], _quote(record['label']),
                      round(record['marker'][0] * 65536), round(record['marker'][1] * 65536),
                      pages, record['met_page']))
    out.append('];')
    path = ROOT / 'data/npc_lines.rs'
    path.write_text('\n'.join(out) + '\n')
    dump(ROOT / '.hkpsx/npc-lines.json', {
        'npcs': audit, 'sheets': sorted(sheets),
        'output_sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
        'adaptations': ['The two fresh-save entries run as one page list in the tablet panel; '
                        'the source closes and reopens the box between them',
                        'metElderbug is a session flag on the guest, not a saved PlayerData record'],
    })
    print(f'Cooked {len(records)} NPC conversations, '
          f'{sum(len(r["pages"]) for r in records)} pages', flush=True)
    return records


def _quote(text):
    """A Rust string literal for ASCII panel text."""
    if any(not 32 <= ord(c) <= 126 for c in text):
        raise ValueError(f'panel text outside the cooked font: {text!r}')
    return '"' + text.replace('\\', '\\\\').replace('"', '\\"') + '"'
