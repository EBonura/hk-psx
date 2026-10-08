//! Bench Save Game on the port-1 memory card (P13 step 5 to 7).
//!
//! The record holds the bench respawn marker, Geo wallet, Great Door hit count,
//! the owed Shade, the charm board, NPC conversation cursors, the script bank's
//! PlayerData, Sly's half of PlayerData (which is where the masks a shard fuse
//! bought live: `shop::vitals` composes `maxHealthBase` from it), and since
//! HKS5 the port's own PlayerData and SceneData (`persist.rs`): bosses beaten,
//! walls broken, cocoons opened, secrets revealed, Geo rocks mined. Writes run
//! between input checkpoints so the pad sampler keeps polling (SIO0 is shared,
//! so a card frame and a pad poll never overlap). A missing or unformatted card
//! is a silent no-save; the rest still happens.
//!
//! Two files are written alternately, each carrying a sequence number, and the
//! newer valid one wins at boot. `Card::write` frees the file's directory entry
//! before writing its data and only restores it at the end, so for roughly 190
//! ms an interrupted write leaves that file unfindable with its predecessor
//! already destroyed. Alternating means an interrupted write never touches the
//! copy currently being relied on.
use psx_mc::{Block, Card, HardwareCard, FRAME_SIZE};

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
/// The longest, with the store full. Six 128-byte card frames with the psx-mc
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    NoCard,
    Full,
    Corrupt,
    Unformatted,
    Other,
}
impl Fault {
    fn of(error: psx_mc::Error) -> Self {
        use psx_mc::Error::*;
        match error {
            NoCard | Protocol => Fault::NoCard,
            NoSpace => Fault::Full,
            BadChecksum | Corrupt | BadContainer => Fault::Corrupt,
            NotFormatted => Fault::Unformatted,
            _ => Fault::Other,
        }
    }
    pub fn message(self) -> &'static str {
        match self {
            Fault::NoCard => "No memory card in slot 1",
            Fault::Full => "Memory card full",
            Fault::Corrupt => "Memory card data damaged",
            Fault::Unformatted => "Memory card not formatted",
            Fault::Other => "Memory card error",
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
    pub charms_owned: u64,
    pub charms_equipped: u64,
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
    bytes.iter().fold(0x811c9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(16777619))
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
        out[52..60].copy_from_slice(&self.charms_owned.to_le_bytes());
        out[60..68].copy_from_slice(&self.charms_equipped.to_le_bytes());
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
        out[HKS4_FIELDS + 4..HKS4_FIELDS + 4 + persist::LEVELS].copy_from_slice(&self.player_levels);
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
    #[optimize(size)]
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
        if bytes.len() < len || fnv(&bytes[..len - 4]) != u32::from_le_bytes(bytes[len - 4..len].try_into().ok()?) {
            return None;
        }
        let word = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let long = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let items: &[u8] = if version == 5 { &bytes[ITEMS_AT..len - 4] } else { &[] };
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
        Some((Self {
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
            charms_owned: long(52),
            charms_equipped: long(60),
            charm_notches: bytes[68],
            can_overcharm: bytes[69] != 0,
            npc_conversations: u32::from_le_bytes(bytes[70..74].try_into().unwrap()),
            script_fields: core::array::from_fn(|slot| word(74 + slot * 4)),
            script_field_fnv: word(74 + SCRIPT_FIELD_SLOTS * 4) as u32,
            shop_slots: word(78 + SCRIPT_FIELD_SLOTS * 4) as u32,
            shop_counters: core::array::from_fn(|i| bytes[82 + SCRIPT_FIELD_SLOTS * 4 + i]),
            player_bools: if version == 5 { word(HKS4_FIELDS) as u32 } else { 0 },
            player_levels: if version == 5 {
                core::array::from_fn(|i| bytes[HKS4_FIELDS + 4 + i])
            } else {
                [0; persist::LEVELS]
            },
            version,
        }, items))
    }
    /// The item words of a decoded record's item bytes.
    pub fn items(raw: &[u8]) -> impl Iterator<Item = u32> + '_ {
        raw.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
    }
}
/// The hardware card with a pad checkpoint after every frame transfer.
struct PollingCard(HardwareCard);
impl Block for PollingCard {
    fn read_frame(&mut self, frame: u16, out: &mut [u8; FRAME_SIZE]) -> psx_mc::Result<()> {
        let r = self.0.read_frame(frame, out);
        crate::input::checkpoint();
        r
    }
    fn write_frame(&mut self, frame: u16, data: &[u8; FRAME_SIZE]) -> psx_mc::Result<()> {
        let r = self.0.write_frame(frame, data);
        crate::input::checkpoint();
        r
    }
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
#[inline(never)]
#[cfg_attr(not(test),optimize(size))]
pub fn write(profile: usize, save: &Save, items: &[u32]) -> Result<(), Fault> {
    let mut record = *save;
    let copy = unsafe { NEXT_COPY[profile] };
    record.sequence = unsafe { LAST_SEQUENCE[profile] }.wrapping_add(1);
    let mut name = [0u8; 19];
    let name = file_name(profile, copy, &mut name);
    let out = buf();
    let len = record.encode(items, out);
    let mut card = Card::new(PollingCard(HardwareCard::new(psx_mc::Slot::One)));
    let result = card.is_formatted().and_then(|formatted| {
        if !formatted { card.format()?; }
        card.write(name, TITLE, &out[..len])
    });
    unsafe {
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
                let fault = Fault::of(error);
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
#[optimize(size)]
pub fn survey() -> ([Slot; PROFILES], Option<Fault>) {
    let mut card = Card::new(HardwareCard::new(psx_mc::Slot::One));
    let mut slots = [Slot::Empty; PROFILES];
    let mut fault = None;
    for profile in 0..PROFILES {
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
    let mut card = Card::new(HardwareCard::new(psx_mc::Slot::One));
    let buf = buf();
    let read = card.read(name, buf);
    let decoded = read.as_ref().ok().and_then(|&len| Save::decode(&buf[..len]));
    match decoded {
        Some((save, items)) if save.sequence == sequence => {
            let mut installed = 0;
            for item in Save::items(items) {
                let (kind, scene, local, value) = persist::unpack(item).expect("validated by decode");
                installed += usize::from(store.set(kind, scene, local, value));
            }
            installed
        }
        _ => {
            let fault = match read { Err(error) => Fault::of(error), Ok(_) => Fault::Corrupt };
            unsafe { HK_SAVE_FAULT = fault as u32 + 1 }
            0
        }
    }
}
