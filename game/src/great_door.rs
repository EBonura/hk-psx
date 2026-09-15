//! Source Great Door damage stages and session persistence. The transition
//! delay combines two source NextFrame events and the 2.5 second entry delay;
//! original fade/camera choreography and destruction effects remain separate.
use hk_format::{Room,i32_at,u32_at};
#[derive(Clone,Copy)]
pub struct Params {pub stage_hits:[u8;3],pub cooldown:u8,pub transition:u16}
#[derive(Clone,Copy)]
pub struct Binding {pub frames:[u16;3],pub edges:&'static [u16]}
include!(concat!(env!("CARGO_MANIFEST_DIR"),"/../data/great_door.rs"));
#[derive(Clone,Copy,Default,Debug,PartialEq,Eq)]
pub struct Strike {pub hit:bool,pub opened:bool}
pub struct World {hits:u8,cooldown:u8,wait:u16,transitions:u16}
#[no_mangle]pub static mut HK_GREAT_DOOR_HITS:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_STAGE:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_OPENED:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_WAIT:u32=0;
#[no_mangle]pub static mut HK_GREAT_DOOR_TRANSITIONS:u32=0;
impl World {
    pub const fn new()->Self {Self {hits:0,cooldown:0,wait:0,transitions:0}}
    pub fn reset(&mut self) {*self=Self::new();self.publish();}
    pub fn opened(&self)->bool {self.hits>=PARAMS.stage_hits[2]}
    pub fn pending(&self)->bool {self.wait!=0}
    fn stage(&self)->usize {if self.hits>=PARAMS.stage_hits[1]{2}else if self.hits>=PARAMS.stage_hits[0]{1}else{0}}
    fn publish(&self) {unsafe {
        HK_GREAT_DOOR_HITS=self.hits as u32;HK_GREAT_DOOR_STAGE=self.stage()as u32;
        HK_GREAT_DOOR_OPENED=u32::from(self.opened());HK_GREAT_DOOR_WAIT=self.wait as u32;
        HK_GREAT_DOOR_TRANSITIONS=self.transitions as u32;
    }}
    pub fn tick(&mut self,scene:usize)->bool {
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
