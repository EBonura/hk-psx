//! Edge-triggered pause navigation, independent of render cadence.
const UP:u16=0x10;const RIGHT:u16=0x20;const DOWN:u16=0x40;const LEFT:u16=0x80;
const CIRCLE:u16=0x2000;const CROSS:u16=0x4000;
pub struct State {pub row:usize,pub controls:bool,pub cheats:bool,pub action:crate::cheats::Action,previous:u16}
impl State {
    pub const fn new()->Self {Self {row:0,controls:false,cheats:false,action:crate::cheats::Action::None,previous:0}}
    pub fn enter(&mut self,bits:u16) {self.row=0;self.controls=false;self.cheats=false;self.action=crate::cheats::Action::None;self.previous=bits;}
    /// Return true to resume. The caller freezes gameplay while handling this.
    pub fn step(&mut self,bits:u16,sfx:&mut u8,ambience:&mut u8,cheats:&mut crate::cheats::Settings)->bool {
        self.action=crate::cheats::Action::None;
        let press=bits&!self.previous;self.previous=bits;
        if self.cheats {
            if press&CIRCLE!=0 {self.cheats=false;self.row=4;return false;}
            if press&UP!=0 {self.row=(self.row+7)%8;}
            if press&DOWN!=0 {self.row=(self.row+1)%8;}
            let delta=i8::from(press&RIGHT!=0)-i8::from(press&LEFT!=0);
            if delta!=0 {cheats.adjust(self.row,delta);}
            if press&CROSS!=0 {
                match self.row {
                    0..=3=>{let delta=if cheats.enabled(self.row){-1}else{1};cheats.adjust(self.row,delta);},
                    4=>self.action=crate::cheats::Action::Restore,
                    5=>self.action=crate::cheats::Action::AddBlue,
                    6=>{*cheats=crate::cheats::Settings::new();self.action=crate::cheats::Action::Reset;},
                    _=>{self.cheats=false;self.row=4;},
                }
            }
            return false;
        }
        if self.controls {
            if press&(CIRCLE|CROSS)!=0 {self.controls=false;}
            return false;
        }
        if press&CIRCLE!=0 {return true;}
        if press&UP!=0 {self.row=(self.row+4)%5;}
        if press&DOWN!=0 {self.row=(self.row+1)%5;}
        let delta=i16::from(press&RIGHT!=0)-i16::from(press&LEFT!=0);
        let value=match self.row {1=>Some(sfx),2=>Some(ambience),_=>None};
        if let Some(value)=value {*value=(i16::from(*value)+delta).clamp(0,10)as u8;}
        if press&CROSS!=0 {
            if self.row==0 {return true;}
            if self.row==3 {self.controls=true;}
            if self.row==4 {self.cheats=true;self.row=0;}
        }
        false
    }
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn held_inputs_do_not_repeat_and_volume_clamps() {
        let mut p=State::new();let(mut a,mut b)=(10,0);
        assert!(!p.step(DOWN,&mut a,&mut b,&mut crate::cheats::Settings::new()));assert_eq!(p.row,1);
        p.step(DOWN,&mut a,&mut b,&mut crate::cheats::Settings::new());assert_eq!(p.row,1);
        p.step(RIGHT,&mut a,&mut b,&mut crate::cheats::Settings::new());assert_eq!(a,10);
        p.step(LEFT,&mut a,&mut b,&mut crate::cheats::Settings::new());assert_eq!(a,9);
        p.step(LEFT,&mut a,&mut b,&mut crate::cheats::Settings::new());assert_eq!(a,9);
        p.step(DOWN,&mut a,&mut b,&mut crate::cheats::Settings::new());assert_eq!(p.row,2);
        p.step(LEFT,&mut a,&mut b,&mut crate::cheats::Settings::new());assert_eq!(b,0);
    }
    #[test]fn controls_back_and_resume_are_separate() {
        let mut p=State::new();let(mut a,mut b)=(10,10);
        p.step(UP,&mut a,&mut b,&mut crate::cheats::Settings::new());p.step(0,&mut a,&mut b,&mut crate::cheats::Settings::new());p.step(UP,&mut a,&mut b,&mut crate::cheats::Settings::new());assert_eq!(p.row,3);
        assert!(!p.step(CROSS,&mut a,&mut b,&mut crate::cheats::Settings::new()));assert!(p.controls);
        assert!(!p.step(CIRCLE,&mut a,&mut b,&mut crate::cheats::Settings::new()));assert!(!p.controls);
        p.step(0,&mut a,&mut b,&mut crate::cheats::Settings::new());assert!(p.step(CIRCLE,&mut a,&mut b,&mut crate::cheats::Settings::new()));
        p.enter(CROSS);assert!(!p.step(CROSS,&mut a,&mut b,&mut crate::cheats::Settings::new()));
    }
}
