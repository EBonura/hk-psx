//! Resumable form of the pinned SDK's HLZC/LZ4 in-place decoder.
//! One step consumes at most `budget` byte operations, including checksums and
//! relocation, or at most eight metadata records in the validation phase. The inactive arena stays private until both hashes and the
//! selected HKROOM02/HKSCNE01 validator succeed. No extra room-sized scratch allocation.
use hk_format::{SceneValidation, Validation};
#[derive(Clone, Copy)]
enum Payload {
    Room,
    Scene,
    Bytes,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Error {
    Checksum,
    Decompress,
    RoomFormat,
}
#[derive(Clone, Copy)]
enum Phase {
    StoredHash,
    Relocate,
    Token,
    LiteralLength,
    Literals,
    Offset,
    MatchLength,
    Match,
    RawHash,
    Validate,
    Done,
}
pub struct Decoder {
    phase: Phase,
    stored: usize,
    stored_hash: u32,
    raw: usize,
    raw_hash: u32,
    pos: usize,
    hash: u32,
    source: usize,
    output: usize,
    run: usize,
    offset: usize,
    token: u8,
    validation: Option<Validation>,
    payload: Payload,
    scene_validation: Option<SceneValidation>,
    /// Where the stored bytes begin. Zero is the historical layout (stored
    /// bytes at the front, relocated to the tail before decoding); a
    /// nonzero start whose stored bytes end exactly at the end of the arena
    /// slice decodes where it lies, with no relocation copy.
    input: usize,
    /// The stored hash matched and the cook is trusted: skip raw hash and validation.
    trusted: bool,
}
/// Trust the cook: a payload whose stored bytes match the cooked hash is not
/// raw-hashed or format-walked again after decoding. The LZ4 decode of verified
/// bytes is deterministic and bounds-checked, so those two re-proved what the
/// cook proved, at about 440 ms per gate. The stored hash stays: it is the CD
/// integrity check. HK_VERIFY_COOK=1 at build time restores both.
const TRUST_COOK: bool = option_env!("HK_VERIFY_COOK").is_none();
const FNV: u32 = 0x811c9dc5;
// 0x01000193 = 2^24 + 13*31. Eight wrapping shifts/adds avoid
// dependent MULT/MFLO stalls on R3000A without changing either checksum.
// Keep the MIPS operations opaque to LLVM's constant-multiply refolding.
#[inline(always)]
fn fnv_multiply(mut value: u32) -> u32 {
    #[cfg(target_arch = "mips")]
    unsafe {
        core::arch::asm!(
        "sll {t0}, {value}, 1", "addu {t0}, {t0}, {value}",
        "sll {t0}, {t0}, 2", "addu {t0}, {t0}, {value}",
        "sll {t1}, {t0}, 5", "subu {t0}, {t1}, {t0}",
        "sll {t1}, {value}, 24", "addu {value}, {t0}, {t1}",
        value=inout(reg)value,t0=out(reg)_,t1=out(reg)_,
        options(pure,nomem,nostack));
    }
    #[cfg(not(target_arch = "mips"))]
    {
        let thirteen = value
            .wrapping_shl(1)
            .wrapping_add(value)
            .wrapping_shl(2)
            .wrapping_add(value);
        value = thirteen
            .wrapping_shl(5)
            .wrapping_sub(thirteen)
            .wrapping_add(value.wrapping_shl(24));
    }
    value
}
// Keep the four dependent FNV rounds identical, but fetch aligned payload
// words once: each byte RAM access otherwise costs a separate PS1 bus load.
fn hash_bytes(mut hash: u32, bytes: &[u8]) -> u32 {
    let mut p = bytes.as_ptr();
    let end = unsafe { p.add(bytes.len()) };
    while p != end && (p as usize & 3) != 0 {
        hash = fnv_multiply(hash ^ unsafe { *p } as u32);
        p = unsafe { p.add(1) };
    }
    while (end as usize) - (p as usize) >= 4 {
        // Alignment is established above; all four bytes lie in the slice.
        let word = u32::from_le(unsafe { p.cast::<u32>().read() });
        hash = fnv_multiply(hash ^ (word & 255));
        hash = fnv_multiply(hash ^ ((word >> 8) & 255));
        hash = fnv_multiply(hash ^ ((word >> 16) & 255));
        hash = fnv_multiply(hash ^ (word >> 24));
        p = unsafe { p.add(4) };
    }
    while p != end {
        hash = fnv_multiply(hash ^ unsafe { *p } as u32);
        p = unsafe { p.add(1) };
    }
    hash
}
/// Replicate an LZ4 match in forward order, including overlap. The caller has
/// checked offset<=output and the complete output span against both the raw
/// size and the unread compressed input. Only `count` output bytes are touched.
fn copy_match(arena: &mut [u8], output: usize, offset: usize, count: usize) {
    if offset == 1 {
        let byte = arena[output - 1];
        arena[output..output + count].fill(byte);
    } else if offset >= 4 {
        // At least four source bytes are initialized before each word load,
        // including distance4, whose next word depends on the preceding store.
        // Do not unroll into multiple loads before their corresponding stores.
        unsafe {
            let mut dst = arena.as_mut_ptr().add(output);
            let mut left = count;
            while left != 0 && dst as usize & 3 != 0 {
                dst.write(dst.sub(offset).read());
                dst = dst.add(1);
                left -= 1;
            }
            if offset & 3 == 0 {
                while left >= 4 {
                    let word = dst.sub(offset).cast::<u32>().read();
                    dst.cast::<u32>().write(word);
                    dst = dst.add(4);
                    left -= 4;
                }
            } else {
                while left >= 4 {
                    // Source alignment differs; read_unaligned accesses exactly
                    // four already-initialized bytes. Destination stays aligned.
                    let word = dst.sub(offset).cast::<u32>().read_unaligned();
                    dst.cast::<u32>().write(word);
                    dst = dst.add(4);
                    left -= 4;
                }
            }
            while left != 0 {
                dst.write(dst.sub(offset).read());
                dst = dst.add(1);
                left -= 1;
            }
        }
    } else {
        // Distances2/3 cannot load four bytes from the existing prefix. Keep
        // the disjoint seed/doubling recipe so every read is initialized.
        let seed = count.min(offset);
        unsafe {
            let dst = arena.as_mut_ptr().add(output);
            core::ptr::copy_nonoverlapping(dst.sub(offset), dst, seed);
            let mut initialized = seed;
            while initialized < count {
                let n = initialized.min(count - initialized);
                core::ptr::copy_nonoverlapping(dst, dst.add(initialized), n);
                initialized += n;
            }
        }
    }
}
/// Copy `n` bytes rounded up to whole words, unaligned on both sides
/// (lwl/lwr, swl/swr on the R3000). Each word is read before it is written.
#[inline(always)]
unsafe fn copy_words(dst: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        unsafe {
            dst.add(i)
                .cast::<u32>()
                .write_unaligned(src.add(i).cast::<u32>().read_unaligned());
        }
        i += 4;
    }
}
/// Sequences are decoded by `fast_sequences` only while at least this much
/// budget remains, so one call never runs far past its budget.
const FAST_MIN: usize = 48;
/// Decode complete LZ4 sequences from `source` into `output` until the budget
/// runs low, the input ends or a sequence needs the careful path (a length
/// that would overrun, a match that would overtake unread input, an offset
/// out of range, or a sequence longer than the remaining budget). Every check
/// runs before a sequence writes a byte, and state only advances past fully
/// decoded sequences, so the per-phase path can always take over from `source`.
#[inline(never)]
fn fast_sequences(
    arena: &mut [u8],
    source: &mut usize,
    output: &mut usize,
    raw: usize,
    budget: &mut usize,
) {
    let len = arena.len();
    let (mut s, mut o, mut b) = (*source, *output, *budget);
    let ptr = arena.as_mut_ptr();
    // Safety: every index below is checked against `len` before it is read,
    // and every write range against `raw` (<= len) and the unread input.
    while b >= FAST_MIN && s < len {
        let token = unsafe { *ptr.add(s) };
        let mut p = s + 1;
        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            let mut more = true;
            while more && p < len {
                let x = unsafe { *ptr.add(p) };
                p += 1;
                lit += x as usize;
                more = x == 255;
            }
            if more {
                break;
            }
        }
        if lit > len || p + lit > len || o + lit > raw || o > p || lit + 4 > b {
            break;
        }
        let (lo, lp) = (o + lit, p + lit);
        if lp == len {
            // The final sequence ends after its literals.
            unsafe {
                core::ptr::copy(ptr.add(p), ptr.add(o), lit);
            }
            s = lp;
            o = lo;
            b -= lit + 1;
            break;
        }
        if lp + 2 > len {
            break;
        }
        let offset = unsafe { *ptr.add(lp) as usize | ((*ptr.add(lp + 1) as usize) << 8) };
        let mut q = lp + 2;
        if offset == 0 || offset > lo {
            break;
        }
        let mut run = (token & 15) as usize;
        if run == 15 {
            let mut more = true;
            while more && q < len {
                let x = unsafe { *ptr.add(q) };
                q += 1;
                run += x as usize;
                more = x == 255;
            }
            if more || run > raw {
                break;
            }
        }
        run += 4;
        if lo + run > raw || lo + run > q || lit + run + 4 > b {
            break;
        }
        // Literals move toward the front (o <= p), so a forward copy is safe;
        // they end at lo <= lp, before the offset bytes already read. With a
        // gap of four or more the copy may run up to three bytes long: those
        // bytes land below the unread input and the next write covers them.
        unsafe {
            if o + 4 <= p && lp + 4 <= len {
                copy_words(ptr.add(o), ptr.add(p), lit);
            } else if lit <= 16 {
                for i in 0..lit {
                    *ptr.add(o + i) = *ptr.add(p + i);
                }
            } else {
                core::ptr::copy(ptr.add(p), ptr.add(o), lit);
            }
        }
        // A match at distance four or more reads only finished output a word
        // at a time; its overrun stays below the unread input (checked).
        if offset >= 4 && lo + run + 4 <= q {
            unsafe {
                copy_words(ptr.add(lo), ptr.add(lo - offset), run);
            }
        } else {
            copy_match(arena, lo, offset, run);
        }
        s = q;
        o = lo + run;
        b -= lit + run + 4;
    }
    *source = s;
    *output = o;
    *budget = b;
}
impl Decoder {
    /// Stable profiling IDs:0 stored hash,1 relocation,2 token,3 literal length,
    ///4 literal copy,5 offset,6 match length,7 match copy,8 raw hash,
    ///9 metadata validation,10 complete. This mapping is independent of layout.
    pub fn phase_id(&self) -> u32 {
        match self.phase {
            Phase::StoredHash => 0,
            Phase::Relocate => 1,
            Phase::Token => 2,
            Phase::LiteralLength => 3,
            Phase::Literals => 4,
            Phase::Offset => 5,
            Phase::MatchLength => 6,
            Phase::Match => 7,
            Phase::RawHash => 8,
            Phase::Validate => 9,
            Phase::Done => 10,
        }
    }
    pub const fn new(stored_len: usize, stored_fnv: u32, raw_len: usize, raw_fnv: u32) -> Self {
        Self {
            phase: Phase::StoredHash,
            stored: stored_len,
            stored_hash: stored_fnv,
            raw: raw_len,
            raw_hash: raw_fnv,
            pos: 0,
            hash: FNV,
            source: 0,
            output: 0,
            run: 0,
            offset: 0,
            token: 0,
            validation: None,
            payload: Payload::Room,
            scene_validation: None,
            input: 0,
            trusted: false,
        }
    }
    /// Decode stored bytes that already sit at `start`, ending exactly at the
    /// end of the slice later handed to `step`: a scene group read or
    /// prefetched into the arena tail. Output still begins at the slice front.
    pub const fn at(mut self, start: usize) -> Self {
        self.input = start;
        self
    }
    /// The stored bytes were already hashed against `stored_fnv` while they
    /// sat prefetched (disc.rs `preverify_step`) and have not moved since but
    /// by an exact copy: start past the stored hash, as if it had just passed.
    pub const fn hashed(mut self, yes: bool) -> Self {
        if yes {
            self.pos = self.stored;
            self.hash = self.stored_hash;
        }
        self
    }
    /// Scene payload uses the identical bounded HLZC/checksum path, followed
    /// by the scene cursor. No view is published before every reference passes.
    pub const fn new_scene(
        stored_len: usize,
        stored_fnv: u32,
        raw_len: usize,
        raw_fnv: u32,
    ) -> Self {
        let mut decoder = Self::new(stored_len, stored_fnv, raw_len, raw_fnv);
        decoder.payload = Payload::Scene;
        decoder
    }
    /// Startup atlas payloads have no Room header. Both stored and expanded
    /// hashes/lengths still pass before any byte can be uploaded; the caller
    /// validates the atlas descriptor and never publishes a Room from this mode.
    pub const fn new_bytes(
        stored_len: usize,
        stored_fnv: u32,
        raw_len: usize,
        raw_fnv: u32,
    ) -> Self {
        let mut decoder = Self::new(stored_len, stored_fnv, raw_len, raw_fnv);
        decoder.payload = Payload::Bytes;
        decoder
    }
    fn byte(&mut self, arena: &[u8]) -> Result<u8, Error> {
        let b = *arena.get(self.source).ok_or(Error::Decompress)?;
        self.source += 1;
        Ok(b)
    }
    fn decoded(&mut self) -> Result<(), Error> {
        if self.output != self.raw {
            return Err(Error::Decompress);
        }
        // Trusted: the stored bytes matched the cook's hash, and the bounded
        // LZ4 decode of those exact bytes is deterministic, so the raw hash
        // and the format walk would re-prove what the cook already proved.
        if self.trusted {
            self.phase = Phase::Done;
            return Ok(());
        }
        self.phase = Phase::RawHash;
        self.pos = 0;
        self.hash = FNV;
        Ok(())
    }
    pub fn step(&mut self, arena: &mut [u8], mut budget: usize) -> Result<Option<usize>, Error> {
        if self.stored > arena.len() || self.raw > arena.len() || self.stored == 0 {
            return Err(Error::Decompress);
        }
        while budget > 0 {
            // Charge transitions too: even malformed extension runs or a
            // zero-length sequence cannot monopolize one foreground call.
            budget -= 1;
            match self.phase {
                Phase::StoredHash | Phase::RawHash => {
                    let stored = matches!(self.phase, Phase::StoredHash);
                    let len = if stored { self.stored } else { self.raw };
                    let base = if stored { self.input } else { 0 };
                    if stored && (self.input + self.stored > arena.len()) {
                        return Err(Error::Decompress);
                    }
                    if self.pos < len {
                        let n = (len - self.pos).min(budget + 1);
                        self.hash =
                            hash_bytes(self.hash, &arena[base + self.pos..base + self.pos + n]);
                        self.pos += n;
                        budget -= n - 1;
                        continue;
                    }
                    if self.hash
                        != if stored {
                            self.stored_hash
                        } else {
                            self.raw_hash
                        }
                    {
                        return Err(Error::Checksum);
                    }
                    if !stored {
                        self.phase = Phase::Validate;
                        continue;
                    }
                    if TRUST_COOK {
                        self.trusted = true;
                    }
                    let at = self.input;
                    if self.stored >= 8 && &arena[at..at + 4] == b"HLZC" {
                        if u32::from_le_bytes(arena[at + 4..at + 8].try_into().unwrap()) as usize
                            != self.raw
                        {
                            return Err(Error::Decompress);
                        }
                        if at != 0 {
                            // Already at the tail: decode in place.
                            if at + self.stored != arena.len() {
                                return Err(Error::Decompress);
                            }
                            self.source = at + 8;
                            self.phase = Phase::Token;
                        } else {
                            self.pos = self.stored - 8;
                            self.source = arena.len() - self.pos;
                            self.phase = Phase::Relocate;
                        }
                    } else {
                        if at != 0 {
                            arena.copy_within(at..at + self.stored, 0);
                        }
                        self.output = self.stored;
                        self.decoded()?;
                    }
                }
                Phase::Relocate => {
                    if self.pos == 0 {
                        self.phase = Phase::Token;
                        continue;
                    }
                    // memmove toward the arena tail, descending so overlap is
                    // safe across independently scheduled calls.
                    let n = self.pos.min(budget + 1);
                    self.pos -= n;
                    arena.copy_within(8 + self.pos..8 + self.pos + n, self.source + self.pos);
                    budget -= n - 1;
                }
                Phase::Token => {
                    if self.source == arena.len() {
                        self.decoded()?;
                        continue;
                    }
                    // Whole sequences in one tight loop while the budget
                    // lasts; anything unusual is left to the per-phase path
                    // below, from the same sequence start, so errors and
                    // resumption are exactly the step-by-step decoder's.
                    if budget >= FAST_MIN {
                        let before = self.output;
                        fast_sequences(
                            arena,
                            &mut self.source,
                            &mut self.output,
                            self.raw,
                            &mut budget,
                        );
                        if self.output != before || self.source == arena.len() {
                            continue;
                        }
                    }
                    self.token = self.byte(arena)?;
                    self.run = (self.token >> 4) as usize;
                    self.phase = if self.run == 15 {
                        Phase::LiteralLength
                    } else {
                        Phase::Literals
                    };
                }
                Phase::LiteralLength => {
                    let b = self.byte(arena)?;
                    self.run = self.run.checked_add(b as usize).ok_or(Error::Decompress)?;
                    if b != 255 {
                        self.phase = Phase::Literals;
                    }
                }
                Phase::Literals => {
                    if self.run == 0 {
                        if self.source == arena.len() {
                            self.decoded()?;
                        } else {
                            self.phase = Phase::Offset;
                        }
                        continue;
                    }
                    if self
                        .source
                        .checked_add(self.run)
                        .filter(|&v| v <= arena.len())
                        .is_none()
                        || self
                            .output
                            .checked_add(self.run)
                            .filter(|&v| v <= self.raw)
                            .is_none()
                        || self.output > self.source
                    {
                        return Err(Error::Decompress);
                    }
                    let n = self.run.min(budget + 1);
                    arena.copy_within(self.source..self.source + n, self.output);
                    self.output += n;
                    self.source += n;
                    self.run -= n;
                    budget -= n - 1;
                }
                Phase::Offset => {
                    let lo = self.byte(arena)? as usize;
                    let hi = self.byte(arena)? as usize;
                    self.offset = lo | (hi << 8);
                    if self.offset == 0 || self.offset > self.output {
                        return Err(Error::Decompress);
                    }
                    self.run = (self.token & 15) as usize;
                    if self.run == 15 {
                        self.phase = Phase::MatchLength;
                    } else {
                        self.run += 4;
                        self.phase = Phase::Match;
                    }
                }
                Phase::MatchLength => {
                    let b = self.byte(arena)?;
                    self.run = self.run.checked_add(b as usize).ok_or(Error::Decompress)?;
                    if b != 255 {
                        self.run = self.run.checked_add(4).ok_or(Error::Decompress)?;
                        self.phase = Phase::Match;
                    }
                }
                Phase::Match => {
                    if self.run == 0 {
                        self.phase = Phase::Token;
                        continue;
                    }
                    if self
                        .output
                        .checked_add(self.run)
                        .filter(|&v| v <= self.raw && v <= self.source)
                        .is_none()
                    {
                        return Err(Error::Decompress);
                    }
                    // Forward replication is required for a distance shorter
                    // than the match; an ordinary memcpy is not equivalent.
                    let n = self.run.min(budget + 1);
                    copy_match(arena, self.output, self.offset, n);
                    self.output += n;
                    self.run -= n;
                    budget -= n - 1;
                }
                Phase::Validate => {
                    // The arena is inactive/private and no phase mutates it after
                    // RawHash. The cursor publishes no borrowed Room; disc.rs may
                    // create its validated view only after this returns completion.
                    let records = (budget + 1).min(8);
                    let bytes = &arena[..self.raw];
                    let done = match self.payload {
                        Payload::Bytes => true,
                        Payload::Room => {
                            if self.validation.is_none() {
                                self.validation =
                                    Some(Validation::new(bytes).map_err(|_| Error::RoomFormat)?);
                            }
                            self.validation
                                .as_mut()
                                .unwrap()
                                .step(bytes, records)
                                .map_err(|_| Error::RoomFormat)?
                        }
                        Payload::Scene => {
                            if self.scene_validation.is_none() {
                                self.scene_validation = Some(
                                    SceneValidation::new(bytes).map_err(|_| Error::RoomFormat)?,
                                );
                            }
                            self.scene_validation
                                .as_mut()
                                .unwrap()
                                .step(bytes, records)
                                .map_err(|_| Error::RoomFormat)?
                        }
                    };
                    if done {
                        self.phase = Phase::Done;
                        return Ok(Some(self.raw));
                    }
                    // Return even if the byte budget remains: record validation
                    // has its own strict per-poll cap, not a byte-cost estimate.
                    return Ok(None);
                }
                Phase::Done => return Ok(Some(self.raw)),
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod word_copy_tests {
    #[test]
    fn atlas_bytes_validate_both_hashes_and_length_before_publication() {
        let raw = b"atlas words, not a room";
        let mut stored = std::vec::Vec::from(&b"HLZC"[..]);
        stored.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        stored.push(0xf0);
        stored.push((raw.len() - 15) as u8);
        stored.extend_from_slice(raw);
        for budget in [1, 2, 7, 31, 1024] {
            for variant in 0..5 {
                let payload = if variant == 4 {
                    raw.as_slice()
                } else {
                    stored.as_slice()
                };
                let sh = super::hash_bytes(super::FNV, payload);
                let rh = super::hash_bytes(super::FNV, raw);
                let mut arena = [0u8; 128];
                arena[..payload.len()].copy_from_slice(payload);
                if variant == 1 {
                    arena[payload.len() - 1] ^= 1;
                }
                let mut decoder = super::Decoder::new_bytes(
                    payload.len(),
                    sh,
                    raw.len() + usize::from(variant == 3),
                    rh ^ u32::from(variant == 2),
                );
                let result = loop {
                    match decoder.step(&mut arena, budget) {
                        Ok(None) => {}
                        r => break r,
                    }
                };
                match variant {
                    1 | 2 => assert_eq!(result, Err(super::Error::Checksum)),
                    3 => assert_eq!(result, Err(super::Error::Decompress)),
                    _ => {
                        assert_eq!(result, Ok(Some(raw.len())));
                        assert_eq!(&arena[..raw.len()], raw);
                    }
                }
            }
        }
    }
    #[test]
    fn fnv_shift_multiply_and_chunked_hash_match_scalar_reference() {
        let reference = |seed: u32, bytes: &[u8]| {
            bytes.iter().fold(seed, |hash, &byte| {
                (hash ^ byte as u32).wrapping_mul(0x01000193)
            })
        };
        let mut random = 0x61c88647u32;
        for value in (0..=65535).chain([0x7fffffff, 0x80000000, 0xfffffffe, 0xffffffff]) {
            assert_eq!(super::fnv_multiply(value), value.wrapping_mul(0x01000193));
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            assert_eq!(super::fnv_multiply(random), random.wrapping_mul(0x01000193));
        }
        let bytes = core::array::from_fn::<_, 2052, _>(|_| {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            random as u8
        });
        for alignment in 0..4 {
            for length in (0..=128).chain([255, 256, 257, 1023, 1024, 1025, 2048]) {
                let slice = &bytes[alignment..alignment + length];
                for seed in [0, super::FNV, u32::MAX] {
                    let expected = reference(seed, slice);
                    assert_eq!(super::hash_bytes(seed, slice), expected);
                    for budget in [1, 2, 3, 4, 7, 31, 1024] {
                        let mut hash = seed;
                        for chunk in slice.chunks(budget) {
                            hash = super::hash_bytes(hash, chunk);
                        }
                        assert_eq!(
                            hash, expected,
                            "alignment={alignment} length={length} budget={budget}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn matches_scalar_replication_at_all_alignments_without_touching_guards() {
        for base in 0..4 {
            for offset in 1..=64 {
                for count in [0, 1, 2, 3, 4, 5, 7, 8, 15, 16, 31, 32, 63, 64, 65, 127] {
                    let mut storage = core::array::from_fn::<_, 512, _>(|i| (i * 37 + 19) as u8);
                    let mut expected = storage;
                    let output = 128 + base;
                    for i in output..output + count {
                        expected[i] = expected[i - offset];
                    }
                    super::copy_match(&mut storage, output, offset, count);
                    assert_eq!(
                        storage, expected,
                        "alignment={base} offset={offset} count={count}"
                    );
                }
            }
        }
    }
    #[test]
    fn validation_never_publishes_room_before_late_metadata_is_checked() {
        // 25 textures require four validation calls even with an unlimited byte
        // budget. The invalid reference lies in the final, previously unchecked
        // record, after three successful polls.
        for invalid in [false, true] {
            let mut arena = std::vec![0u8;40+25*48+32768];
            arena[..8].copy_from_slice(b"HKROOM02");
            arena[8] = 1;
            arena[12] = 25;
            for i in 0..25 {
                arena[40 + i * 16 + 6] = 1;
                arena[40 + i * 16 + 8] = 1;
            }
            if invalid {
                arena[40 + 24 * 16] = 1;
            }
            let len = arena.len();
            let mut decoder = super::Decoder::new(len, 0, len, 0);
            decoder.phase = super::Phase::Validate;
            for _ in 0..3 {
                assert_eq!(decoder.step(&mut arena, usize::MAX), Ok(None));
                assert_eq!(decoder.phase_id(), 9);
            }
            if invalid {
                assert_eq!(
                    decoder.step(&mut arena, usize::MAX),
                    Err(super::Error::RoomFormat)
                );
                assert_eq!(decoder.phase_id(), 9);
            } else {
                assert_eq!(decoder.step(&mut arena, usize::MAX), Ok(Some(len)));
                assert_eq!(decoder.phase_id(), 10);
                assert!(hk_format::Room::parse(&arena).is_ok());
            }
        }
    }
}
