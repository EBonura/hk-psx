//! Gruz Mother's art and geometry, as host/false_knight_art.py's Gruz Mother
//! section cooks them into Crossroads_04: a `crate::boss_art::Bank` whose clips
//! are `hk_sim::gruz_mother::Clip` in order.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/gruz_art.rs"));

const _: () = assert!(
    GZ_ART_CLIPS.len() == hk_sim::gruz_mother::CLIPS,
    "the cooked clip table and hk_sim::gruz_mother::Clip disagree"
);

pub const BANK: crate::boss_art::Bank = crate::boss_art::Bank {
    anchor: GZ_ART_ANCHOR_CLIP,
    sprites: &GZ_ART_SPRITES,
    clips: &GZ_ART_CLIPS,
    sequence: &GZ_ART_SEQUENCE,
};
