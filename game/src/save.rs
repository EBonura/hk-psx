//! Bench Save Game on the port-1 memory card (P13 step 5 to 7, first slice).
//!
//! The record holds the bench respawn marker, Geo wallet, Great Door hit count
//! and the owed Shade (masks are a constant 5 here). Writes run between input
//! checkpoints so the pad sampler keeps polling (SIO0 is shared, so a card
//! frame and a pad poll never overlap). A missing or unformatted card is a
//! silent no-save; the rest still happens.
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
/// joined the record, and from HKS2 when the script runtime's PlayerData did. An
/// older file is both shorter and differently tagged, so it reads back as
/// Corrupt rather than decoding into a save whose notch board, conversations or
/// progression flags would be invented.
const MAGIC: [u8; 4] = *b"HKS3";
pub const LEN: usize = 146;
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
}
fn fnv(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(16777619))
}
impl Save {
    pub fn encode(&self) -> [u8; LEN] {
        let mut out = [0u8; LEN];
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
        let sum = fnv(&out[..LEN - 4]);
        out[LEN - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < LEN || bytes[0..4] != MAGIC
            || fnv(&bytes[..LEN - 4]) != u32::from_le_bytes(bytes[LEN - 4..LEN].try_into().ok()?) {
            return None;
        }
        let word = |at: usize| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let long = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        Some(Self {
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
        })
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
/// Call only inside an input scene-load bracket (the sampler drops the
/// samples polled while the card is busy). Writes the copy `read` did not
/// return, so an interruption cannot destroy the save still being relied on.
#[inline(never)]
pub fn write(profile: usize, save: &Save) -> Result<(), Fault> {
    let mut record = *save;
    let copy = unsafe { NEXT_COPY[profile] };
    record.sequence = unsafe { LAST_SEQUENCE[profile] }.wrapping_add(1);
    let mut name = [0u8; 19];
    let name = file_name(profile, copy, &mut name);
    let mut card = Card::new(PollingCard(HardwareCard::new(psx_mc::Slot::One)));
    let result = card.is_formatted().and_then(|formatted| {
        if !formatted { card.format()?; }
        card.write(name, TITLE, &record.encode())
    });
    unsafe {
        match result {
            Ok(()) => {
                HK_SAVE_WRITES = HK_SAVE_WRITES.saturating_add(1);
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
/// One profile's newer valid copy, and which copy the next write should use.
fn read_profile<B: Block>(card: &mut Card<B>, profile: usize) -> Result<Slot, Fault> {
    let mut newest: Option<(usize, Save)> = None;
    let mut seen = false;
    let mut fault = None;
    for copy in 0..2 {
        let mut name = [0u8; 19];
        let name = file_name(profile, copy, &mut name);
        let mut buf = [0u8; 256];
        match card.read(name, &mut buf) {
            Ok(len) => {
                seen = true;
                if let Some(save) = Save::decode(&buf[..len]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_reject_corruption() {
        let save = Save { scene: 3, seat: [-65536, 720896], facing: -1, region: 7, geo: 41, door_hits: 13,
            shade: crate::shade::Record { present: true, scene: 2, position: [-131072, 65536], hp: 10, geo_pool: 77 },
            sequence: 9, charms_owned: 0x0000_00ff_0000_1005, charms_equipped: 0x0000_0000_0000_1001,
            charm_notches: 5, can_overcharm: true, npc_conversations: 0x0000_0009,
            script_fields: core::array::from_fn(|slot| slot as i32 - 3),
            script_field_fnv: 0x1234_5678 };
        let bytes = save.encode();
        assert_eq!(Save::decode(&bytes), Some(save));
        // The last script slot and the field identity are the two fields most
        // likely to be left out of a layout change, so they are named here.
        for at in [24, 29, 33, 37, 41, 45, 49, 53, 61, 68, 69, 70, 73, 74,
                   74 + (SCRIPT_FIELD_SLOTS - 1) * 4, 74 + SCRIPT_FIELD_SLOTS * 4] {
            let mut bad = bytes;
            bad[at] ^= 1;
            assert_eq!(Save::decode(&bad), None, "byte {at} is outside the checksum");
        }
        assert_eq!(Save::decode(&bytes[..LEN - 1]), None);
        // An older record is both shorter and differently tagged.
        for tag in [b"HKS1", b"HKS2"] {
            let mut old = bytes;
            old[..4].copy_from_slice(tag);
            assert_eq!(Save::decode(&old), None);
        }
    }
    #[test]
    fn the_script_reserve_leaves_the_record_inside_one_card_block() {
        // A card file is one 8,192-byte block and the migrator refuses a record
        // that would leave it, so the reserve has to be checked against that
        // bound here rather than discovered when a fixture stops migrating.
        assert_eq!(LEN, 74 + SCRIPT_FIELD_SLOTS * 4 + 8);
        assert!(LEN < 8192);
    }
}
