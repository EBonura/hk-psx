//! Brooding Mawlek's art and geometry, as host/mawlek_art.py cooks them into
//! Crossroads_09: a `crate::boss_art::Bank` whose first clips are
//! `hk_sim::mawlek::Clip` in order, then the extras below.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/mawlek_art.rs"));

pub const CLIPS: usize = hk_sim::mawlek::CLIP_TICKS.len();
/// `Shot Mawlek NoDrip`'s `Idle` and `Impact`, the `Spit Effect`'s `Enemy
/// Shot`, and the corpse's `Dummy Roar` at the corpse's own scale.
pub const SHOT: usize = CLIPS;
pub const SHOT_IMPACT: usize = CLIPS + 1;
pub const SPIT: usize = CLIPS + 2;
pub const CORPSE: usize = CLIPS + 3;
const _: () = assert!(MW_ART_CLIPS.len() == CORPSE + 1, "the cooked clip table and hk_sim::mawlek::Clip disagree");

pub const BANK: crate::boss_art::Bank = crate::boss_art::Bank {
    anchor: MW_ART_ANCHOR_CLIP, sprites: &MW_ART_SPRITES, clips: &MW_ART_CLIPS, sequence: &MW_ART_SEQUENCE,
};
