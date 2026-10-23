//! Bench Save Game on the port-1 memory card (P13 step 5 to 7).
//!
//! The record holds the bench respawn marker, Geo wallet, Great Door hit count,
//! the owed Shade, the charm board, NPC conversation cursors, the script bank's
//! PlayerData, Sly's half of PlayerData (which is where the masks a shard fuse
//! bought live: `shop::vitals` composes `maxHealthBase` from it), and since
//! HKS5 the port's own PlayerData and SceneData (`persist.rs`): bosses beaten,
//! walls broken, cocoons opened, secrets revealed, Geo rocks mined. Writes run
//! between input checkpoints so the pad sampler keeps polling (SIO0 is shared,
//! so a card frame and a pad poll never overlap). A card that cannot be written
//! is never silent: `write` returns the `Fault`, `save_ui.rs` says so on screen
//! and the card layer's last transaction is exported (`HK_CARD_*`) for a console
//! photograph or an emulator watch. An unformatted card is only formatted when
//! the player agrees.
//!
//! Two files are written alternately, each carrying a sequence number, and the
//! newer valid one wins at boot. `Card::write` frees the file's directory entry
//! before writing its data and only restores it at the end, so for roughly 190
//! ms an interrupted write leaves that file unfindable with its predecessor
//! already destroyed. Alternating means an interrupted write never touches the
//! copy currently being relied on.
use psx_mc::{Block, Card, HardwareCard, SaveIcon, TransportFault, FRAME_SIZE};

/// Source save profiles. Each one owns two card files so an interrupted write
/// cannot destroy the copy in use, which is 8 of the card's 15 blocks.
pub const PROFILES: usize = 4;
const TITLE: &str = "HOLLOW KNIGHT PSX";
/// Bumped from HKS1 when the charm board and the NPC conversation cursors
/// joined the record, from HKS2 when the script runtime's PlayerData did, from
/// HKS3 when Sly's half of PlayerData did, and from HKS4 when the world state
/// did. HKS1 to HKS3 read back as Corrupt rather than decoding into a save
/// whose notch board, conversations, progression flags or shop purchases would
/// be invented.
///
/// HKS4 still loads, because nothing HKS5 adds is invented by reading it: an
/// HKS4 save never recorded a broken wall or a mined rock, so its world is the
/// authored one, which is exactly what that save restored before. The one
/// thing it did carry, the False Knight's arena and first plop in the script
/// reserve's top two slots, `main` moves into `persist` on load.
const MAGIC: [u8; 4] = *b"HKS5";
const MAGIC_HKS4: [u8; 4] = *b"HKS4";
pub const LEN_HKS4: usize = 156;
/// HKS4's fields end here, where HKS4 put its checksum; HKS5 keeps every one of
/// them at the same offset.
const HKS4_FIELDS: usize = LEN_HKS4 - 4;
/// Then `persist`'s PlayerData bools (4), its levels (4) and the SceneData item
/// count (2), then the items, then the checksum.
const ITEMS_AT: usize = HKS4_FIELDS + 4 + persist::LEVELS + 2;
/// The shortest HKS5 record: a world nobody has touched, with no items.
/// `tools/migrate_cards.py` reads this as the length an HKS4 fixture grows to.
pub const LEN: usize = 166;
/// The longest, with the store full. Eight 128-byte card frames with the psx-mc
/// container ahead of it, against the one 8 KiB block each file owns.
pub const MAX_LEN: usize = LEN + persist::MAX_ITEMS * 4;
use crate::persist;
/// The shop counters `shop::State::record` packs, which is the one field here
/// whose width is another module's to choose. Named so the length arithmetic
/// below reads as the fields rather than as a number.
pub const SHOP_COUNTERS: usize = 6;
/// PlayerData slots reserved for the cooked script bank. A fixed reserve rather
/// than the bank's own count, because the record length cannot follow a
/// generated table: every change to it invalidates every card fixture. The
/// cooker refuses a bank that needs more, which is a build failure rather than a
/// record that quietly holds some of the fields.
pub const SCRIPT_FIELD_SLOTS: usize = 16;
/// The profile-wide record (the original's roaming shared data): which title styles the endings and the
/// Mighty Zote fight have unlocked, one bit each (`menu_state::STYLE_KEY`). Its own file, so no slot owns
/// it; a missing or damaged one reads as nothing unlocked. docs/SAVE.md.
const SHARED_NAME: &str = "BASLUS-00000HKPSXMS";
const SHARED_MAGIC: [u8; 4] = *b"HKU1";
/// Magic, the unlock mask, three zero bytes, then the checksum of those eight.
const SHARED_LEN: usize = 12;
static mut UNLOCKED: u8 = 0;
/// The unlock mask the card gave at boot, plus what this session unlocked.
pub fn unlocked_styles() -> u8 {
    unsafe { UNLOCKED }
}
fn read_shared<B: Block>(card: &mut Card<B>) {
    let buf = buf();
    if let Ok(len) = card.read(SHARED_NAME, buf) {
        let b = &buf[..len.min(MAX_LEN)];
        if b.len() >= SHARED_LEN
            && b[..4] == SHARED_MAGIC
            && fnv(&b[..8]).to_le_bytes() == b[8..12]
        {
            unsafe { UNLOCKED = b[4] & 0xF }
        }
    }
}
/// Set the keys in `mask` (an ending, the Mighty Zote fight, or the Cheats page) and write the record.
/// The keys hold for the session whatever the card does: a card that is missing, unformatted or full
/// is left alone (an unformatted one is never formatted unasked) and the record is written again at
/// the next unlock. A full card keeps its blocks: the record needs one of its own.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn unlock_styles(mask: u8) {
    let mask = unsafe {
        UNLOCKED |= mask & 0xF;
        UNLOCKED
    };
    let out = buf();
    out[..4].copy_from_slice(&SHARED_MAGIC);
    out[4..8].copy_from_slice(&[mask, 0, 0, 0]);
    let sum = fnv(&out[..8]);
    out[8..12].copy_from_slice(&sum.to_le_bytes());
    let mut card = Card::new(SlotOne::new(false));
    if !matches!(card.is_formatted(), Ok(true)) {
        return;
    }
    let result = card.write_with_icon(SHARED_NAME, TITLE, &out[..SHARED_LEN], &ICON);
    unsafe {
        match result {
            Ok(()) => HK_SAVE_WRITES = HK_SAVE_WRITES.saturating_add(1),
            Err(_) => HK_SAVE_ERRORS = HK_SAVE_ERRORS.saturating_add(1),
        }
    }
}
/// Per profile: the copy the next write goes to, and the newest sequence seen.
static mut NEXT_COPY: [usize; PROFILES] = [0; PROFILES];
static mut LAST_SEQUENCE: [u32; PROFILES] = [0; PROFILES];

