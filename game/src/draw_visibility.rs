//! Cached admission for immutable draw geometry with mutable visual state.
//! The three source state arrays remain authoritative. Only changed indices
//! need restoring before the next frame's gameplay bindings are applied.
use super::{MAX_DRAWS,DRAW_COUNT,DRAW_FLAGS,DRAW_COLORS,draw_record,VISIBLE,GAINS,OPACITIES,BLACK_MASKS,
    BLACK_AVERAGE,scenery_color,TILE_ELIGIBILITY_DIRTY,TILE_GENERATION,TILE_PREPARED,tile_eligibility_changed,vignette_recolor};
pub(super) const DRAW_ENABLED:u8=64;
const TOUCHED:u8=128;
static mut TOUCHED_DRAWS:[u16;MAX_DRAWS]=[0;MAX_DRAWS];
static mut TOUCHED_COUNT:usize=0;

/// Exact final state key. Untouched draws are at the documented defaults;
/// retaining order can cause a harmless miss, never a false state match.
#[inline]fn snapshot_word(index:usize)->u32 {
    unsafe {let draw=TOUCHED_DRAWS[index]as usize;
        ((draw as u32)<<17)|((u32::from(VISIBLE[draw/32]&(1<<(draw%32))!=0))<<16)
            |((GAINS[draw]as u32)<<8)|OPACITIES[draw]as u32}
}
pub(super) fn sparse_snapshot(out:&mut[u32;64])->Option<usize>{
    unsafe {if TOUCHED_COUNT>out.len(){return None;}
        for (i,word) in out[..TOUCHED_COUNT].iter_mut().enumerate(){*word=snapshot_word(i);}
        Some(TOUCHED_COUNT)}
}
pub(super) fn matches_snapshot(state:&[u32])->bool {
    unsafe {state.len()==TOUCHED_COUNT&&state.iter().enumerate().all(|(i,&word)|word==snapshot_word(i))}
}

/// Region activation already rewrites every active Draw, including its flags.
/// Drop the previous region's local indices before that rewrite and initialize
/// all legacy arrays once. Subsequent frame resets only visit changed draws.
pub(super) fn begin_region() {
    unsafe {
        TOUCHED_COUNT=0;
        VISIBLE=[u32::MAX;MAX_DRAWS/32];GAINS=[128;MAX_DRAWS];OPACITIES=[128;MAX_DRAWS];
    }
}
#[inline]
fn changed(draw:usize) {
    unsafe {
        if DRAW_FLAGS[draw]&TOUCHED==0 {
            assert!(TOUCHED_COUNT<MAX_DRAWS);
            TOUCHED_DRAWS[TOUCHED_COUNT]=draw as u16;TOUCHED_COUNT+=1;
            DRAW_FLAGS[draw]|=TOUCHED;
        }
    }
}
#[inline]
fn refresh(draw:usize) {
    unsafe {
        let enabled=VISIBLE[draw/32]&(1<<(draw%32))!=0&&GAINS[draw]!=0&&OPACITIES[draw]!=0;
        DRAW_FLAGS[draw]=(DRAW_FLAGS[draw]&!DRAW_ENABLED)|if enabled{DRAW_ENABLED}else{0};
    }
}
#[inline]
fn refresh_color(draw:usize) {
    unsafe {
        let opacity=OPACITIES[draw]as u16|if DRAW_FLAGS[draw]&BLACK_AVERAGE!=0 {256}else{0};
        DRAW_COLORS[draw]=scenery_color::command(0,draw_record(draw).tint(),GAINS[draw],opacity);
        vignette_recolor(draw);
    }
}
pub fn reset_visibility() {
    unsafe {
        // Keep the established tile-generation/invalidation protocol, even
        // when a draw was restored to its default value within this frame.
        if TILE_ELIGIBILITY_DIRTY {
            TILE_GENERATION=TILE_GENERATION.wrapping_add(1);
            (&mut *(&raw mut TILE_PREPARED)).invalidate();
            TILE_ELIGIBILITY_DIRTY=false;
        }
        for index in 0..TOUCHED_COUNT {
            let draw=TOUCHED_DRAWS[index]as usize;
            let color_changed=GAINS[draw]!=128||OPACITIES[draw]!=128;
            VISIBLE[draw/32]|=1<<(draw%32);GAINS[draw]=128;OPACITIES[draw]=128;
            DRAW_FLAGS[draw]=(DRAW_FLAGS[draw]&!TOUCHED)|DRAW_ENABLED;
            if color_changed {refresh_color(draw);}
        }
        TOUCHED_COUNT=0;
    }
}
pub fn set_visible(draw:usize,visible:bool) {
    unsafe {
        assert!(draw<DRAW_COUNT);let bit=1u32<<(draw%32);
        if (VISIBLE[draw/32]&bit!=0)==visible {return;}
        tile_eligibility_changed(draw);changed(draw);
        if visible {VISIBLE[draw/32]|=bit;}else{VISIBLE[draw/32]&=!bit;}
        refresh(draw);
    }
}
pub fn set_gain(draw:usize,gain:u8) {
    unsafe {
        assert!(draw<DRAW_COUNT);let gain=gain.min(128);
        if GAINS[draw]==gain {return;}
        if (GAINS[draw]==0)!=(gain==0) {tile_eligibility_changed(draw);}
        changed(draw);GAINS[draw]=gain;refresh(draw);refresh_color(draw);
    }
}
pub fn set_opacity(draw:usize,alpha128:u8) {
    unsafe {
        assert!(draw<DRAW_COUNT);let t=draw_record(draw).texture();
        assert!(BLACK_MASKS[t/32]&(1<<(t%32))!=0,"unsupported mask opacity palette");
        let alpha128=alpha128.min(128);
        if OPACITIES[draw]==alpha128 {return;}
        if (OPACITIES[draw]==128)!=(alpha128==128) {tile_eligibility_changed(draw);}
        changed(draw);OPACITIES[draw]=alpha128;refresh(draw);refresh_color(draw);
    }
}
