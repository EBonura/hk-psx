//! Rectangle collector snapshot leased by an exact BACK prefix key.
use core::mem::MaybeUninit;
/// A value-only initialized prefix. Its validity belongs to the outer exact
/// frame key; no stale or uninitialized entries are exposed after replacement.
#[repr(C)]
pub struct Snapshot<T:Copy,const N:usize> {
 entries:[MaybeUninit<T>;N],count:usize,union:[i16;4],
}
impl<T:Copy,const N:usize> Snapshot<T,N> {
 pub const fn new()->Self{Self{entries:[const{MaybeUninit::uninit()};N],count:0,union:[i16::MAX,i16::MAX,i16::MIN,i16::MIN]}}
 pub fn save(&mut self,entries:&[T],union:&[i16;4]){
  assert!(entries.len()<=N);self.count=0;
  for (slot,&value)in self.entries.iter_mut().zip(entries){slot.write(value);}
  self.union=*union;self.count=entries.len();
 }
 pub fn entries(&self)->&[T]{unsafe{core::slice::from_raw_parts(self.entries.as_ptr().cast(),self.count)}}
 pub fn union(&self)->[i16;4]{self.union}
}
#[cfg(test)]mod tests{
 use super::*;
 #[derive(Clone,Copy,PartialEq,Eq,Debug)]#[repr(C)]struct Occluder{draw:u16,rect:[i16;4],front:bool}
 fn value(i:usize)->Occluder{Occluder{draw:i as u16,rect:[i as i16,2*i as i16,320-i as i16,240-i as i16],front:i%2==0}}
 #[test]fn initialized_prefix_survives_every_shrink_grow_and_empty_replacement(){
  let mut snap=Snapshot::<Occluder,8>::new();assert!(snap.entries().is_empty());
  for old in 0..=8{for new in 0..=8{
   let prior:Vec<_>=(0..old).map(value).collect();snap.save(&prior,&[0,0,320,240]);assert_eq!(snap.entries(),&prior);
   let next:Vec<_>=(10..10+new).map(value).collect();let union=if new==0{[i16::MAX,i16::MAX,i16::MIN,i16::MIN]}else{[10,20,310,230]};
   snap.save(&next,&union);assert_eq!(snap.entries(),&next);assert_eq!(snap.union(),union);
  }}
 }
 #[test]fn reset_working_store_then_restore_keeps_order_source_front_and_bounds(){
  let mut snap=Snapshot::<Occluder,8>::new();let input=[value(7),value(2),value(300),value(0)];snap.save(&input,&[1,2,319,238]);
  let mut work=[value(99);8];let count=snap.entries().len();work[..count].copy_from_slice(snap.entries());assert_eq!(&work[..count],&input);assert_eq!(&work[count..],&[value(99);4]);
  assert_eq!(snap.union(),[1,2,319,238]);
 }
 #[test]fn poisoned_backing_never_becomes_a_bool_until_written(){
  let mut store=MaybeUninit::<Snapshot<Occluder,8>>::uninit();unsafe{
   let p=store.as_mut_ptr();p.cast::<u8>().write_bytes(0xff,core::mem::size_of_val(&store));
   (&raw mut (*p).count).write(0);(&raw mut (*p).union).write([0;4]);
   let mut s=store.assume_init();assert!(s.entries().is_empty());s.save(&[value(1),value(2)],&[3,4,5,6]);assert_eq!(s.entries(),&[value(1),value(2)]);
  }
 }
}
