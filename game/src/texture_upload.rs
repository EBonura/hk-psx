//! Use the SDK's aligned FIFO path for the aligned room arenas. Embedded or
//! odd-sized animation assets retain the byte path; no DMA hardware policy is
//! changed. Word loads avoid rebuilding every VRAM word from four RAM reads.
use psx_vram::VramRect;
pub fn upload(rect:VramRect,bytes:&[u8]) {
    if bytes.as_ptr() as usize & 3 == 0 && bytes.len() & 3 == 0 {
        let words=unsafe {core::slice::from_raw_parts(bytes.as_ptr().cast::<u32>(),bytes.len()/4)};
        psx_vram::upload_words(rect,words);
    } else {
        psx_vram::upload_bytes(rect,bytes);
    }
}