/// `BASLUS-00000HKPSX<profile><copy>`, built without a formatter.
fn file_name(profile: usize, copy: usize, out: &mut [u8; 19]) -> &str {
    out.copy_from_slice(b"BASLUS-00000HKPSX00");
    out[17] = b'0' + profile as u8;
    out[18] = b'0' + copy as u8;
    core::str::from_utf8(out).expect("ASCII save name")
}
/// What a profile holds, for the selection screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Empty,
    Used(Save),
    /// Both copies present but neither decodes.
    Corrupt,
}
/// Why a card operation could not be completed, in the terms the plan asks for.
/// The discriminants are published in `HK_SAVE_FAULT` (plus one), so new kinds
/// go on the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    NoCard,
    Full,
    Corrupt,
    Unformatted,
    Other,
    /// A card answered and then broke the protocol on a read.
    Comms,
    /// A card answered and then refused or lost a frame write.
    Write,
}
impl Fault {
    /// A read-side error. A card that answers and then breaks protocol is not
    /// an absent card, and saying "no card" for it would send the player to
    /// reseat a card that is fine.
    pub fn of(error: psx_mc::Error) -> Self {
        use psx_mc::Error::*;
        match error {
            NoCard => Fault::NoCard,
            Protocol => Fault::Comms,
            NoSpace => Fault::Full,
            BadChecksum | Corrupt | BadContainer => Fault::Corrupt,
            NotFormatted => Fault::Unformatted,
            _ => Fault::Other,
        }
    }
    /// A write-side error: the card's refusal of a frame is a write error, not
    /// damage the player could find with the card manager.
    pub fn of_write(error: psx_mc::Error) -> Self {
        match error {
            psx_mc::Error::Protocol | psx_mc::Error::BadChecksum => Fault::Write,
            other => Self::of(other),
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Fault::NoCard => "No memory card in slot 1",
            Fault::Full => "Memory card is full",
            Fault::Corrupt => "Memory card data is damaged",
            Fault::Unformatted => "Memory card is not formatted",
            Fault::Comms => "Memory card is not responding",
            Fault::Write => "Could not write to the memory card",
            Fault::Other => "Memory card error",
        }
    }
    /// What to do about it, for the failure notice.
    pub fn hint(self) -> &'static str {
        match self {
            Fault::NoCard => "Insert a card in slot 1 and save again.",
            Fault::Full => "Free a block in the memory card manager.",
            Fault::Corrupt => "Check the card in the memory card manager.",
            Fault::Unformatted => "",
            Fault::Comms | Fault::Write | Fault::Other => "Check the card and try again.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Save {
    pub scene: u32,
    pub seat: [i32; 2],
    pub facing: i32,
    pub region: u32,
    pub geo: u32,
    pub door_hits: u8,
    /// Higher wins when both slots hold a valid record.
    pub sequence: u32,
    /// The Hollow Shade owed at the time of the save, and the Geo it carries.
    pub shade: crate::shade::Record,
    /// The charm half of PlayerData: `gotCharm_N` and `equippedCharm_N` as bit
    /// per charm, `charmSlots`, and `canOvercharm`. `charmSlotsFilled` and
    /// `overcharmed` are not stored because the source derives both, in
    /// `CalculateNotchesUsed` and `RefreshOvercharm`, and a stored copy could
    /// only ever contradict the set it was derived from.
    pub charms_owned: psx_math::Mask64,
    pub charms_equipped: psx_math::Mask64,
    pub charm_notches: u8,
    pub can_overcharm: bool,
    /// How far along its cooked conversation chain each talkable NPC has got,
    /// two bits per NPC. The port's stand-in for the per-NPC PlayerData bools
    /// the source's own conversations write, and the only reason Elderbug does
    /// not introduce himself again after a reload. See game/src/npc_state.rs.
    pub npc_conversations: u32,
    /// The PlayerData the cooked script bank reads and writes, by bank slot.
    pub script_fields: [i32; SCRIPT_FIELD_SLOTS],
    /// The identity of the field list those values are indexed by. Slot order
    /// is the cook's, so a record written by one bank and read by another would
    /// put `seenFocusTablet` where `currentArea` belongs. The loader compares
    /// this with the bank it is running and drops the values when they disagree,
    /// which is the one failure a checksum cannot catch.
    pub script_field_fnv: u32,
    /// The shop half of PlayerData: the cooked shop bools Sly's stock reads and
    /// writes, as `shop::State`'s own bitmask. The charm half of a purchase is
    /// absent because it is already in `charms_owned`: a charm row's bool *is*
    /// `gotCharm_N`, which is why a charm bought at Sly's survived a quit while
    /// everything else on the shelf did not.
    pub shop_slots: u32,
    /// `heartPieces`, `vesselFragments`, `maxHealthBase` above the starting
    /// five, the fused vessels, `simpleKeys` and `rancidEggs`, in that order.
    /// `shop::State::restore` is what decides whether they agree with each
    /// other; this only carries them.
    pub shop_counters: [u8; SHOP_COUNTERS],
    /// `persist`'s PlayerData: the bools by bit, then `fireballLevel`,
    /// `quakeLevel`, `screamLevel` and a reserved zero. The SceneData items
    /// follow them on the card but are not part of this struct: they go
    /// straight between the card buffer and `persist::store()`, because four
    /// profiles of them would cost the boot survey 2 KiB of stack for records
    /// only one of which is ever loaded.
    pub player_bools: u32,
    pub player_levels: [u8; persist::LEVELS],
    /// The tag this record was read from, 4 or 5; `encode` always writes 5.
    pub version: u8,
}
fn fnv(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0x811c9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(16777619))
}
impl Save {
    /// Write the record and `items` (sorted `persist` words) into `out`, and
    /// return the record's length. `out` must hold `MAX_LEN` bytes.
    pub fn encode(&self, items: &[u32], out: &mut [u8]) -> usize {
        assert!(items.len() <= persist::MAX_ITEMS && out.len() >= MAX_LEN);
        let len = ITEMS_AT + items.len() * 4 + 4;
        let out = &mut out[..len];
        out.fill(0);
        out[0..4].copy_from_slice(&MAGIC);
        out[4..8].copy_from_slice(&self.scene.to_le_bytes());
        out[8..12].copy_from_slice(&self.seat[0].to_le_bytes());
        out[12..16].copy_from_slice(&self.seat[1].to_le_bytes());
        out[16..20].copy_from_slice(&self.facing.to_le_bytes());
        out[20..24].copy_from_slice(&self.region.to_le_bytes());
        out[24..28].copy_from_slice(&self.geo.to_le_bytes());
        out[28] = self.door_hits;
        out[29] = u8::from(self.shade.present);
        out[30..32].copy_from_slice(&self.shade.hp.to_le_bytes());
        out[32..36].copy_from_slice(&self.shade.geo_pool.to_le_bytes());
        out[36..40].copy_from_slice(&self.shade.scene.to_le_bytes());
        out[40..44].copy_from_slice(&self.shade.position[0].to_le_bytes());
        out[44..48].copy_from_slice(&self.shade.position[1].to_le_bytes());
        out[48..52].copy_from_slice(&self.sequence.to_le_bytes());
        out[52..56].copy_from_slice(&self.charms_owned.low().to_le_bytes());
        out[56..60].copy_from_slice(&self.charms_owned.high().to_le_bytes());
        out[60..64].copy_from_slice(&self.charms_equipped.low().to_le_bytes());
        out[64..68].copy_from_slice(&self.charms_equipped.high().to_le_bytes());
        out[68] = self.charm_notches;
        out[69] = u8::from(self.can_overcharm);
        out[70..74].copy_from_slice(&self.npc_conversations.to_le_bytes());
        for (slot, value) in self.script_fields.iter().enumerate() {
            out[74 + slot * 4..78 + slot * 4].copy_from_slice(&value.to_le_bytes());
        }
        let after = 74 + SCRIPT_FIELD_SLOTS * 4;
        out[after..after + 4].copy_from_slice(&self.script_field_fnv.to_le_bytes());
        out[after + 4..after + 8].copy_from_slice(&self.shop_slots.to_le_bytes());
        out[after + 8..after + 8 + SHOP_COUNTERS].copy_from_slice(&self.shop_counters);
        out[HKS4_FIELDS..HKS4_FIELDS + 4].copy_from_slice(&self.player_bools.to_le_bytes());
        out[HKS4_FIELDS + 4..HKS4_FIELDS + 4 + persist::LEVELS]
            .copy_from_slice(&self.player_levels);
        out[ITEMS_AT - 2..ITEMS_AT].copy_from_slice(&(items.len() as u16).to_le_bytes());
        for (i, item) in items.iter().enumerate() {
            out[ITEMS_AT + i * 4..ITEMS_AT + i * 4 + 4].copy_from_slice(&item.to_le_bytes());
        }
        let sum = fnv(&out[..len - 4]);
        out[len - 4..].copy_from_slice(&sum.to_le_bytes());
        len
    }
    /// The record and its SceneData item bytes (four per item, validated and
    /// sorted), or None. An HKS4 record decodes with an empty world.
    #[cfg_attr(not(test), optimize(size))]
    pub fn decode(bytes: &[u8]) -> Option<(Self, &[u8])> {
        let (len, version) = if bytes.get(0..4)? == MAGIC_HKS4 {
            (LEN_HKS4, 4)
        } else if bytes.get(0..4)? == MAGIC && bytes.len() >= LEN {
            let count = u16::from_le_bytes([bytes[ITEMS_AT - 2], bytes[ITEMS_AT - 1]]) as usize;
            if count > persist::MAX_ITEMS {
                return None;
            }
            (ITEMS_AT + count * 4 + 4, 5)
        } else {
            return None;
        };
        if bytes.len() < len
            || fnv(&bytes[..len - 4]) != u32::from_le_bytes(bytes[len - 4..len].try_into().ok()?)
        {
            return None;
        }
        let word = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let mask = |at: usize| {
            psx_math::Mask64::from_words(
                u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
                u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()),
            )
        };
        let items: &[u8] = if version == 5 {
            &bytes[ITEMS_AT..len - 4]
        } else {
            &[]
        };
        // A valid checksum over a list no build could have written is still
        // refused: unknown kinds, and keys out of order or repeated.
        let mut last = None;
        for item in Self::items(items) {
            persist::unpack(item)?;
            if last.is_some_and(|last| item >> 8 <= last) {
                return None;
            }
            last = Some(item >> 8);
        }
        Some((
            Self {
                scene: word(4) as u32,
                seat: [word(8), word(12)],
                facing: word(16).signum().max(-1),
                region: word(20) as u32,
                geo: word(24) as u32,
                door_hits: bytes[28],
                shade: crate::shade::Record {
                    present: bytes[29] != 0,
                    hp: u16::from_le_bytes([bytes[30], bytes[31]]),
                    geo_pool: word(32) as u32,
                    scene: word(36) as u32,
                    position: [word(40), word(44)],
                },
                sequence: word(48) as u32,
                charms_owned: mask(52),
                charms_equipped: mask(60),
                charm_notches: bytes[68],
                can_overcharm: bytes[69] != 0,
                npc_conversations: u32::from_le_bytes(bytes[70..74].try_into().unwrap()),
                script_fields: core::array::from_fn(|slot| word(74 + slot * 4)),
                script_field_fnv: word(74 + SCRIPT_FIELD_SLOTS * 4) as u32,
                shop_slots: word(78 + SCRIPT_FIELD_SLOTS * 4) as u32,
                shop_counters: core::array::from_fn(|i| bytes[82 + SCRIPT_FIELD_SLOTS * 4 + i]),
                player_bools: if version == 5 {
                    word(HKS4_FIELDS) as u32
                } else {
                    0
                },
                player_levels: if version == 5 {
                    core::array::from_fn(|i| bytes[HKS4_FIELDS + 4 + i])
                } else {
                    [0; persist::LEVELS]
                },
                version,
            },
            items,
        ))
    }
    /// The item words of a decoded record's item bytes.
    pub fn items(raw: &[u8]) -> impl Iterator<Item = u32> + '_ {
        raw.chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
    }
}
/// Frame retries before an error stands. A silicon card read under load has
/// dropped an `/ACK` once in a dozen frames (hardware-tests v2.1) and a write
/// frame is idempotent, so a transient protocol error is tried again after the
/// card has had two video periods to settle. An absent card is asked again too,
/// because the first frame after a chain-load is the one that finds the port in
/// whatever state the launcher left it.
const FRAME_RETRIES: u32 = 3;
const RETRY_VBLANKS: u32 = 2;
/// Whether a transaction's failure might be a transient one worth repeating.
fn transient(error: psx_mc::Error, write: bool) -> bool {
    match error {
        psx_mc::Error::Protocol | psx_mc::Error::BadChecksum => true,
        // A card pulled mid-write will not come back, but one that did not
        // answer the first frame may simply not have been listening yet.
        psx_mc::Error::NoCard => !write,
        _ => false,
    }
}
/// The hardware card with retries, a record of the last transaction, and (for
/// a save in play) a pad checkpoint after every frame transfer.
struct SlotOne {
    card: HardwareCard,
    poll: bool,
}
impl SlotOne {
    fn new(poll: bool) -> Self {
        Self {
            card: HardwareCard::new(psx_mc::Slot::One),
            poll,
        }
    }
    /// Publish the transaction that decided the result.
    fn note(&self, retries: u32, frame: u16, write: bool) {
        let trace = self.card.last_trace();
        unsafe {
            HK_CARD_FAULT = match trace.fault {
                TransportFault::None => 0,
                TransportFault::TxTimeout => 1,
                TransportFault::RxTimeout => 2,
                TransportFault::AckTimeout => 3,
                TransportFault::AckReleaseTimeout => 4,
            };
            HK_CARD_FAULT_EXCHANGE = trace.fault_exchange as u32;
            HK_CARD_RETRIES = HK_CARD_RETRIES.saturating_add(retries);
            HK_CARD_FRAMES = HK_CARD_FRAMES.saturating_add(1);
            HK_CARD_LAST = u32::from(write) << 16 | frame as u32;
            #[cfg(feature = "card-diag")]
            crate::card_diag::note(&trace, retries);
        }
    }
}
impl Block for SlotOne {
    fn read_frame(&mut self, frame: u16, out: &mut [u8; FRAME_SIZE]) -> psx_mc::Result<()> {
        let mut retries = 0;
        let mut r = self.card.read_frame(frame, out);
        while let Err(e) = r {
            if retries == FRAME_RETRIES || !transient(e, false) {
                break;
            }
            retries += 1;
            crate::input::wait_vblanks(RETRY_VBLANKS);
            r = self.card.read_frame(frame, out);
        }
        self.note(retries, frame, false);
        if self.poll {
            crate::input::checkpoint();
        }
        r
    }
    fn write_frame(&mut self, frame: u16, data: &[u8; FRAME_SIZE]) -> psx_mc::Result<()> {
        let mut retries = 0;
        let mut r = self.write_once(frame, data);
        while let Err(e) = r {
            if retries == FRAME_RETRIES || !transient(e, true) {
                break;
            }
            retries += 1;
            crate::input::wait_vblanks(RETRY_VBLANKS);
            r = self.write_once(frame, data);
        }
        self.note(retries, frame, true);
        if self.poll {
            crate::input::checkpoint();
        }
        r
    }
}
impl SlotOne {
    /// One frame write, with the title frame in the BIOS card manager's own
    /// Shift-JIS: a retail save's title is double-width Latin, and the manager's
    /// font is what draws it, so the ASCII the filesystem layer copies would be
    /// the one thing about this save that the manager might not show.
    fn write_once(&mut self, frame: u16, data: &[u8; FRAME_SIZE]) -> psx_mc::Result<()> {
        if frame >= FRAMES_PER_BLOCK && frame % FRAMES_PER_BLOCK == 0 && data[..2] == *b"SC" {
            let mut titled = *data;
            sjis_title(TITLE, &mut titled[4..68]);
            return self.card.write_frame(frame, &titled);
        }
        self.card.write_frame(frame, data)
    }
}
const FRAMES_PER_BLOCK: u16 = 64;
/// `title` as double-width Shift-JIS Latin (A-Z, 0-9, space), zero padded.
/// Anything else becomes a double-width space.
pub fn sjis_title(title: &str, out: &mut [u8]) {
    out.fill(0);
    for (i, b) in title.bytes().enumerate() {
        let (hi, lo) = match b {
            b'A'..=b'Z' => (0x82, 0x60 + (b - b'A')),
            b'a'..=b'z' => (0x82, 0x81 + (b - b'a')),
            b'0'..=b'9' => (0x82, 0x4f + (b - b'0')),
            _ => (0x81, 0x40),
        };
        if 2 * i + 1 < out.len() {
            out[2 * i] = hi;
            out[2 * i + 1] = lo;
        }
    }
}
/// The save's icon in the card manager: the Knight's mask, white on black.
pub const ICON: SaveIcon = SaveIcon::new(
    [
        0x0000, 0x7fff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
    pack_icon([
        "................",
        "..1..........1..",
        "..11........11..",
        "..111......111..",
        "..111111111111..",
        ".11111111111111.",
        ".11111111111111.",
        ".11..111111..11.",
        ".11..111111..11.",
        ".111.111111.111.",
        "..111111111111..",
        "...1111111111...",
        "....11111111....",
        "......1111......",
        "................",
        "................",
    ]),
);
/// Sixteen rows of sixteen palette digits, packed as the card manager wants
/// them: two pixels per byte, the left one in the low nibble.
const fn pack_icon(rows: [&str; 16]) -> [u8; FRAME_SIZE] {
    let mut out = [0u8; FRAME_SIZE];
    let mut y = 0;
    while y < 16 {
        let row = rows[y].as_bytes();
        let mut x = 0;
        while x < 16 {
            let v = if row[x] == b'.' { 0 } else { row[x] - b'0' };
            let at = y * 8 + x / 2;
            out[at] |= if x % 2 == 0 { v } else { v << 4 };
            x += 1;
        }
        y += 1;
    }
    out
}
#[no_mangle]
pub static mut HK_SAVE_WRITES: u32 = 0;
#[no_mangle]
pub static mut HK_SAVE_ERRORS: u32 = 0;
#[no_mangle]
pub static mut HK_SAVE_LOADED: u32 = 0;
/// Which of the two copies was last read or written.
#[no_mangle]
pub static mut HK_SAVE_SLOT: u32 = 0;
#[no_mangle]
pub static mut HK_SAVE_PROFILE: u32 = 0;
/// Zero for none, otherwise the Fault discriminant plus one.
#[no_mangle]
pub static mut HK_SAVE_FAULT: u32 = 0;
/// The transaction that decided the last card operation: zero for none, else
/// the first low-level fault (1 TX never ready, 2 RX never filled, 3 `/ACK`
/// never came, 4 `/ACK` never released) and the exchange it happened at. With
/// the fault it is the whole answer to "the card did not respond" on a console.
#[no_mangle]
pub static mut HK_CARD_FAULT: u32 = 0;
#[no_mangle]
pub static mut HK_CARD_FAULT_EXCHANGE: u32 = 0;
/// Frame retries taken, frames transferred, the last frame (bit 16 set for a
/// write) and the video periods the last save held the CPU for.
#[no_mangle]
pub static mut HK_CARD_RETRIES: u32 = 0;
#[no_mangle]
pub static mut HK_CARD_FRAMES: u32 = 0;
#[no_mangle]
pub static mut HK_CARD_LAST: u32 = 0;
#[no_mangle]
pub static mut HK_CARD_WRITE_VBLANKS: u32 = 0;
/// The one record buffer: every card read and write goes through it, so the
/// longest record costs `MAX_LEN` bytes of BSS once rather than stack in the
/// deepest card path. Nothing here is re-entrant.
static mut BUF: [u8; MAX_LEN] = [0; MAX_LEN];
fn buf() -> &'static mut [u8; MAX_LEN] {
    unsafe { &mut *(&raw mut BUF) }
}
/// Call only inside an input scene-load bracket (the sampler drops the
/// samples polled while the card is busy). Writes the copy `read` did not
/// return, so an interruption cannot destroy the save still being relied on.
/// `items` is `persist`'s SceneData list, already brought up to date.
/// An unformatted card is refused with `Fault::Unformatted` unless `format` is
/// set: the player is asked first, as the console's own card manager does.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn write(profile: usize, save: &Save, items: &[u32], format: bool) -> Result<(), Fault> {
    let mut record = *save;
    let copy = unsafe { NEXT_COPY[profile] };
    record.sequence = unsafe { LAST_SEQUENCE[profile] }.wrapping_add(1);
    let mut name = [0u8; 19];
    let name = file_name(profile, copy, &mut name);
    let out = buf();
    let len = record.encode(items, out);
    let began = crate::input::vblanks();
    let mut card = Card::new(SlotOne::new(true));
    let result = card.is_formatted().and_then(|formatted| {
        if !formatted {
            if !format {
                return Err(psx_mc::Error::NotFormatted);
            }
            card.format()?;
        }
        match card.write_with_icon(name, TITLE, &out[..len], &ICON) {
            // Replacing a copy needs room for both until the new one is whole.
            // On a card with one block free, the older copy of this profile is
            // the one to give up: the newer one stays on the card throughout.
            Err(psx_mc::Error::NoSpace) => {
                card.delete(name)?;
                card.write_with_icon(name, TITLE, &out[..len], &ICON)
            }
            other => other,
        }
    });
    unsafe {
        HK_CARD_WRITE_VBLANKS = crate::input::vblanks().wrapping_sub(began);
        match result {
            Ok(()) => {
                HK_SAVE_WRITES = HK_SAVE_WRITES.saturating_add(1);
                HK_SAVE_BYTES = len as u32;
                LAST_SEQUENCE[profile] = record.sequence;
                NEXT_COPY[profile] = 1 - copy;
                HK_SAVE_SLOT = copy as u32;
                HK_SAVE_PROFILE = profile as u32;
                Ok(())
            }
            Err(error) => {
                HK_SAVE_ERRORS = HK_SAVE_ERRORS.saturating_add(1);
                let fault = Fault::of_write(error);
                HK_SAVE_FAULT = fault as u32 + 1;
                Err(fault)
            }
        }
    }
}
/// Length of the last record written, which is what the card write's duration
/// and power-cut window scale with.
#[no_mangle]
pub static mut HK_SAVE_BYTES: u32 = 0;
/// One profile's newer valid copy, and which copy the next write should use.
fn read_profile<B: Block>(card: &mut Card<B>, profile: usize) -> Result<Slot, Fault> {
    let mut newest: Option<(usize, Save)> = None;
    let mut seen = false;
    let mut fault = None;
    for copy in 0..2 {
        let mut name = [0u8; 19];
        let name = file_name(profile, copy, &mut name);
        let buf = buf();
        match card.read(name, buf) {
            Ok(len) => {
                seen = true;
                if let Some((save, _)) = Save::decode(&buf[..len]) {
                    // Sequences only ever increase, so a plain comparison holds.
                    if newest.is_none_or(|(_, best)| save.sequence > best.sequence) {
                        newest = Some((copy, save));
                    }
                }
            }
            Err(psx_mc::Error::NotFound) => {}
            Err(error) => fault = Some(Fault::of(error)),
        }
    }
    if let Some((copy, save)) = newest {
        unsafe {
            LAST_SEQUENCE[profile] = save.sequence;
            NEXT_COPY[profile] = 1 - copy;
        }
        return Ok(Slot::Used(save));
    }
    // A card-level fault matters more than an unreadable record.
    if let Some(fault) = fault {
        return Err(fault);
    }
    Ok(if seen { Slot::Corrupt } else { Slot::Empty })
}
/// Every profile, for the selection screen. Boot-time, before the pad sampler
/// starts (nothing else uses SIO0).
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn survey() -> ([Slot; PROFILES], Option<Fault>) {
    let mut card = Card::new(SlotOne::new(false));
    let mut slots = [Slot::Empty; PROFILES];
    // The first frame decides whether there is a usable card at all. Reading
    // the directory of a blank card finds nothing and says nothing, which is
    // how an unformatted card passed for an empty one until the first save.
    let gate = match card.is_formatted() {
        Ok(true) => None,
        Ok(false) => Some(Fault::Unformatted),
        Err(error) => Some(Fault::of(error)),
    };
    #[cfg(feature = "card-diag")]
    crate::card_diag::boot(gate);
    let mut fault = gate;
    for profile in 0..PROFILES {
        if gate.is_some() {
            break;
        }
        match read_profile(&mut card, profile) {
            Ok(slot) => slots[profile] = slot,
            Err(seen) => {
                fault = Some(seen);
                // A missing or unformatted card fails every profile the same
                // way; there is nothing to learn from asking three more times.
                if matches!(seen, Fault::NoCard | Fault::Unformatted) {
                    break;
                }
            }
        }
    }
    if gate.is_none() {
        read_shared(&mut card);
    }
    unsafe {
        HK_SAVE_LOADED = u32::from(slots.iter().any(|s| matches!(s, Slot::Used(_))));
        HK_SAVE_FAULT = fault.map_or(0, |f| f as u32 + 1);
    }
    (slots, fault)
}
/// The chosen profile's SceneData items, into `store`. Re-reads the copy
/// `survey` picked rather than keeping every profile's items from the survey,
/// which would hold four lists to use one. Boot-time, after the menu and
/// before the pad sampler starts, the same window `survey` runs in.
///
/// Returns the items installed. A read that fails, or finds a different
/// record than the survey did (the card was swapped at the title), installs
/// nothing and says so in `HK_SAVE_FAULT`: the rest of the save still loads,
/// with the authored world, which is the HKS4 behaviour and never an invented
/// one.
#[inline(never)]
pub fn load_world(profile: usize, sequence: u32, store: &mut persist::Store) -> usize {
    let copy = 1 - unsafe { NEXT_COPY[profile] };
    let mut name = [0u8; 19];
    let name = file_name(profile, copy, &mut name);
    let mut card = Card::new(SlotOne::new(false));
    let buf = buf();
    let read = card.read(name, buf);
    let decoded = read
        .as_ref()
        .ok()
        .and_then(|&len| Save::decode(&buf[..len]));
    match decoded {
        Some((save, items)) if save.sequence == sequence => {
            let mut installed = 0;
            for item in Save::items(items) {
                let (kind, scene, local, value) =
                    persist::unpack(item).expect("validated by decode");
                installed += usize::from(store.set(kind, scene, local, value));
            }
            installed
        }
        _ => {
            let fault = match read {
                Err(error) => Fault::of(error),
                Ok(_) => Fault::Corrupt,
            };
            unsafe { HK_SAVE_FAULT = fault as u32 + 1 }
            0
        }
    }
}
