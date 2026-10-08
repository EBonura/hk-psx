//! Bounded NPC conversation state: which conversation each NPC is up to, and
//! the panel that runs the chosen one. Independent of texture residency and of
//! the panel's own drawing, the way read_state.rs is for the tablets.
/// One conversation of an NPC's cooked chain: the entries its `Conversation
/// Control` reaches, flattened into panel pages in the order its states run
/// them, and the page whose own state writes the PlayerData that moves the
/// source on to the next conversation. `advance_page` equal to the page count
/// marks the conversation the source repeats for the rest of the game.
pub struct NpcConversation {
    pub pages: &'static [&'static [&'static str]],
    pub advance_page: usize,
}
/// One talkable NPC: where its prompt sits, and every conversation the cook
/// could decide, in the order a player hears them.
pub struct NpcLines {
    pub scene: usize,
    pub source_id: u32,
    pub label: &'static str,
    pub marker: [i32; 2],
    /// This NPC's own cursor inside `Cursors`.
    pub slot: u8,
    pub conversations: &'static [NpcConversation],
}
/// The same pad bits read_state.rs names; dialogue.rs asserts they agree.
pub const UP: u16 = 0x10;
pub const DOWN: u16 = 0x40;
pub const CROSS: u16 = 0x4000;
pub const CIRCLE: u16 = 0x2000;
/// The buttons a panel owns while it is up; the tablet reader's own set.
pub const ACTIONS: u16 = UP | DOWN | CROSS | CIRCLE | 0x8000;
/// Bits per NPC in the persistent word. host/npc_dialogue.py refuses a chain
/// longer than this can count, so a cursor always names a real conversation.
pub const CURSOR_BITS: u32 = 2;
pub const MAX_CONVERSATIONS: usize = 1 << CURSOR_BITS;
pub const MAX_SLOTS: usize = 32 / CURSOR_BITS as usize;
const CURSOR_MASK: u32 = (1 << CURSOR_BITS) - 1;

/// How far along its cooked chain each NPC has got. This is the port's stand-in
/// for the per-NPC PlayerData bools the source's own conversations write
/// (metElderbug, elderbugHistory1, metMiner); nothing else in the port reads
/// those fields, and this word rides in the bench save record, so a cursor
/// survives a quit exactly as the source's flags survive a save.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Cursors(u32);
impl Cursors {
    pub const fn new() -> Self {
        Self(0)
    }
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    pub const fn bits(self) -> u32 {
        self.0
    }
    pub fn get(self, slot: u8) -> usize {
        ((self.0 >> (slot as u32 * CURSOR_BITS)) & CURSOR_MASK) as usize
    }
    pub fn set(&mut self, slot: u8, value: usize) {
        let shift = slot as u32 * CURSOR_BITS;
        self.0 = (self.0 & !(CURSOR_MASK << shift)) | ((value as u32 & CURSOR_MASK) << shift);
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Event {
    pub opened: bool,
    pub advanced: bool,
    pub closed: bool,
}

/// An open NPC conversation. The tablet's `State` owns a list of world
/// triggers; an NPC's trigger is a resident world-bank record instead, so the
/// caller passes the NPC already in range and this keeps only the panel.
pub struct Convo {
    pub active: Option<usize>,
    /// The conversation chosen when the panel opened. The source runs
    /// `Convo Choice` once, on CONVO START, and not again while the box is up,
    /// so a cursor the conversation itself advances cannot switch the pages
    /// out from under the player.
    pub entry: usize,
    pub prompt: Option<usize>,
    pub page: usize,
    previous: u16,
    release: bool,
}
impl Convo {
    pub const fn new() -> Self {
        Self { active: None, entry: 0, prompt: None, page: 0, previous: 0, release: false }
    }
    pub fn open(&self) -> bool {
        self.active.is_some()
    }
    pub fn consumes_actions(&self) -> bool {
        self.open() || self.release
    }
    pub fn cancel(&mut self) {
        self.active = None;
        self.entry = 0;
        self.prompt = None;
        self.page = 0;
        self.release = self.previous & ACTIONS != 0;
    }
    /// One tick. `in_range` is the cooked NPC whose talk trigger the hero body
    /// is inside; UP there is npc_control's own CONVO START. Paging and closing
    /// follow the tablet panel exactly, including holding the closing press
    /// until it is released so it cannot also swing the nail.
    pub fn step(&mut self, lines: &[NpcLines], cursors: &mut Cursors, scene: usize,
                in_range: Option<u32>, eligible: bool, bits: u16) -> Event {
        let pressed = bits & !self.previous;
        self.previous = bits;
        if bits & ACTIONS == 0 {
            self.release = false;
        }
        let mut event = Event::default();
        if let Some(id) = self.active {
            let pages = lines[id].conversations[self.entry].pages.len();
            if lines[id].scene != scene || pressed & CIRCLE != 0 {
                self.cancel();
                event.closed = true;
            } else if pressed & CROSS != 0 {
                if self.page + 1 < pages {
                    self.page += 1;
                    event.advanced = true;
                } else {
                    self.cancel();
                    event.closed = true;
                }
            }
        } else {
            self.prompt = if eligible && !self.release {
                in_range.and_then(|source| {
                    lines.iter().position(|n| n.scene == scene && n.source_id == source)
                })
            } else {
                None
            };
            // npc_control listens for UP alone, unlike the tablet reader.
            if pressed & UP != 0 {
                if let Some(id) = self.prompt {
                    self.active = Some(id);
                    // A cursor past the end of a shorter chain, from a record an
                    // older build wrote, reads as the repeating conversation.
                    self.entry = cursors.get(lines[id].slot).min(lines[id].conversations.len() - 1);
                    self.page = 0;
                    event.opened = true;
                }
            }
        }
        if let Some(id) = self.active {
            let npc = &lines[id];
            // Source `Intro Main` writes metElderbug as PlayMaker enters it,
            // which is before the DialogueBox call beside the write has spoken,
            // so the cursor moves on the page that carries the write and not
            // when the panel closes.
            if self.page >= npc.conversations[self.entry].advance_page
                && self.entry + 1 < npc.conversations.len()
            {
                cursors.set(npc.slot, self.entry + 1);
            }
        }
        event
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Elderbug's cooked shape: an intro whose second page carries the write, a
    // second conversation that writes on its first page, and a terminal one.
    static LINES: [NpcLines; 2] = [
        NpcLines { scene: 1, source_id: 1001, label: "Listen", marker: [0, 0], slot: 0,
            conversations: &[
                NpcConversation { pages: &[&["intro a"], &["intro b"]], advance_page: 1 },
                NpcConversation { pages: &[&["history"]], advance_page: 0 },
                NpcConversation { pages: &[&["generic"]], advance_page: 1 }] },
        NpcLines { scene: 35, source_id: 177, label: "Listen", marker: [0, 0], slot: 1,
            conversations: &[
                NpcConversation { pages: &[&["meet"]], advance_page: 0 },
                NpcConversation { pages: &[&["repeat"]], advance_page: 1 }] },
    ];

    fn open(convo: &mut Convo, cursors: &mut Cursors, scene: usize, source: u32) -> Event {
        convo.step(&LINES, cursors, scene, Some(source), true, 0);
        convo.step(&LINES, cursors, scene, Some(source), true, UP)
    }

    #[test]
    fn cursor_word_packs_every_slot_independently() {
        let mut cursors = Cursors::new();
        for slot in 0..MAX_SLOTS as u8 {
            cursors.set(slot, MAX_CONVERSATIONS - 1);
        }
        assert_eq!(cursors.bits(), u32::MAX);
        for slot in 0..MAX_SLOTS as u8 {
            assert_eq!(cursors.get(slot), MAX_CONVERSATIONS - 1);
        }
        cursors.set(3, 1);
        assert_eq!(cursors.get(3), 1);
        for slot in (0..MAX_SLOTS as u8).filter(|s| *s != 3) {
            assert_eq!(cursors.get(slot), MAX_CONVERSATIONS - 1, "slot {slot} moved");
        }
        assert_eq!(Cursors::from_bits(cursors.bits()), cursors);
    }

    #[test]
    fn the_chain_advances_once_per_conversation_and_stops_at_the_repeating_one() {
        let mut cursors = Cursors::new();
        let mut convo = Convo::new();
        // The intro advances on the page that carries the write, not on close.
        assert!(open(&mut convo, &mut cursors, 1, 1001).opened);
        assert_eq!(cursors.get(0), 0);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        assert!(convo.step(&LINES, &mut cursors, 1, Some(1001), true, CROSS).advanced);
        assert_eq!(cursors.get(0), 1);
        // The pages in front of the player do not change under that write.
        assert_eq!(convo.entry, 0);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        assert!(convo.step(&LINES, &mut cursors, 1, Some(1001), true, CROSS).closed);
        // Second talk: History 1, which advances on its own first page.
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        open(&mut convo, &mut cursors, 1, 1001);
        assert_eq!(convo.entry, 1);
        assert_eq!(cursors.get(0), 2);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, CROSS);
        // Third and after: the generic line, forever.
        for _ in 0..3 {
            convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
            open(&mut convo, &mut cursors, 1, 1001);
            assert_eq!(convo.entry, 2);
            assert_eq!(cursors.get(0), 2);
            convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
            convo.step(&LINES, &mut cursors, 1, Some(1001), true, CROSS);
        }
        // One NPC's progress is its own.
        assert_eq!(cursors.get(1), 0);
    }

    #[test]
    fn a_restored_word_picks_up_where_the_save_left_off() {
        let mut cursors = Cursors::new();
        cursors.set(1, 1);
        let bits = cursors.bits();
        let mut cursors = Cursors::from_bits(bits);
        let mut convo = Convo::new();
        open(&mut convo, &mut cursors, 35, 177);
        assert_eq!(convo.entry, 1);
        // Terminal: its advance page is out of reach, so nothing moves.
        assert_eq!(cursors.get(1), 1);
    }

    #[test]
    fn a_cursor_past_a_shortened_chain_reads_as_the_repeating_conversation() {
        let mut cursors = Cursors::from_bits(0);
        cursors.set(1, MAX_CONVERSATIONS - 1);
        let mut convo = Convo::new();
        open(&mut convo, &mut cursors, 35, 177);
        assert_eq!(convo.entry, LINES[1].conversations.len() - 1);
    }

    #[test]
    fn leaving_the_scene_or_pressing_circle_closes_and_holds_the_press() {
        let mut cursors = Cursors::new();
        let mut convo = Convo::new();
        open(&mut convo, &mut cursors, 1, 1001);
        assert!(convo.step(&LINES, &mut cursors, 35, Some(1001), true, 0).closed);
        assert!(!convo.open());
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        open(&mut convo, &mut cursors, 1, 1001);
        assert!(convo.step(&LINES, &mut cursors, 1, Some(1001), true, CIRCLE | 0x8000).closed);
        assert!(convo.consumes_actions());
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0x8000);
        assert!(convo.consumes_actions());
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        assert!(!convo.consumes_actions());
    }

    #[test]
    fn the_trigger_the_knight_stands_in_does_not_reopen_behind_a_held_press() {
        let mut cursors = Cursors::new();
        let mut convo = Convo::new();
        open(&mut convo, &mut cursors, 1, 1001);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, CROSS);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        assert!(convo.step(&LINES, &mut cursors, 1, Some(1001), true, CROSS).closed);
        // The close consumed a held CROSS, and UP inside the same trigger must
        // wait for the release before it can start another conversation.
        assert!(!convo.step(&LINES, &mut cursors, 1, Some(1001), true, UP | CROSS).opened);
        assert!(!convo.open());
    }

    #[test]
    fn an_ineligible_frame_never_restarts_an_open_page() {
        let mut cursors = Cursors::new();
        let mut convo = Convo::new();
        open(&mut convo, &mut cursors, 1, 1001);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, 0);
        convo.step(&LINES, &mut cursors, 1, Some(1001), true, CROSS);
        for _ in 0..10 {
            convo.step(&LINES, &mut cursors, 1, None, false, 0);
            assert_eq!(convo.page, 1);
            assert!(convo.open());
        }
    }
}
