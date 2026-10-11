//! Bank and sector validation shared by the two-pass Runner audio overlay loader.
pub type Clip = (u32, usize, u16, u16, u16, bool);
pub const BASE: u32 = 503808;
// psx_spu::init reserves the final16 bytes as its disabled reverb work area.
pub const END: u32 = 524272;

pub fn valid_manifest(bytes: usize, clips: &[Clip]) -> bool {
    let Some(end) = BASE.checked_add(bytes as u32) else {
        return false;
    };
    if bytes == 0 || bytes > (END - BASE) as usize || end > END {
        return false;
    }
    let mut expected = BASE;
    for &(start, len, pitch, lo, hi, _) in clips {
        if start != expected
            || start % 16 != 0
            || len == 0
            || len % 16 != 0
            || pitch == 0
            || pitch > 0x3fff
            || lo == 0
            || lo > hi
            || hi > 0x3fff
        {
            return false;
        }
        let Some(next) = start.checked_add(len as u32) else {
            return false;
        };
        if len > (END - BASE) as usize || next > end {
            return false;
        }
        expected = next;
    }
    expected == end
}

pub fn valid_fragment(clips: &[Clip], offset: usize, data: &[u8]) -> bool {
    if offset % 16 != 0 || data.is_empty() || data.len() % 16 != 0 {
        return false;
    }
    for (index, block) in data.chunks_exact(16).enumerate() {
        let Some(at) = offset
            .checked_add(index * 16)
            .and_then(|n| u32::try_from(n).ok())
            .and_then(|n| BASE.checked_add(n))
        else {
            return false;
        };
        if at >= END {
            return false;
        }
        let Some(&(start, len, _, _, _, loops)) = clips
            .iter()
            .find(|&&(s, n, _, _, _, _)| at >= s && at < s.saturating_add(n as u32))
        else {
            return false;
        };
        let last = at - start + 16 == len as u32;
        let flags = if loops {
            u8::from(at == start) * 4 | u8::from(last) * 3
        } else {
            u8::from(last)
        };
        if block[0] & 15 > 12 || block[0] >> 4 > 4 || block[1] != flags {
            return false;
        }
        if !loops && last && (block[0] != 12 || block[2..].iter().any(|&x| x != 0)) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    const CLIPS: [Clip; 2] = [
        (BASE, 32, 743, 743, 743, true),
        (BASE + 32, 32, 1024, 1126, 1229, false),
    ];
    fn bank() -> [u8; 64] {
        let mut b = [0; 64];
        b[1] = 4;
        b[17] = 3;
        b[48] = 12;
        b[49] = 1;
        b
    }
    #[test]
    fn sectors_can_split_at_a_clip_boundary() {
        let b = bank();
        assert!(valid_manifest(64, &CLIPS));
        assert!(valid_fragment(&CLIPS, 0, &b[..32]));
        assert!(valid_fragment(&CLIPS, 32, &b[32..]));
    }
    #[test]
    fn malformed_payload_and_partial_clip_are_rejected() {
        let mut b = bank();
        b[49] = 0;
        assert!(!valid_fragment(&CLIPS, 0, &b));
        assert!(!valid_fragment(&CLIPS, 1, &bank()));
        assert!(!valid_fragment(&CLIPS, 0, &bank()[..63]));
        b = bank();
        b[63] = 1;
        assert!(!valid_fragment(&CLIPS, 0, &b));
    }
    #[test]
    fn exact_ordinary_endpoint_is_valid_but_reverb_tail_upload_is_rejected() {
        let clips = [(BASE, 20464, 743, 743, 743, true)];
        assert!(valid_manifest(20464, &clips));
        assert!(!valid_manifest(20480, &clips));
        let mut bank = [0u8; 20480];
        bank[1] = 4;
        bank[20449] = 3;
        assert!(valid_fragment(&clips, 0, &bank[..20464]));
        assert!(!valid_fragment(&clips, 0, &bank));
        assert!(!valid_fragment(&clips, 20464, &bank[20464..]));
    }
    #[test]
    fn gaps_overflow_and_window_growth_are_rejected() {
        let mut c = CLIPS;
        c[1].0 += 16;
        assert!(!valid_manifest(64, &c));
        assert!(!valid_manifest(20496, &CLIPS));
        assert!(!valid_fragment(&CLIPS, usize::MAX - 15, &bank()));
    }
}
