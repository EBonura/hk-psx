//! Native check of the bench Save Game record: the card crate is stubbed, only encode/decode runs.
extern crate self as psx_mc;
pub const FRAME_SIZE:usize=128;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum Error{NoCard,Protocol,BadChecksum,OutOfRange,NotFormatted,NotFound,NoSpace,Exists,Corrupt,BufferTooSmall,BadContainer,Compression,BadName}
pub type Result<T>=core::result::Result<T,Error>;
pub trait Block{fn read_frame(&mut self,_:u16,_:&mut [u8;FRAME_SIZE])->Result<()>;fn write_frame(&mut self,_:u16,_:&[u8;FRAME_SIZE])->Result<()>;}
pub struct HardwareCard;pub enum Slot{One}impl HardwareCard{pub fn new(_:Slot)->Self{Self}}
impl Block for HardwareCard{fn read_frame(&mut self,_:u16,_:&mut [u8;FRAME_SIZE])->Result<()>{Err(Error::NoCard)}fn write_frame(&mut self,_:u16,_:&[u8;FRAME_SIZE])->Result<()>{Err(Error::NoCard)}}
pub struct Card<B>(B);impl<B:Block> Card<B>{pub fn new(b:B)->Self{Self(b)}pub fn is_formatted(&mut self)->Result<bool>{Err(Error::NoCard)}pub fn format(&mut self)->Result<()>{Err(Error::NoCard)}pub fn write(&mut self,_:&str,_:&str,_:&[u8])->Result<()>{Err(Error::NoCard)}pub fn read(&mut self,_:&str,_:&mut [u8])->Result<usize>{Err(Error::NoCard)}}
pub mod input{pub fn checkpoint(){}}
// The save only needs the Shade's plain record; game/src/shade.rs itself is
// covered by tests/shade_runtime.rs, which links hk_sim.
pub mod shade{#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]pub struct Record{
 pub present:bool,pub scene:u32,pub position:[i32;2],pub hp:u16,pub geo_pool:u32}}
#[path="../game/src/save.rs"]mod save;
