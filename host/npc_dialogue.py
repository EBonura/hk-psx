"""Cook the conversations one NPC speaks into the guest's panel table.

The guest already renders a bounded dialogue panel for the Tutorial tablets
(host/read_points.py, game/src/dialogue.rs). An NPC conversation is that same
panel with a different page list, so only the list is cooked here: the entries
`Conversation Control` reaches, decrypted and wrapped to the same Perpetua cell
advances the tablet pages use.

Not one list but a chain. `Conversation Control` picks its line from PlayerData
and the state it picks writes the flag that changes the next pick, so the port
cooks every conversation npcs.py can decide in turn and remembers per NPC how
far along the chain a save has got. The last one in the chain is the one the
source repeats forever, so the cursor saturates there.

Not cooked: the branches that need PlayerData the port has no producer for
(those stop the chain), the yes/no box, the NPC name plate and the
conversation's own audio.
"""
import hashlib, io, math, re
from PIL import ImageFont
from source import ROOT, dump
from language import sheet as language_sheet
from npcs import DEACTIVATORS, conversation_chain, playerdata_defaults
from read_points import wrap_page

# Same panel as the tablets, so the same page and glyph bounds hold.
MAX_PAGES = 8
MAX_PAGE_GLYPHS = 384
FOOTER = 'X:NextO:Close'
# The persistent cursor is two bits per NPC in one 32-bit save word, so these
# are the save record's bounds and not the panel's. game/src/npc_state.rs
# declares the same two, and dialogue.rs refuses to link a table past either,
# so a disagreement fails the guest build rather than wrapping one NPC's
# progress into the next one's.
CURSOR_BITS = 2
MAX_SLOTS = 32 // CURSOR_BITS


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


def _gate_fields(sc):
    """Every PlayerData bool this scene's load-time gates read.

    activation.py answers those gates once, at cook time, so a conversation that
    writes one of them would move a gate the cooked world has already committed
    to. Refusing is the only honest answer while the port cannot re-evaluate it.
    """
    return {tree['boolName'] for _cid, (typ, tree) in sc.objects.items()
            if typ in DEACTIVATORS and isinstance(tree.get('boolName'), str)}


def conversations(source, scene, npc, advances, sheets, defaults=None):
    """One NPC's conversation chain as panel pages.

    Each conversation carries the page its own PlayerData write happens on,
    because PlayMaker performs a state's `SetPlayerDataBool` as it enters, before
    the DialogueBox call beside it has spoken. That page is where the guest
    advances the cursor, so the port writes the flag exactly where the source
    does rather than when the panel closes.
    """
    chain = conversation_chain(source, scene, npc['game_object'], defaults)
    gated = _gate_fields(scene)
    cooked = []
    for index, conversation in enumerate(chain):
        pages, advance_page = [], None
        for entry in conversation['entries']:
            key, sheet = entry.get('key'), entry.get('sheet')
            if not isinstance(key, str) or not isinstance(sheet, str) or not key or not sheet:
                raise ValueError(f'{npc["name"]} chooses its key or sheet at runtime')
            if sheet not in sheets:
                sheets[sheet] = language_sheet(source, sheet)
            if key not in sheets[sheet]:
                raise ValueError(f'EN_{sheet} has no {key}')
            written = [write['field'] for write in entry['writes']]
            moves_a_gate = sorted(set(written) & gated)
            if moves_a_gate:
                raise ValueError(f'{npc["name"]} writes {moves_a_gate}, which this scene '
                                 'gates an object on at load')
            if written and advance_page is None:
                advance_page = len(pages)
            pages.extend(_pages(sheets[sheet][key], advances))
        if not 1 <= len(pages) <= MAX_PAGES:
            raise ValueError(f'{npc["name"]} conversation {index} needs {len(pages)} pages; '
                             f'the panel budget is {MAX_PAGES}')
        for page in pages:
            if sum(len(line.replace(' ', '')) for line in page) + len(FOOTER) > MAX_PAGE_GLYPHS:
                raise ValueError(f'{npc["name"]} conversation {index} exceeds the panel glyph budget')
        # The terminal conversation never advances the cursor, and neither does
        # one whose only writes the source has already made; both encode as a
        # page the panel cannot reach.
        if conversation['terminal']:
            advance_page = None
        cooked.append(dict(conversation, pages=pages,
                           advance_page=len(pages) if advance_page is None else advance_page))
    return cooked


