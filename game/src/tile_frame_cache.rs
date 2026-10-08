//! Reusable CPU-only coverage preparation; packets and GPU state are not cached.
//! Events stay immutable in renderer RAM. Only their initial row words need to
//! survive the next begin(), which clears the scratchpad's working rows.
pub struct Prepared {camera:(i32,i32),generation:u32,rows:[u32;15],max_rank:u16,valid:bool}
impl Prepared {
    pub const fn new()->Self {Self{camera:(0,0),generation:0,rows:[0;15],max_rank:0,valid:false}}
    #[inline]pub fn invalidate(&mut self){self.valid=false;}
    #[inline]pub fn matches(&self,camera:(i32,i32),generation:u32)->bool {self.valid&&self.camera==camera&&self.generation==generation}
    pub fn save(&mut self,camera:(i32,i32),generation:u32,rows:&[u32;15],max_rank:u16){
        self.rows.copy_from_slice(rows);self.camera=camera;self.generation=generation;self.max_rank=max_rank;self.valid=true;
    }
    pub fn take(&mut self,camera:(i32,i32),generation:u32,rows:&mut[u32;15])->Option<u16>{
        let hit=self.matches(camera,generation);
        if !hit{self.valid=false;return None;}rows.copy_from_slice(&self.rows);Some(self.max_rank)
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn cold_misses_and_exact_key_hits_restore_rows_after_each_frame(){
        let mut cache=Prepared::new();let mut rows=[0xabcd;15];assert_eq!(cache.take((0,0),0,&mut rows),None);assert_eq!(rows,[0xabcd;15]);
        let initial=core::array::from_fn(|i|1<<i);cache.save((123,-456),19,&initial,1042);rows.fill(0);
        assert_eq!(cache.take((123,-456),19,&mut rows),Some(1042));assert_eq!(rows,initial);
        rows.fill(0xdead);assert_eq!(cache.take((123,-456),19,&mut rows),Some(1042));assert_eq!(rows,initial);
        cache.invalidate();rows.fill(0xdead);assert_eq!(cache.take((123,-456),19,&mut rows),None);assert_eq!(rows,[0xdead;15]);
    }
    #[test]fn camera_state_and_explicit_invalidation_reject_without_touching_output(){
        for change in 0..4 {let mut cache=Prepared::new();cache.save((123,-456),u32::MAX,&[0xff;15],9);if change==3{cache.invalidate();}let camera=match change{0=>(124,-456),1=>(123,-455),_=>(123,-456)};let generation=if change==2{0}else{u32::MAX};let mut rows=[0xdead;15];assert_eq!(cache.take(camera,generation,&mut rows),None);assert_eq!(rows,[0xdead;15]);}
    }
}
