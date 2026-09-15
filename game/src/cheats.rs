//! Optional session cheats. Disabled settings delegate to normal gameplay.
//! Toggles survive death/Select reset; only Reset All turns them off. Disabling
//! max masks clamps excess red health; earned blue masks remain independent.
use hk_sim::{Hurt,VitalParams,Vitals};
// Windows PlayerData.AddGGPlayerDataOverrides sets nailDamage=21 and upgrades=4.
pub const PURE_NAIL_DAMAGE:u16=21;
pub const MAX_MASKS:u16=9;
pub const BLUE_CHEAT_CAP:u16=20;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Settings {pub invincible:bool,pub max_nail:bool,pub infinite_soul:bool,pub max_masks:bool}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Action {None,Restore,AddBlue,Reset}
impl Settings {
    pub const fn new()->Self {Self{invincible:false,max_nail:false,infinite_soul:false,max_masks:false}}
    pub fn enabled(&self,row:usize)->bool {match row {0=>self.invincible,1=>self.max_nail,2=>self.infinite_soul,3=>self.max_masks,_=>false}}
    pub fn adjust(&mut self,row:usize,delta:i8) {
        if delta==0{return;}
        let value=match row {0=>&mut self.invincible,1=>&mut self.max_nail,2=>&mut self.infinite_soul,3=>&mut self.max_masks,_=>return};
        *value=delta>0;
    }
    pub fn bits(self)->u32 {u32::from(self.invincible)|(u32::from(self.max_nail)<<1)|(u32::from(self.infinite_soul)<<2)|(u32::from(self.max_masks)<<3)}
    pub fn params(self,mut base:VitalParams)->VitalParams {
        if self.max_nail {base.nail_damage=PURE_NAIL_DAMAGE;}
        if self.max_masks {base.max_health=MAX_MASKS;}
        base
    }
    pub fn new_vitals(self,base:VitalParams)->Vitals {
        let mut v=Vitals::new(self.params(base));self.maintain(&mut v,base);v
    }
    /// Called before/after consumption so Focus retains its ordinary timing
    /// and healing rules, while the displayed resource stays full.
    pub fn maintain(self,v:&mut Vitals,base:VitalParams) {
        if self.infinite_soul&&!v.dead {v.soul=base.max_soul;}
    }
    pub fn apply(self,previous:Self,v:&mut Vitals,base:VitalParams,action:Action) {
        let p=self.params(base);
        if self.max_masks!=previous.max_masks&&!v.dead {
            if self.max_masks {v.health=p.max_health;}else{v.health=v.health.min(p.max_health);}
        }
        if !v.dead {match action {
            Action::Restore=>{v.health=p.max_health;v.soul=p.max_soul;},
            // Do not reduce blue health already obtained from normal gameplay.
            Action::AddBlue=>{if v.blue_health<BLUE_CHEAT_CAP {v.blue_health=v.blue_health.saturating_add(5).min(BLUE_CHEAT_CAP);}},
            _=>{},
        }}
        self.maintain(v,base);
    }
    pub fn hurt(self,v:&mut Vitals,base:VitalParams,damage:u16,direction:i32,hazard:bool)->Hurt {
        if !self.invincible {return v.hurt(self.params(base),damage,direction,hazard);}
        if !hazard||damage==0||v.dead||v.hazard_pending {return Hurt::Ignored;}
        // Invulnerability must not strand the Knight below the room or in
        // spikes: keep the existing safe-position recovery, without damage.
        v.recoil_direction=direction.clamp(-1,1);v.hazard_pending=true;
        v.freeze_ticks=0;v.recoil_ticks=0;Hurt::Hazard
    }
}
#[no_mangle]pub static mut HK_CHEATS:u32=0;
#[no_mangle]pub static mut HK_CHEAT_NAIL_DAMAGE:u32=0;
#[no_mangle]pub static mut HK_CHEAT_MASK_CAP:u32=0;
pub fn publish(settings:Settings,base:VitalParams) {
    let p=settings.params(base);
    unsafe {HK_CHEATS=settings.bits();HK_CHEAT_NAIL_DAMAGE=p.nail_damage as u32;HK_CHEAT_MASK_CAP=p.max_health as u32;}
}
