//! Actual CD transport with a deterministic controller and interrupt-latch fixture.
extern crate self as psx_io;
extern crate self as psx_rt;
use std::sync::atomic::{AtomicU32,Ordering};
use std::sync::Mutex;
static MASK:AtomicU32=AtomicU32::new(4);
static PENDING:AtomicU32=AtomicU32::new(0);
static FLAG:AtomicU32=AtomicU32::new(0);
static COMMANDS:Mutex<Vec<u8>>=Mutex::new(Vec::new());
pub fn fixture_reset(){MASK.store(4,Ordering::SeqCst);PENDING.store(0,Ordering::SeqCst);FLAG.store(0,Ordering::SeqCst);COMMANDS.lock().unwrap().clear();}
pub fn fixture_event(flag:u32,latched:bool){FLAG.store(flag,Ordering::SeqCst);PENDING.store(if latched{4}else{0},Ordering::SeqCst);}
pub fn fixture_mask()->u32{MASK.load(Ordering::SeqCst)}
pub fn fixture_flag()->u32{FLAG.load(Ordering::SeqCst)}
pub fn fixture_commands()->Vec<u8>{COMMANDS.lock().unwrap().clone()}
pub unsafe fn read8(address:u32)->u8{if address==0x1f801800{0x40}else{0}}
pub unsafe fn write8(address:u32,value:u8){if address==0x1f801801{COMMANDS.lock().unwrap().push(value);}}
pub mod irq {
 pub mod source {pub const CDROM:u32=2;}
 pub fn mask()->u32{super::fixture_mask()}
 pub fn set_mask(n:u32){super::MASK.store(n,super::Ordering::SeqCst);}
 pub fn pending()->u32{super::PENDING.load(super::Ordering::SeqCst)}
 pub fn ack(n:u32){super::PENDING.fetch_and(!n,super::Ordering::SeqCst);}
}
pub mod cdrom {
 pub const BASE:u32=0x1f801800;
 pub const CMD_PAUSE:u8=9;pub const CMD_SETLOC:u8=2;pub const CMD_SEEKL:u8=0x15;
 pub const CMD_SETMODE:u8=0x0e;pub const CMD_READN:u8=6;pub const MODE_DOUBLE_SPEED:u8=0x80;
 pub fn irq_flag_value()->u8{super::fixture_flag() as u8}
 pub fn discard_response(){}
 pub fn acknowledge_irq(_:u8){super::FLAG.store(0,super::Ordering::SeqCst);super::PENDING.store(0,super::Ordering::SeqCst);}
 pub fn bin_to_bcd(n:u8)->u8{(n/10)*16+n%10}
}
pub mod timers {pub enum Timer{Timer2} pub fn counter(_:Timer)->u16{0}pub fn set_mode(_:Timer,_:u16){}}
pub mod interrupts{pub fn vblank_count()->u32{0}}
pub mod disc_base{pub fn shift_lba(n:u32)->u32{n}}
pub mod cache{pub fn flush_i_cache(){}}
static mut HK_CD_SECTORS_READ:u32=0;
#[path="../game/src/cd_stream.rs"] mod transport;
