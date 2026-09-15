//! Source Great Door damage stages and session persistence. The transition
//! departure follows two source NextFrame events after an instant blackout.
//! The 2.5 second delay belongs to the destination entry, not this departure.
use hk_format::{Room,i32_at,u32_at};
#[derive(Clone,Copy)]
pub struct Params {pub stage_hits:[u8;3],pub cooldown:u8,pub transition:u16,pub entry_delay:u16}
#[derive(Clone,Copy)]
pub struct Entry {pub spawn:[i32;2],pub settle:u16,pub fade_delay:u16,pub fade:u16,pub lead:u16,pub walk:u16,pub speed:i32}
#[derive(Clone,Copy)]
pub struct Binding {pub frames:[u16;3],pub edges:&'static [u16]}
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/great_door.rs"));
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
pub struct Strike {pub hit:bool,pub opened:bool}
pub struct World {hits:u8,cooldown:u8,wait:u16,transitions:u16,entry:u16,entered:bool}
#[no_mangle]pub static mut HK_GREAT_DOOR_HITS:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_STAGE:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_OPENED:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_WAIT:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_ENTRY_WAIT:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_TRANSITIONS:u32=0;
impl World {
    pub const fn new()->Self {Self {hits:0,cooldown:0,wait:0,transitions:0,entry:0,entered:false}}
    pub fn reset(&mut self) {*self=Self::new();self.publish();}
    pub fn opened(&self)->bool {self.hits>=PARAMS.stage_hits[2]}
    pub fn pending(&self)->bool {self.wait!=0 || self.entry!=0 || (self.transitions!=0&&!self.entered)}
    pub fn blackout(&self)->bool {self.shade()==255}
    pub fn entering(&self)->bool {self.entry!=0}
    pub fn entry_footsteps(&self)->bool {self.entry!=0&&self.entry<=ENTRY.lead+ENTRY.walk}
    pub fn forced_direction(&self)->i32 {i32::from(self.entry!=0&&self.entry<=ENTRY.walk)}
    pub fn shade(&self)->u8 {
        if self.wait!=0 || (self.transitions!=0&&!self.entered) {return 255;}
        if self.entry==0 {return 0;}
        let elapsed=ENTRY.settle+PARAMS.entry_delay+ENTRY.lead+ENTRY.walk-self.entry;
        let fade_elapsed=elapsed.saturating_sub(ENTRY.settle+ENTRY.fade_delay);
        255-((fade_elapsed.min(ENTRY.fade)as u32*255)/ENTRY.fade as u32)as u8
    }
    /// Apply after tick(), instead of ordinary input/gravity/collision motion.
    /// Source transition velocity is horizontal and gravity remains disabled.
    pub fn apply_entry(&self,player:&mut hk_sim::Player)->bool {
        if !self.entering() {return false;}
        let dir=self.forced_direction();player.x+=dir*ENTRY.speed/60;
        player.vy=0;player.grounded=true;player.facing=1;player.jumping=false;
        player.jump_tick=0;player.land_tick=0;
        let animation=dir as u16;
        if player.animation!=animation {player.animation=animation;player.animation_tick=0;}
        else {player.animation_tick=player.animation_tick.wrapping_add(1);}
        true
    }
    /// Call only after the successful Great Door destination spawn. Repeated
    /// scene activations must not restart this session's one entry delay.
    pub fn begin_entry(&mut self,scene:usize) {
        if scene==TARGET_SCENE && self.transitions==1 && !self.entered {
            self.entered=true;self.entry=ENTRY.settle+PARAMS.entry_delay+ENTRY.lead+ENTRY.walk;self.publish();
        }
    }
    fn stage(&self)->usize {if self.hits>=PARAMS.stage_hits[1]{2}else if self.hits>=PARAMS.stage_hits[0]{1}else{0}}
    fn publish(&self) {unsafe {
        HK_GREAT_DOOR_HITS=self.hits as u32;HK_GREAT_DOOR_STAGE=self.stage()as u32;
        HK_GREAT_DOOR_OPENED=u32::from(self.opened());HK_GREAT_DOOR_WAIT=self.wait as u32;
        HK_GREAT_DOOR_TRANSITIONS=self.transitions as u32;HK_GREAT_DOOR_ENTRY_WAIT=self.entry as u32;
    }}
    pub fn tick(&mut self,scene:usize)->bool {
        if scene==TARGET_SCENE {
            self.entry=self.entry.saturating_sub(1);self.publish();return false;
        }
        if scene!=0 {return false;}
        self.cooldown=self.cooldown.saturating_sub(1);
        let due=self.wait==1;self.wait=self.wait.saturating_sub(1);
        if due {self.transitions=self.transitions.saturating_add(1);}
        self.publish();due
    }
    pub fn strike(&mut self,scene:usize,polygon:&[[i32;2]])->Strike {
        if scene!=0||self.opened()||self.cooldown!=0||!hk_sim::polygon_hits_box(polygon,BOUNDS){return Strike::default();}
        self.hits+=1;self.cooldown=PARAMS.cooldown;
        let opened=self.opened();if opened {self.wait=PARAMS.transition;}
        self.publish();Strike {hit:true,opened}
    }
    pub fn frame(&self,region:usize,camera:(i32,i32))->Option<usize> {
        if self.opened() {return None;}
        let binding=REGIONS.get(region)?.as_ref()?;
        // Conservative world bounds for all three source sprite poses.
        if (camera.0-POSITION[0]).abs()>20*65536 ||(camera.1-POSITION[1]).abs()>18*65536{return None;}
        Some(binding.frames[self.stage()]as usize)
    }
}
pub fn apply(world:&World,state:&mut crate::world::State,region:usize) {
    if world.opened() {if let Some(Some(binding))=REGIONS.get(region){state.append_script_edges(binding.edges);}}
}
pub fn draw(room:&Room,frame:usize,camera:(i32,i32))->u32 {
    let f=room.frame(frame);let b=core::array::from_fn::<_,4,_>(|i|i32_at(f,4+i*4));
    let coords=[(b[0],b[3]),(b[2],b[3]),(b[0],b[1]),(b[2],b[1])];let mut vertices=[(0i16,0i16);4];
    for(k,(x,y))in coords.iter().enumerate(){
        vertices[k]=((160+((((POSITION[0]+x-camera.0)>>8)*SCALE)>>20))as i16,
                     (120-((((POSITION[1]+y-camera.1)>>8)*SCALE)>>20))as i16);
    }
    crate::render::texture(u32_at(f,0)as usize,vertices,(128,128,128));1
}
