//! Bounded tablet interaction state; independent of texture/region residency.
#[derive(Clone,Copy)]
pub struct ReadPoint {
    pub scene:usize,pub source_id:u32,pub polygon:&'static [[i32;2]],pub marker:[i32;2],
    pub pages:&'static [&'static [&'static str]],pub label:&'static str,
}
pub const UP:u16=0x10;pub const DOWN:u16=0x40;pub const CROSS:u16=0x4000;pub const CIRCLE:u16=0x2000;
const ACTIONS:u16=UP|DOWN|CROSS|CIRCLE|0x8000;
#[derive(Default,Debug,PartialEq,Eq)]pub struct Event {pub opened:bool,pub advanced:bool,pub closed:bool}
pub struct State {pub active:Option<usize>,pub prompt:Option<usize>,pub page:usize,previous:u16,release:bool}
impl State {
    pub const fn new()->Self{Self{active:None,prompt:None,page:0,previous:0,release:false}}
    pub fn open(&self)->bool{self.active.is_some()}
    pub fn consumes_actions(&self)->bool{self.open()||self.release}
    pub fn cancel(&mut self){self.active=None;self.prompt=None;self.page=0;self.release=self.previous&ACTIONS!=0;}
    pub fn step(&mut self,points:&[ReadPoint],scene:usize,body:[i32;4],eligible:bool,bits:u16)->Event {
        let pressed=bits & !self.previous;self.previous=bits;
        if bits&ACTIONS==0 {self.release=false;}
        let mut event=Event::default();
        if let Some(id)=self.active {
            if points[id].scene!=scene {self.cancel();event.closed=true;return event;}
            if pressed&CIRCLE!=0 {self.cancel();event.closed=true;}
            else if pressed&CROSS!=0 {
                if self.page+1<points[id].pages.len(){self.page+=1;event.advanced=true;}
                else{self.cancel();event.closed=true;}
            }
            return event;
        }
        self.prompt=if eligible && !self.release {points.iter().position(|p|p.scene==scene && hk_sim::polygon_hits_box(p.polygon,body))}else{None};
        if pressed&(UP|DOWN)!=0 {
            if let Some(id)=self.prompt {self.active=Some(id);self.page=0;event.opened=true;}
        }
        event
    }
}
#[cfg(test)]mod tests {
    use super::*;
    static POINTS:[ReadPoint;2]=[
      ReadPoint{scene:0,source_id:1,polygon:&[[0,0],[100,0],[100,100],[0,100]],marker:[50,150],pages:&[&["first"],&["second"]],label:"Inspect"},
      ReadPoint{scene:1,source_id:2,polygon:&[[0,0],[100,0],[100,100],[0,100]],marker:[50,150],pages:&[&["other"]],label:"Inspect"}];
    const BODY:[i32;4]=[20,20,30,30];
    #[test]fn fresh_up_requires_eligibility_and_true_polygon_overlap(){
      let mut s=State::new();assert!(!s.step(&POINTS,0,BODY,false,UP).opened);
      assert!(!s.step(&POINTS,0,BODY,true,UP).opened);s.step(&POINTS,0,BODY,true,0);
      assert!(s.step(&POINTS,0,BODY,true,UP).opened);assert_eq!(s.active,Some(0));
      s.cancel();s.step(&POINTS,0,[200,200,210,210],true,0);assert!(!s.step(&POINTS,0,[200,200,210,210],true,UP).opened);
    }
    #[test]fn pages_advance_on_edges_and_closing_consumes_held_buttons(){
      let mut s=State::new();s.step(&POINTS,0,BODY,true,UP);
      assert!(s.step(&POINTS,0,BODY,true,CROSS).advanced);assert_eq!(s.page,1);
      assert!(!s.step(&POINTS,0,BODY,true,CROSS).closed);s.step(&POINTS,0,BODY,true,0);
      assert!(s.step(&POINTS,0,BODY,true,CROSS).closed);assert!(s.consumes_actions());
      s.step(&POINTS,0,BODY,true,CROSS);assert!(s.consumes_actions());s.step(&POINTS,0,BODY,true,0);assert!(!s.consumes_actions());
    }
    #[test]fn cancel_damage_and_scene_change_release_modal_lock(){
      let mut s=State::new();s.step(&POINTS,0,BODY,true,DOWN);assert!(s.open());
      assert!(s.step(&POINTS,0,BODY,true,CIRCLE).closed);s.step(&POINTS,0,BODY,true,0);
      s.step(&POINTS,0,BODY,true,UP);assert!(s.step(&POINTS,1,BODY,true,0).closed);
      s.step(&POINTS,1,BODY,true,UP);assert_eq!(s.active,Some(1));s.cancel();assert!(!s.open());
    }
    #[test]fn attack_held_during_close_is_consumed_until_release(){
      let mut s=State::new();s.step(&POINTS,0,BODY,true,UP);s.step(&POINTS,0,BODY,true,0x8000);
      assert!(s.step(&POINTS,0,BODY,true,CIRCLE|0x8000).closed);s.step(&POINTS,0,BODY,true,0x8000);
      assert!(s.consumes_actions());s.step(&POINTS,0,BODY,true,0);assert!(!s.consumes_actions());
    }
    #[test]fn residency_and_ineligible_frames_do_not_restart_an_open_page(){
      let mut s=State::new();s.step(&POINTS,0,BODY,true,UP);s.step(&POINTS,0,BODY,true,CROSS);
      for _ in 0..10{s.step(&POINTS,0,BODY,false,0);assert_eq!(s.page,1);assert!(s.open());}
    }
}
