//! LZ4 HC block compression at level 9, a port of liblz4 1.9.4's
//! `LZ4_compress_HC` hash-chain compressor (lz4hc.c, BSD 2-Clause, Copyright
//! (c) Yann Collet), so its output is byte-identical to what hk-psx's Python
//! cookers get from `lz4.block.compress(data, mode='high_compression',
//! store_size=False)` (python-lz4 4.4.5, which bundles liblz4 1.9.4). One
//! block, no dictionary, unlimited output.

const MINMATCH: usize = 4;
const LASTLITERALS: usize = 5;
const MFLIMIT: usize = 12;
const ML_BITS: u32 = 4;
const ML_MASK: usize = (1 << ML_BITS) - 1;
const RUN_MASK: usize = (1 << (8 - ML_BITS)) - 1;
const OPTIMAL_ML: usize = ML_MASK - 1 + MINMATCH;
const HASH_LOG: u32 = 15;
const MAXD: usize = 1 << 16;
const DISTANCE_MAX: u32 = 65535;
const MAX_ATTEMPTS: i32 = 256; // level 9
/// A fresh state starts its indexes 64 KB in.
const BASE: u32 = 64 * 1024;

struct Ctx<'a> {
    src: &'a [u8],
    hash: Vec<u32>,
    chain: Vec<u16>,
    next_to_update: u32,
}

fn read32(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn read16(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap())
}
fn hash_of(v: u32) -> usize {
    (v.wrapping_mul(2654435761) >> ((MINMATCH as u32 * 8) - HASH_LOG)) as usize
}

/// LZ4_count: equal bytes from `a` and `b` onward, `a` stopping before `limit`.
fn count(s: &[u8], mut a: usize, mut b: usize, limit: usize) -> usize {
    let start = a;
    while a < limit && s[a] == s[b] {
        a += 1;
        b += 1;
    }
    a - start
}

fn count_back(s: &[u8], ip: usize, m: usize, imin: usize, mmin: usize) -> isize {
    let min = (imin as isize - ip as isize).max(mmin as isize - m as isize);
    let mut back: isize = 0;
    while back > min && s[(ip as isize + back - 1) as usize] == s[(m as isize + back - 1) as usize] {
        back -= 1;
    }
    back
}

/// LZ4HC_countPattern on a little-endian 64-bit build.
fn count_pattern(s: &[u8], mut ip: usize, end: usize, pattern32: u32) -> usize {
    let start = ip;
    let pattern = pattern32 as u64 | (pattern32 as u64) << 32;
    while end >= 7 && ip < end - 7 {
        let diff = u64::from_le_bytes(s[ip..ip + 8].try_into().unwrap()) ^ pattern;
        if diff == 0 {
            ip += 8;
            continue;
        }
        ip += (diff.trailing_zeros() / 8) as usize;
        return ip - start;
    }
    let mut byte = pattern;
    while ip < end && s[ip] == byte as u8 {
        ip += 1;
        byte >>= 8;
    }
    ip - start
}

fn reverse_count_pattern(s: &[u8], mut ip: usize, low: usize, pattern: u32) -> usize {
    let start = ip;
    while ip >= low + 4 {
        if read32(s, ip - 4) != pattern {
            break;
        }
        ip -= 4;
    }
    let bytes = pattern.to_le_bytes();
    let mut k = 3usize;
    while ip > low {
        if s[ip - 1] != bytes[k] {
            break;
        }
        ip -= 1;
        k = k.wrapping_sub(1);
        if k == usize::MAX {
            k = 3; // C walks off the pattern's bytes here; four matched bytes already returned above
        }
    }
    start - ip
}

fn protect_dict_end(dict_limit: u32, match_index: u32) -> bool {
    dict_limit.wrapping_sub(1).wrapping_sub(match_index) >= 3
}

