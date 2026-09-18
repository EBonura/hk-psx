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
use psx_mc::{Block, Card, HardwareCard, Slot, FRAME_SIZE};

const NAMES: [&str; 2] = ["BASLUS-00000HKPSX000", "BASLUS-00000HKPSX001"];
const TITLE: &str = "HOLLOW KNIGHT PSX";
const MAGIC: [u8; 4] = *b"HKS1";
pub const LEN: usize = 56;
/// The slot the next write goes to, so it is never the one `read` returned.
static mut NEXT_SLOT: usize = 0;
/// Sequence of the newest save seen, so a later write always outranks it.
static mut LAST_SEQUENCE: u32 = 0;

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
/// Which of the two files was last read or written.
#[no_mangle]
pub static mut HK_SAVE_SLOT: u32 = 0;
/// Call only inside an input scene-load bracket (the sampler drops the
/// samples polled while the card is busy). Writes the slot `read` did not
/// return, so an interruption cannot destroy the save still being relied on.
#[inline(never)]
pub fn write(save: &Save) -> bool {
    let mut record = *save;
    let slot = unsafe { NEXT_SLOT };
    record.sequence = unsafe { LAST_SEQUENCE }.wrapping_add(1);
    let mut card = Card::new(PollingCard(HardwareCard::new(Slot::One)));
    let result = card.is_formatted().and_then(|formatted| {
        if !formatted { card.format()?; }
        card.write(NAMES[slot], TITLE, &record.encode())
    });
    unsafe {
        match result {
            Ok(()) => {
                HK_SAVE_WRITES = HK_SAVE_WRITES.saturating_add(1);
                LAST_SEQUENCE = record.sequence;
                NEXT_SLOT = 1 - slot;
            }
            Err(_) => HK_SAVE_ERRORS = HK_SAVE_ERRORS.saturating_add(1),
        }
        HK_SAVE_SLOT = slot as u32;
    }
    result.is_ok()
}
/// Boot-time read, before the pad sampler starts (nothing else uses SIO0).
/// Both slots are read; the valid record with the higher sequence wins and the
/// next write goes to the other one. None for no card, no file or two corrupt
/// records.
#[inline(never)]
pub fn read() -> Option<Save> {
    let mut card = Card::new(HardwareCard::new(Slot::One));
    let mut newest: Option<(usize, Save)> = None;
    for slot in 0..NAMES.len() {
        let mut buf = [0u8; 256];
        let Ok(len) = card.read(NAMES[slot], &mut buf) else { continue };
        let Some(save) = Save::decode(&buf[..len]) else { continue };
        // Sequences only ever increase here, so a plain comparison is enough.
        if newest.is_none_or(|(_, best)| save.sequence > best.sequence) {
            newest = Some((slot, save));
        }
    }
    let (slot, save) = newest?;
    unsafe {
        HK_SAVE_LOADED = 1;
        HK_SAVE_SLOT = slot as u32;
        LAST_SEQUENCE = save.sequence;
        NEXT_SLOT = 1 - slot;
    }
    Some(save)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_and_reject_corruption() {
        let save = Save { scene: 3, seat: [-65536, 720896], facing: -1, region: 7, geo: 41, door_hits: 13,
            shade: crate::shade::Record { present: true, scene: 2, position: [-131072, 65536], hp: 10, geo_pool: 77 },
            sequence: 9 };
        let bytes = save.encode();
        assert_eq!(Save::decode(&bytes), Some(save));
        for at in [24, 29, 33, 37, 41, 45, 49] {
            let mut bad = bytes;
            bad[at] ^= 1;
            assert_eq!(Save::decode(&bad), None, "byte {at} is outside the checksum");
        }
        assert_eq!(Save::decode(&bytes[..LEN - 1]), None);
    }
}
