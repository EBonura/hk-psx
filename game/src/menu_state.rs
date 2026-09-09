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
pub struct Settings {pub sfx:u8,pub ambience:u8}
impl Settings {
    pub const fn new()->Self{Self{sfx:10,ambience:10}}
    /// Rows0/1 adjust sound effects/ambience. Returns whether the value changed.
    pub fn adjust(&mut self,row:usize,delta:i8)->bool {
        let value=match row{0=>&mut self.sfx,1=>&mut self.ambience,_=>return false};
        let next=(*value as i16+delta as i16).clamp(0,10)as u8;
        let changed=next!=*value;*value=next;changed
    }
}
impl Default for Settings {fn default()->Self{Self::new()}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Page {Main,Options,Controls}
pub const MAIN_ITEMS:[&str;3]=["Start Game","Options","Controls"];
pub const OPTION_ITEMS:[&str;3]=["Sound effects","Ambience","Back"];
pub const CONTROL_LINES:[&str;9]=[
    "D-pad: move","X: jump","Square: swing nail","Hold O: Focus / heal",
    "Up + Square: upward nail","Airborne Down + Square: downward nail",
    "Up/Down at tablet: inspect","Start: pause / resume","Select: reset session",
];
pub struct State {
    pub page:Page,pub selected:usize,pub settings:Settings,
    previous:u16,direction:u16,repeat:u8,
}
impl State {
    pub const fn new()->Self{Self{page:Page::Main,selected:0,settings:Settings::new(),previous:0,direction:0,repeat:0}}
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
                self.selected=if self.page==Page::Options{1}else{2};self.page=Page::Main;
            }
            return false;
        }
        if edge&(START|CROSS)!=0 {
            match self.page {
                Page::Main=>match self.selected {
                    0=>return true,
                    1=>{self.page=Page::Options;self.selected=0;},
                    _=>{self.page=Page::Controls;self.selected=0;},
                },
                Page::Options=>if self.selected==2{self.page=Page::Main;self.selected=1;},
                Page::Controls=>{self.page=Page::Main;self.selected=2;},
            }
            return false;
        }
        if self.page==Page::Controls{return false;}
        if navigate==UP{self.selected=(self.selected+2)%3;}
        else if navigate==DOWN{self.selected=(self.selected+1)%3;}
        else if self.page==Page::Options {
            if navigate==LEFT{self.settings.adjust(self.selected,-1);}
            else if navigate==RIGHT{self.settings.adjust(self.selected,1);}
        }
        false
    }
}