impl Ctx<'_> {
    fn idx(&self, p: usize) -> u32 {
        p as u32 + BASE
    }
    fn pos(&self, i: u32) -> usize {
        (i - BASE) as usize
    }
    fn chain_at(&self, i: u32) -> u32 {
        self.chain[i as usize & (MAXD - 1)] as u32
    }

    fn insert(&mut self, ip: usize) {
        let target = self.idx(ip);
        let mut idx = self.next_to_update;
        while idx < target {
            let h = hash_of(read32(self.src, self.pos(idx)));
            let delta = (idx - self.hash[h]).min(DISTANCE_MAX);
            self.chain[idx as usize & (MAXD - 1)] = delta as u16;
            self.hash[h] = idx;
            idx += 1;
        }
        self.next_to_update = target;
    }

    /// LZ4HC_InsertAndGetWiderMatch without dictionaries, chain swap off.
    /// Returns (longest, match position, start position).
    fn wider_match(&mut self, ip: usize, ilow: usize, ihigh: usize, mut longest: usize) -> (usize, usize, usize) {
        let s = self.src;
        let prefix_idx = BASE;
        let ip_index = self.idx(ip);
        let low_limit = BASE;
        let lowest = if low_limit + (DISTANCE_MAX + 1) > ip_index { low_limit } else { ip_index - DISTANCE_MAX };
        let look_back = ip - ilow;
        let mut attempts = MAX_ATTEMPTS;
        let pattern = read32(s, ip);
        let mut repeat = 0u8; // 0 untested, 1 not, 2 confirmed
        let mut src_pattern_len = 0usize;
        let (mut matchpos, mut startpos) = (0usize, ip);
        self.insert(ip);
        let mut match_index = self.hash[hash_of(pattern)];
        while match_index >= lowest && attempts > 0 {
            attempts -= 1;
            // Always within the prefix: there is no dictionary.
            let mp = self.pos(match_index);
            if read16(s, ilow + longest - 1) == read16(s, mp + longest - 1 - look_back) && read32(s, mp) == pattern {
                let back = if look_back > 0 { count_back(s, ip, mp, ilow, 0) } else { 0 };
                let ml = (MINMATCH + count(s, ip + MINMATCH, mp + MINMATCH, ihigh)) as isize - back;
                if ml as usize > longest {
                    longest = ml as usize;
                    matchpos = (mp as isize + back) as usize;
                    startpos = (ip as isize + back) as usize;
                }
            }
            let dist_next = self.chain_at(match_index);
            if dist_next == 1 {
                let candidate = match_index - 1;
                if repeat == 0 {
                    if (pattern & 0xffff) == (pattern >> 16) && (pattern & 0xff) == (pattern >> 24) {
                        repeat = 2;
                        src_pattern_len = count_pattern(s, ip + 4, ihigh, pattern) + 4;
                    } else {
                        repeat = 1;
                    }
                }
                if repeat == 2 && candidate >= lowest && protect_dict_end(prefix_idx, candidate) {
                    let mp = self.pos(candidate);
                    if read32(s, mp) == pattern {
                        let forward = count_pattern(s, mp + 4, ihigh, pattern) + 4;
                        let mut back_len = reverse_count_pattern(s, mp, 0, pattern);
                        back_len = (candidate - (candidate - back_len as u32).max(lowest)) as usize;
                        let segment = back_len + forward;
                        if segment >= src_pattern_len && forward <= src_pattern_len {
                            let new = candidate + forward as u32 - src_pattern_len as u32;
                            match_index = if protect_dict_end(prefix_idx, new) { new } else { prefix_idx };
                        } else {
                            let new = candidate - back_len as u32;
                            if !protect_dict_end(prefix_idx, new) {
                                match_index = prefix_idx;
                            } else {
                                match_index = new;
                                if look_back == 0 {
                                    let max_ml = segment.min(src_pattern_len);
                                    if longest < max_ml {
                                        if ip_index - match_index > DISTANCE_MAX {
                                            break;
                                        }
                                        longest = max_ml;
                                        matchpos = self.pos(match_index);
                                        startpos = ip;
                                    }
                                    let dist = self.chain_at(match_index);
                                    if dist > match_index {
                                        break;
                                    }
                                    match_index -= dist;
                                }
                            }
                        }
                        continue;
                    }
                }
            }
            match_index = match_index.wrapping_sub(self.chain_at(match_index));
        }
        (longest, matchpos, startpos)
    }
}

