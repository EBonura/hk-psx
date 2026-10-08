//! The False Knight's art, as host/false_knight_art.py cooks it into its scene:
//! a `crate::boss_art::Bank` plus the geometry the fight reads beside it.
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/false_knight_art.rs"));

/// The cooked clips past `FalseyControl`'s own, in host/false_knight_art.py's
/// `EXTRA_CLIPS` order. A body clip's index is its `hk_sim` `Clip` value.
pub const BODY: usize = hk_sim::false_knight::CLIP_TICKS.len();
pub const HEAD_IDLE: usize = BODY + 1;
pub const HEAD_HIT: usize = BODY + 2;
pub const HEAD_SPAZ: usize = BODY + 3;
pub const DEATH_HEAD_1: usize = BODY + 4;
pub const DEATH_HEAD_2: usize = BODY + 5;
/// `Shockwave Spurt`, the slam wave's own clip, from its own library.
pub const SPURT: usize = BODY + 6;
const _: () = assert!(FK_ART_CLIPS.len() == SPURT + 1, "the cooked clip table and ArtClip disagree");

/// The bank itself; the free functions below keep the False Knight's call sites.
pub const BANK: crate::boss_art::Bank = crate::boss_art::Bank {
    anchor: FK_ART_ANCHOR_CLIP, sprites: &FK_ART_SPRITES, clips: &FK_ART_CLIPS, sequence: &FK_ART_SEQUENCE,
};
pub fn sprite(clip: usize, tick: u32) -> usize {
    BANK.sprite(clip, tick)
}
pub fn last_sprite(clip: usize) -> usize {
    BANK.last_sprite(clip)
}
/// `Shockwave Wave` and `Shockwave Spurt`, as the cook read them.
pub const WAVE: hk_sim::shockwave::Params = hk_sim::shockwave::Params {
    start_speed: FK_WAVE_START_SPEED, accel: FK_WAVE_ACCEL, wave_box: FK_WAVE_BOX,
    ground_ray: FK_WAVE_GROUND_RAY, spurt_box: FK_SPURT_BOX, damage_from: FK_SPURT_DAMAGE_FROM,
    damage_to: FK_SPURT_DAMAGE_TO, damage: FK_SPURT_DAMAGE, spurt_ticks: FK_SPURT_TICKS,
};
