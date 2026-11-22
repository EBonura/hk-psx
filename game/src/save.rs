//! Bench Save Game on the port-1 memory card (P13 step 5 to 7).
//!
//! The record holds the bench respawn marker, Geo wallet, Great Door hit count
//! and the owed Shade in a fixed header, then PlayerData (`player_data.rs`: the
//! charm board, NPC conversation cursors, the script bank's fields, Sly's half,
//! abilities, map and boss flags, spell levels) as named fields, then SceneData
//! (`persist.rs`): bosses beaten, walls broken, cocoons opened, secrets
//! revealed, Geo rocks mined. docs/SAVE.md has the layout. Writes run
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
/// HKS3 when Sly's half of PlayerData did, from HKS4 when the world state did,
/// and from HKS5 when the six PlayerData stores became one (`player_data.rs`).
/// HKS1 to HKS3 read back as Corrupt rather than decoding into a save whose
/// notch board, conversations, progression flags or shop purchases would be
/// invented.
///
/// HKS4 and HKS5 still load: `migrate` puts every value they carried on its
/// named field, exactly as the HKS5 guest restored it (tests/save_runtime.rs
/// loads every committed fixture both ways). HKS6 is the last format that has to
/// grow by a version: a field joining PlayerData is an id the older records do
/// not mention, which reads as its default.
const MAGIC: [u8; 4] = *b"HKS6";
const MAGIC_HKS5: [u8; 4] = *b"HKS5";
const MAGIC_HKS4: [u8; 4] = *b"HKS4";
pub const LEN_HKS4: usize = 156;
/// HKS4's fields end here, where HKS4 put its checksum; HKS5 kept every one of
/// them at the same offset.
const HKS4_FIELDS: usize = LEN_HKS4 - 4;
/// HKS5's `persist` bools (4), levels (4) and SceneData item count (2), then the
/// items, then the checksum.
const HKS5_ITEMS_AT: usize = HKS4_FIELDS + 4 + persist::LEVELS + 2;
/// The shortest HKS5 record, a world nobody touched.
pub const LEN_HKS5: usize = 166;
/// HKS6: the header (the fields that are the frame's rather than PlayerData's:
/// the respawn, the wallet, the Great Door, the Shade and the sequence, at
/// HKS5's offsets), the PlayerData section (`player_data::PlayerData::encode`),
/// the SceneData item count and items, then the checksum.
const HEADER: usize = 52;
/// The shortest HKS6 record: every field at its default and no items.
/// `tools/migrate_cards.py` reads this.
pub const LEN: usize = 60;
const _: () = assert!(LEN == HEADER + 2 + 2 + 4);
/// The longest, with every field set and the store full. The psx-mc container
/// ahead of it and the title and icon frames still fit the file's one block.
pub const MAX_LEN: usize = HEADER + player_data::MAX_SECTION + 2 + persist::MAX_ITEMS * 4 + 4;
const _: () = assert!(MAX_LEN >= LEN_HKS5 + persist::MAX_ITEMS * 4 && 16 + MAX_LEN <= 8192 - 256);
use crate::persist;
use crate::player_data::{self, PlayerData};
/// HKS4 and HKS5's shop counters (`shop::State::record`).
pub const SHOP_COUNTERS: usize = 6;
/// HKS4 and HKS5's script reserve: PlayerData slots in the cooked bank's order,
/// with the bank's field-list identity after them. `host/cook_scripts.py` still
/// reads this as the most fields a bank may cook; the guest binds each one to
/// its `player_data` field by name (`script.rs`), so the number no longer sizes
/// anything on the card.
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
    /// The tag this record was read from, 4, 5 or 6; `encode` always writes 6.
    /// PlayerData and the SceneData items are not part of this struct: they go
    /// straight between the card buffer and their stores (`load`), because four
    /// profiles of them would cost the boot survey a kilobyte of stack for
    /// records only one of which is ever loaded.
    pub version: u8,
}
fn fnv(bytes: &[u8]) -> u32 {
    psx_pack::fnv1a32(bytes)
}
/// What a migrated record needs from the build reading it: the cooked script
/// bank's fields in slot order and its field-list identity (an HKS4/HKS5 record's
/// reserve is that bank's or is dropped, as the HKS5 guest did), the False
/// Knight's arena scene for an HKS4 record, and the shop's own units.
pub struct Build<'a> {
    pub script_fields: &'a [player_data::Field],
    pub script_fnv: u32,
    pub false_knight_scene: Option<usize>,
    pub starting_masks: u8,
    pub soul_per_vessel: u16,
}
/// What `migrate` restored, for the loader's telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Restored {
    pub items: usize,
    /// Whether the script fields were taken: always for HKS6, and for an older
    /// record only when its reserve was written by this bank.
    pub scripts: bool,
}
impl Save {
    /// Write the record, `pd` and `items` (sorted `persist` words) into `out`,
    /// and return the record's length. `out` must hold `MAX_LEN` bytes.
    pub fn encode(&self, pd: &PlayerData, items: &[u32], out: &mut [u8]) -> usize {
        assert!(items.len() <= persist::MAX_ITEMS && out.len() >= MAX_LEN);
        out[..MAX_LEN].fill(0);
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
        let mut at = HEADER + pd.encode(&mut out[HEADER..]);
        out[at..at + 2].copy_from_slice(&(items.len() as u16).to_le_bytes());
        at += 2;
        for item in items {
            out[at..at + 4].copy_from_slice(&item.to_le_bytes());
            at += 4;
        }
        let sum = fnv(&out[..at]);
        out[at..at + 4].copy_from_slice(&sum.to_le_bytes());
        at + 4
    }
    /// The record, its PlayerData bytes (the HKS6 section, or for HKS4 and
    /// HKS5 the whole record, whose fields `migrate` reads by offset) and its
    /// SceneData item bytes (four per item, validated and sorted), or None.
    #[inline(never)]
    #[cfg_attr(not(test), optimize(size))]
    pub fn decode(bytes: &[u8]) -> Option<(Self, &[u8], &[u8])> {
        let magic = bytes.get(0..4)?;
        let (version, items_at) = if magic == MAGIC_HKS4 {
            (4, LEN_HKS4 - 4)
        } else if magic == MAGIC_HKS5 && bytes.len() >= LEN_HKS5 {
            (5, HKS5_ITEMS_AT)
        } else if magic == MAGIC {
            (6, HEADER + PlayerData::check(bytes.get(HEADER..)?)? + 2)
        } else {
            return None;
        };
        let count = if version == 4 {
            0
        } else {
            u16::from_le_bytes([*bytes.get(items_at - 2)?, *bytes.get(items_at - 1)?]) as usize
        };
        if count > persist::MAX_ITEMS {
            return None;
        }
        let len = items_at + count * 4 + 4;
        if bytes.len() < len
            || fnv(&bytes[..len - 4]) != u32::from_le_bytes(bytes[len - 4..len].try_into().ok()?)
        {
            return None;
        }
        let word = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let items = &bytes[items_at..len - 4];
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
        let player = if version == 6 {
            &bytes[HEADER..items_at - 2]
        } else {
            &bytes[..items_at]
        };
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
                version,
            },
            player,
            items,
        ))
    }
    /// The item words of a decoded record's item bytes.
    pub fn items(raw: &[u8]) -> impl Iterator<Item = u32> + '_ {
        raw.chunks_exact(4)
            .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
    }
}
/// The HKS4/HKS5 fields `migrate` copies to a PlayerData field as they are:
/// (offset, whether a word or a byte, field). The charm board, the NPC cursors
/// and the shop counters.
const LEGACY: [(u8, bool, u8); 13] = {
    use player_data::*;
    [
        (52, true, PORT_CHARMS_OWNED as u8),
        (56, true, PORT_CHARMS_OWNED as u8 + 1),
        (60, true, PORT_CHARMS_EQUIPPED as u8),
        (64, true, PORT_CHARMS_EQUIPPED as u8 + 1),
        (68, false, CHARM_SLOTS as u8),
        (69, false, CAN_OVERCHARM as u8),
        (70, true, PORT_NPC_CURSORS as u8),
        (146, false, HEART_PIECES as u8),
        (147, false, VESSEL_FRAGMENTS as u8),
        (148, false, MAX_HEALTH_BASE as u8),
        (149, false, MP_RESERVE_MAX as u8),
        (150, false, SIMPLE_KEYS as u8),
        (151, false, RANCID_EGGS as u8),
    ]
};
const _: () = assert!(
    82 + SCRIPT_FIELD_SLOTS * 4 == 146
        && 146 + SHOP_COUNTERS == HKS4_FIELDS
        && player_data::COUNT <= 256
);
/// A decoded record's PlayerData into `pd` and its items into `store`. An HKS6
/// record is its own section. An HKS4 or HKS5 one is migrated: every field it
/// carried goes to the named field the HKS5 guest restored it to, and what it
/// never carried stays a new game's.
///
/// - The charm board (bytes 52 to 70), the NPC cursors (70) and Sly's half
///   (142 to 152) are copied across; `charms::boot` and `shop::boot` still judge
///   whether they agree with each other, as they did.
/// - The script reserve (74 to 138, its field-list identity at 138) goes to the
///   fields the running bank names in those slots, only when the identity is
///   this bank's; otherwise it is dropped, which is what the HKS5 guest did.
/// - HKS5's `persist` bits (152) and levels (156) go to their fields.
/// - HKS4 kept the False Knight's arena `Activated` and `falseKnightFirstPlop`
///   in the reserve's top two slots; they become the bits and the arena's
///   SceneData item, as HKS5's `persist::migrate_hks4` did.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn migrate(
    save: &Save,
    player: &[u8],
    items: &[u8],
    build: &Build,
    pd: &mut PlayerData,
    store: &mut persist::Store,
) -> Restored {
    let mut installed = 0;
    for item in Save::items(items) {
        let (kind, scene, local, value) = persist::unpack(item).expect("validated by decode");
        installed += usize::from(store.set(kind, scene, local, value));
    }
    if save.version == 6 {
        pd.decode(player);
        return Restored { items: installed, scripts: true };
    }
    let b = player;
    let word = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let p = pd;
    p.reset();
    // The fields that copy straight across, as a table: the guest runs this once
    // per boot and the RAM it would cost unrolled is not worth it.
    for &(at, wide, field) in &LEGACY {
        let at = at as usize;
        let value = if wide { word(at) } else { u32::from(b[at]) };
        p.set(field as usize, value as i32);
    }
    // In the source's own units: masks, and SOUL rather than vessels.
    p.set(
        player_data::MAX_HEALTH_BASE,
        p.get(player_data::MAX_HEALTH_BASE) + i32::from(build.starting_masks),
    );
    p.set(
        player_data::MP_RESERVE_MAX,
        p.get(player_data::MP_RESERVE_MAX) * i32::from(build.soul_per_vessel),
    );
    p.put(player_data::CAN_OVERCHARM, p.flag(player_data::CAN_OVERCHARM));
    let shop_slots = word(78 + SCRIPT_FIELD_SLOTS * 4);
    if shop_slots >> player_data::SHOP_FLAGS.len() != 0 {
        // Bits past the cooked flags: `shop::State::restore` refused the whole
        // half and the shelf started afresh, so that is what is migrated.
        for &(_, _, field) in &LEGACY[7..] {
            p.set(field as usize, player_data::default(field as usize));
        }
    } else {
        for (slot, &field) in player_data::SHOP_FLAGS.iter().enumerate() {
            p.set(field, (shop_slots >> slot & 1) as i32);
        }
    }
    let slot = |s: usize| word(74 + s * 4) as i32;
    let scripts = word(74 + SCRIPT_FIELD_SLOTS * 4) == build.script_fnv;
    if scripts {
        // HKS4's top two slots were the False Knight's, never the bank's.
        let usable = if save.version == 4 {
            SCRIPT_FIELD_SLOTS - 2
        } else {
            SCRIPT_FIELD_SLOTS
        };
        for (s, &field) in build.script_fields.iter().enumerate().take(usable) {
            p.set(field, slot(s));
        }
    }
    if save.version == 5 {
        p.set_legacy_bits(word(HKS4_FIELDS));
        for i in 0..persist::LEVELS {
            p.set(player_data::FIREBALL_LEVEL + i, i32::from(b[HKS4_FIELDS + 4 + i]));
        }
    } else if scripts {
        if slot(SCRIPT_FIELD_SLOTS - 2) != 0 {
            p.put(player_data::FALSE_KNIGHT_FIRST_PLOP, true);
        }
        if slot(SCRIPT_FIELD_SLOTS - 1) != 0 {
            p.put(player_data::FALSE_KNIGHT_DEFEATED, true);
            if let Some(scene) = build.false_knight_scene {
                installed += usize::from(store.set(persist::Kind::BattleScene, scene, 0, 1));
            }
        }
    }
    Restored { items: installed, scripts }
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
/// `pd` is PlayerData and `items` `persist`'s SceneData list, both already
/// brought up to date (`player_data::snapshot`, `persist::snapshot`).
/// An unformatted card is refused with `Fault::Unformatted` unless `format` is
/// set: the player is asked first, as the console's own card manager does.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn write(
    profile: usize,
    save: &Save,
    pd: &PlayerData,
    items: &[u32],
    format: bool,
) -> Result<(), Fault> {
    let mut record = *save;
    let copy = unsafe { NEXT_COPY[profile] };
    record.sequence = unsafe { LAST_SEQUENCE[profile] }.wrapping_add(1);
    let mut name = [0u8; 19];
    let name = file_name(profile, copy, &mut name);
    let out = buf();
    let len = record.encode(pd, items, out);
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
                if let Some((save, _, _)) = Save::decode(&buf[..len]) {
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
/// The chosen profile's PlayerData into `pd` and its SceneData items into
/// `store`, migrating an HKS4 or HKS5 record. Re-reads the copy `survey`
/// picked rather than keeping every profile's from the survey, which would hold
/// four records to use one. Boot-time, after the menu and before the pad
/// sampler starts, the same window `survey` runs in.
///
/// A read that fails, or finds a different record than the survey did (the card
/// was swapped at the title), installs nothing and says so in `HK_SAVE_FAULT`:
/// the header still loads, with a new game's PlayerData and the authored world.
#[inline(never)]
#[cfg_attr(not(test), optimize(size))]
pub fn load(
    profile: usize,
    sequence: u32,
    build: &Build,
    pd: &mut PlayerData,
    store: &mut persist::Store,
) -> Restored {
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
        Some((save, player, items)) if save.sequence == sequence => {
            migrate(&save, player, items, build, pd, store)
        }
        _ => {
            let fault = match read {
                Err(error) => Fault::of(error),
                Ok(_) => Fault::Corrupt,
            };
            unsafe { HK_SAVE_FAULT = fault as u32 + 1 }
            Restored { items: 0, scripts: false }
        }
    }
}