fn encode_sequence(out: &mut Vec<u8>, src: &[u8], ip: &mut usize, anchor: &mut usize, ml: usize, m: usize) {
    let token_at = out.len();
    out.push(0);
    let length = *ip - *anchor;
    if length >= RUN_MASK {
        out[token_at] = (RUN_MASK << ML_BITS) as u8;
        let mut len = length - RUN_MASK;
        while len >= 255 {
            out.push(255);
            len -= 255;
        }
        out.push(len as u8);
    } else {
        out[token_at] = (length << ML_BITS) as u8;
    }
    out.extend_from_slice(&src[*anchor..*ip]);
    out.extend_from_slice(&((*ip - m) as u16).to_le_bytes());
    let mut length = ml - MINMATCH;
    if length >= ML_MASK {
        out[token_at] += ML_MASK as u8;
        length -= ML_MASK;
        while length >= 510 {
            out.push(255);
            out.push(255);
            length -= 510;
        }
        if length >= 255 {
            length -= 255;
            out.push(255);
        }
        out.push(length as u8);
    } else {
        out[token_at] += length as u8;
    }
    *ip += ml;
    *anchor = *ip;
}

/// `LZ4_compress_HC(src, dst, len(src), LZ4_compressBound(len(src)), 9)`.
pub fn compress_hc(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + src.len() / 255 + 16);
    let mut ctx = Ctx { src, hash: vec![0; 1 << HASH_LOG], chain: vec![0; MAXD], next_to_update: BASE };
    let iend = src.len();
    let mut ip = 0usize;
    let mut anchor = 0usize;
    if src.len() > MFLIMIT {
        let mflimit = iend - MFLIMIT;
        let matchlimit = iend - LASTLITERALS;
        'main: while ip <= mflimit {
            let (mut ml, mut r, _) = ctx.wider_match(ip, ip, matchlimit, MINMATCH - 1);
            if ml < MINMATCH {
                ip += 1;
                continue;
            }
            let (mut start0, mut ref0, mut ml0) = (ip, r, ml);
            let (mut start2, mut ref2, mut ml2);
            let (mut start3, mut ref3, mut ml3);
            // _Search2
            'search2: loop {
                if ip + ml <= mflimit {
                    let (l, m, s) = ctx.wider_match(ip + ml - 2, ip, matchlimit, ml);
                    ml2 = l;
                    ref2 = m;
                    start2 = s;
                } else {
                    ml2 = ml;
                    ref2 = 0;
                    start2 = 0;
                }
                if ml2 == ml {
                    encode_sequence(&mut out, src, &mut ip, &mut anchor, ml, r);
                    continue 'main;
                }
                if start0 < ip && start2 < ip + ml0 {
                    ip = start0;
                    r = ref0;
                    ml = ml0;
                }
                if start2 - ip < 3 {
                    ml = ml2;
                    ip = start2;
                    r = ref2;
                    continue 'search2;
                }
                // _Search3
                loop {
                    if start2 - ip < OPTIMAL_ML {
                        let mut new_ml = ml.min(OPTIMAL_ML);
                        if ip + new_ml > start2 + ml2 - MINMATCH {
                            new_ml = (start2 - ip) + ml2 - MINMATCH;
                        }
                        let correction = new_ml as isize - (start2 - ip) as isize;
                        if correction > 0 {
                            start2 += correction as usize;
                            ref2 += correction as usize;
                            ml2 -= correction as usize;
                        }
                    }
                    if start2 + ml2 <= mflimit {
                        let (l, m, s) = ctx.wider_match(start2 + ml2 - 3, start2, matchlimit, ml2);
                        ml3 = l;
                        ref3 = m;
                        start3 = s;
                    } else {
                        ml3 = ml2;
                        ref3 = 0;
                        start3 = 0;
                    }
                    if ml3 == ml2 {
                        if start2 < ip + ml {
                            ml = start2 - ip;
                        }
                        encode_sequence(&mut out, src, &mut ip, &mut anchor, ml, r);
                        ip = start2;
                        encode_sequence(&mut out, src, &mut ip, &mut anchor, ml2, ref2);
                        continue 'main;
                    }
                    if start3 < ip + ml + 3 {
                        if start3 >= ip + ml {
                            if start2 < ip + ml {
                                let correction = ip + ml - start2;
                                start2 += correction;
                                ref2 += correction;
                                ml2 = ml2.wrapping_sub(correction);
                                if (ml2 as isize) < MINMATCH as isize {
                                    start2 = start3;
                                    ref2 = ref3;
                                    ml2 = ml3;
                                }
                            }
                            encode_sequence(&mut out, src, &mut ip, &mut anchor, ml, r);
                            ip = start3;
                            r = ref3;
                            ml = ml3;
                            start0 = start2;
                            ref0 = ref2;
                            ml0 = ml2;
                            continue 'search2;
                        }
                        start2 = start3;
                        ref2 = ref3;
                        ml2 = ml3;
                        continue;
                    }
                    if start2 < ip + ml {
                        if start2 - ip < OPTIMAL_ML {
                            if ml > OPTIMAL_ML {
                                ml = OPTIMAL_ML;
                            }
                            if ip + ml > start2 + ml2 - MINMATCH {
                                ml = (start2 - ip) + ml2 - MINMATCH;
                            }
                            let correction = ml as isize - (start2 - ip) as isize;
                            if correction > 0 {
                                start2 += correction as usize;
                                ref2 += correction as usize;
                                ml2 -= correction as usize;
                            }
                        } else {
                            ml = start2 - ip;
                        }
                    }
                    encode_sequence(&mut out, src, &mut ip, &mut anchor, ml, r);
                    ip = start2;
                    r = ref2;
                    ml = ml2;
                    start2 = start3;
                    ref2 = ref3;
                    ml2 = ml3;
                }
            }
        }
    }
    // Last literals.
    let last = iend - anchor;
    if last >= RUN_MASK {
        out.push((RUN_MASK << ML_BITS) as u8);
        let mut acc = last - RUN_MASK;
        while acc >= 255 {
            out.push(255);
            acc -= 255;
        }
        out.push(acc as u8);
    } else {
        out.push((last << ML_BITS) as u8);
    }
    out.extend_from_slice(&src[anchor..]);
    out
}

