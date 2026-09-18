//! Native check of the bench Save Game record: the card crate is stubbed, only encode/decode runs.
extern crate self as psx_mc;
pub const FRAME_SIZE:usize=128;pub type Result<T>=core::result::Result<T,()>;
pub trait Block{fn read_frame(&mut self,_:u16,_:&mut [u8;FRAME_SIZE])->Result<()>;fn write_frame(&mut self,_:u16,_:&[u8;FRAME_SIZE])->Result<()>;}
pub struct HardwareCard;pub enum Slot{One}impl HardwareCard{pub fn new(_:Slot)->Self{Self}}
impl Block for HardwareCard{fn read_frame(&mut self,_:u16,_:&mut [u8;FRAME_SIZE])->Result<()>{Err(())}fn write_frame(&mut self,_:u16,_:&[u8;FRAME_SIZE])->Result<()>{Err(())}}
pub struct Card<B>(B);impl<B:Block> Card<B>{pub fn new(b:B)->Self{Self(b)}pub fn is_formatted(&mut self)->Result<bool>{Err(())}pub fn format(&mut self)->Result<()>{Err(())}pub fn write(&mut self,_:&str,_:&str,_:&[u8])->Result<()>{Err(())}pub fn read(&mut self,_:&str,_:&mut [u8])->Result<usize>{Err(())}}
pub mod input{pub fn checkpoint(){}}
#[path="../game/src/save.rs"]mod save;
