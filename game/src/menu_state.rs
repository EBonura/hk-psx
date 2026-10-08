//! Bounded title menu state. Button bits match the SDK's active-high PS1 map.
//! Session settings are returned to gameplay; no save or memory-card claim.
pub const UP:u16=1<<4;
pub const RIGHT:u16=1<<5;
pub const DOWN:u16=1<<6;
pub const LEFT:u16=1<<7;
pub const START:u16=1<<3;
pub const CIRCLE:u16=1<<13;
pub const CROSS:u16=1<<14;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Settings {pub sfx:u8,pub ambience:u8,pub music:u8,pub cheats:crate::cheats::Settings}
impl Settings {
    pub const fn new()->Self{Self{sfx:10,ambience:10,music:10,cheats:crate::cheats::Settings::new()}}
    /// Rows0/1/2 adjust sound effects/ambience/music. Returns whether the value changed.
    pub fn adjust(&mut self,row:usize,delta:i8)->bool {
        let value=match row{0=>&mut self.sfx,1=>&mut self.ambience,2=>&mut self.music,_=>return false};
        let next=(*value as i16+delta as i16).clamp(0,10)as u8;
        let changed=next!=*value;*value=next;changed
    }
}
impl Default for Settings {fn default()->Self{Self::new()}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Page {Main,Options,Controls,Cheats,Profiles}
pub const MAIN_ITEMS:[&str;4]=["Start Game","Options","Controls","Cheats"];
const _:()=assert!(CHEAT_ITEMS.len()==crate::cheats::CHEAT_ROWS);
pub const CHEAT_ITEMS:[&str;12]=["Invincibility","Pure Nail (21 damage)","Infinite SOUL","Max masks (9)","Mothwing Cloak (L1 dash)","Mantis Claw (wall jump)","Monarch Wings (double jump)","Crystal Heart (hold R1)","Shade Cloak (dash i-frames)","Dream Nail (hold Triangle)","Vengeful Spirit (tap O)","All charms found"];
pub const OPTION_ITEMS:[&str;4]=["Sound effects","Ambience","Music","Back"];
pub const CONTROL_LINES:[&str;12]=[
    "D-pad: move","X: jump","Square: swing nail","Hold O: Focus / heal",
    "Up + Square: upward nail","Airborne Down + Square: downward nail",
    "Up/Down at tablet: inspect","Start: pause / resume","Select: reset session",
    // The abilities below are reachable only through Cheats until their
    // pickups exist, but the mapping is the shipped one.
    "L1: dash (Mothwing Cloak)","Hold R1: Crystal Heart","Hold Triangle: Dream Nail",
];
pub struct State {
    pub page:Page,pub selected:usize,pub settings:Settings,
    /// The profile the player confirmed on the save screen, once chosen.
    pub profile:Option<usize>,
    previous:u16,direction:u16,repeat:u8,
}
impl State {
    pub const fn new()->Self{Self{page:Page::Main,selected:0,settings:Settings::new(),profile:None,previous:0,direction:0,repeat:0}}
    /// Consume exactly one actual pad sample. Navigation repeats after20 held
    /// polls then every5 polls. Confirm/back require fresh edges. Confirm has
    /// priority over a simultaneous direction, preserving direct first START/X.
    pub fn step(&mut self,bits:u16)->bool {
        let edge=bits&!self.previous;self.previous=bits;
        let dir=bits&(UP|DOWN|LEFT|RIGHT);
        let dir=if dir.count_ones()==1{dir}else{0};
        let navigate=if dir!=self.direction {
            self.direction=dir;self.repeat=20;dir
        }else if dir!=0{
            self.repeat=self.repeat.saturating_sub(1);
            if self.repeat==0{self.repeat=5;dir}else{0}
        }else{0};
        if edge&CIRCLE!=0 {
            if self.page!=Page::Main {
                self.selected=match self.page{Page::Options=>1,Page::Cheats=>3,Page::Profiles=>0,_=>2};self.page=Page::Main;
            }
            return false;
        }
        if edge&(START|CROSS)!=0 {
            match self.page {
                Page::Main=>match self.selected {
                    // Start Game opens the save screen, as the source does;
                    // an empty profile begins a new game and a used one resumes.
                    0=>{self.page=Page::Profiles;self.selected=0;},
                    1=>{self.page=Page::Options;self.selected=0;},
                    2=>{self.page=Page::Controls;self.selected=0;},
                    _=>{self.page=Page::Cheats;self.selected=0;},
                },
                Page::Profiles=>{self.profile=Some(self.selected);return true;},
                Page::Options=>if self.selected==3{self.page=Page::Main;self.selected=1;},
                Page::Controls=>{self.page=Page::Main;self.selected=2;},
                Page::Cheats=>if self.selected==CHEAT_ITEMS.len(){self.page=Page::Main;self.selected=3;}else{
                    let delta=if self.settings.cheats.enabled(self.selected){-1}else{1};
                    self.settings.cheats.adjust(self.selected,delta);
                },
            }
            return false;
        }
        if self.page==Page::Controls{return false;}
        let count=match self.page{Page::Cheats=>CHEAT_ITEMS.len()+1,Page::Profiles=>crate::save::PROFILES,_=>4};
        if navigate==UP{self.selected=(self.selected+count-1)%count;}
        else if navigate==DOWN{self.selected=(self.selected+1)%count;}
        else if self.page==Page::Options {
            if navigate==LEFT{self.settings.adjust(self.selected,-1);}
            else if navigate==RIGHT{self.settings.adjust(self.selected,1);}
        }else if self.page==Page::Cheats{
            if navigate==LEFT{self.settings.cheats.adjust(self.selected,-1);}
            else if navigate==RIGHT{self.settings.cheats.adjust(self.selected,1);}
        }
        false
    }
}