/// Decode an LZ4 block (for tests and checks).
pub fn decompress(block: &[u8], size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size);
    let mut p = 0;
    while p < block.len() {
        let token = block[p] as usize;
        p += 1;
        let mut lit = token >> 4;
        if lit == 15 {
            loop {
                let b = *block.get(p)? as usize;
                p += 1;
                lit += b;
                if b != 255 {
                    break;
                }
            }
        }
        out.extend_from_slice(block.get(p..p + lit)?);
        p += lit;
        if p >= block.len() {
            break;
        }
        let off = u16::from_le_bytes([*block.get(p)?, *block.get(p + 1)?]) as usize;
        p += 2;
        let mut ml = (token & 15) + MINMATCH;
        if token & 15 == 15 {
            loop {
                let b = *block.get(p)? as usize;
                p += 1;
                ml += b;
                if b != 255 {
                    break;
                }
            }
        }
        let start = out.len().checked_sub(off)?;
        for i in 0..ml {
            let b = out[start + i];
            out.push(b);
        }
    }
    (out.len() == size).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_compresses_repeats() {
        let inputs: Vec<Vec<u8>> = vec![
            vec![],
            b"short".to_vec(),
            vec![7u8; 1000],
            (0..5000u32).map(|i| (i % 251) as u8).collect(),
            (0..3000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect(),
        ];
        for data in inputs {
            let z = compress_hc(&data);
            assert_eq!(decompress(&z, data.len()).as_deref(), Some(&data[..]));
        }
        assert!(compress_hc(&[7u8; 1000]).len() < 20);
    }
}