def cook(source, scenes, npcs):
    """Write data/npc_lines.rs for the cooked NPC placements."""
    advances = font_advances(source)
    # Decompiled once: reading SetupNewPlayerData is not cheap and every walk
    # starts from the same fresh save.
    defaults = playerdata_defaults(source.directory / 'Managed/Assembly-CSharp.dll')
    sheets, records, audit = {}, [], []
    # A scripted NPC (npc_sources.SCRIPTED) speaks through its own module; it
    # takes no chain and no cursor slot, so the slots of the others hold.
    ordered = sorted((n for n in npcs if not n.get('scripted')), key=lambda n: (n['scene_id'], n['game_object']))
    if len(ordered) > MAX_SLOTS:
        raise ValueError(f'{len(ordered)} cooked NPCs exceed the {MAX_SLOTS} save cursor slots')
    for slot, npc in enumerate(ordered):
        chain = conversations(source, scenes[npc['scene_id']], npc, advances, sheets, defaults)
        records.append({'scene': npc['scene_id'], 'source_id': npc['game_object'],
                        'label': npc['prompt'], 'marker': npc['marker'],
                        'slot': slot, 'chain': chain, 'name': npc['name'],
                        # metElderbug is the one source field an existing route
                        # asserts by name, so its slot stays addressable.
                        'writes_met_elderbug': any(write['field'] == 'metElderbug'
                                                   for write in chain[0]['writes'])})
        audit.append({'npc': npc['name'], 'source': npc['source'], 'scene': npc['scene_id'],
                      'slot': slot, 'limitations': npc.get('limitations', []),
                      'conversations': [
                          {'terminal': c['terminal'], 'pages': len(c['pages']),
                           'advance_page': c['advance_page'], 'states': c['states'],
                           'stops_in': c['stops_in'], 'not_decoded': c['not_decoded'],
                           'entries': [{'key': e['key'], 'sheet': e['sheet'], 'state': e['state'],
                                        'writes': e['writes']} for e in c['entries']]}
                          for c in chain]})
    out = ['// Generated from the local Windows source; no retail payload is embedded.',
           'pub static NPC_LINES:&[NpcLines]=&[']
    for record in records:
        chain = ','.join(
            'NpcConversation{pages:&[%s],advance_page:%d}'
            % (','.join('&[' + ','.join(_quote(line) for line in page) + ']' for page in c['pages']),
               c['advance_page'])
            for c in record['chain'])
        out.append('NpcLines{scene:%d,source_id:%d,label:%s,marker:[%d,%d],slot:%d,conversations:&[%s]},'
                   % (record['scene'], record['source_id'], _quote(record['label']),
                      round(record['marker'][0] * 65536), round(record['marker'][1] * 65536),
                      record['slot'], chain))
    out.append('];')
    witness = [r['slot'] for r in records if r['writes_met_elderbug']]
    # The town-elderbug route asserts the source field by name; this is the slot
    # its cursor lives in, so HK_MET_ELDERBUG stays a real answer.
    out.append('pub static MET_ELDERBUG_SLOT:Option<u8>=%s;'
               % ('None' if not witness else f'Some({witness[0]})'))
    path = ROOT / 'data/npc_lines.rs'
    path.write_text('\n'.join(out) + '\n')
    dump(ROOT / '.hkpsx/npc-lines.json', {
        'npcs': audit, 'sheets': sorted(sheets),
        'cursor_bits': CURSOR_BITS, 'max_slots': MAX_SLOTS,
        'output_sha256': hashlib.sha256(path.read_bytes()).hexdigest(),
        'adaptations': [
            'The entries of one conversation run as one page list in the tablet panel; '
            'the source closes and reopens the box between them',
            'Each NPC keeps a two-bit cursor in the bench save record instead of the '
            'source PlayerData bools its conversations write; nothing else in the port '
            'reads those fields',
            'A conversation the walk could not decide ends the chain, so an NPC repeats '
            'its last decided conversation where the source would move on'],
    })
    print(f'Cooked {len(records)} NPCs, '
          f'{sum(len(r["chain"]) for r in records)} conversations, '
          f'{sum(len(c["pages"]) for r in records for c in r["chain"])} pages', flush=True)
    return records


def _quote(text):
    """A Rust string literal for ASCII panel text."""
    if any(not 32 <= ord(c) <= 126 for c in text):
        raise ValueError(f'panel text outside the cooked font: {text!r}')
    return '"' + text.replace('\\', '\\\\').replace('"', '\\"') + '"'
