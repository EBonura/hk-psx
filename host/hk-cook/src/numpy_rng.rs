//! The slice of `numpy.random.default_rng` that host/quantize.py uses: SeedSequence
//! seeding, the PCG64 stream, `choice(n, p=...)` and `choice(n, size, replace=False)`.
//! Written from the documented algorithms and checked against numpy's own output.

const INIT_A: u32 = 0x43b0_d7e5;
const MULT_A: u32 = 0x931e_8875;
const INIT_B: u32 = 0x8b51_f9dd;
const MULT_B: u32 = 0x58f3_8ded;
const MIX_MULT_L: u32 = 0xca01_f9dd;
const MIX_MULT_R: u32 = 0x4973_f715;
const XSHIFT: u32 = 16;
const POOL: usize = 4;

fn hashmix(value: u32, hash_const: &mut u32) -> u32 {
    let mut v = value ^ *hash_const;
    *hash_const = hash_const.wrapping_mul(MULT_A);
    v = v.wrapping_mul(*hash_const);
    v ^ (v >> XSHIFT)
}

fn mix(x: u32, y: u32) -> u32 {
    let r = MIX_MULT_L
        .wrapping_mul(x)
        .wrapping_sub(MIX_MULT_R.wrapping_mul(y));
    r ^ (r >> XSHIFT)
}

/// `SeedSequence(entropy).generate_state(4, uint64)` for a non-negative integer seed.
fn seed_state(entropy: u64) -> [u64; 4] {
    let mut words = vec![entropy as u32];
    if entropy >> 32 != 0 {
        words.push((entropy >> 32) as u32);
    }
    let mut pool = [0u32; POOL];
    let mut hash_const = INIT_A;
    for (i, slot) in pool.iter_mut().enumerate() {
        *slot = hashmix(words.get(i).copied().unwrap_or(0), &mut hash_const);
    }
    for src in 0..POOL {
        for dst in 0..POOL {
            if src != dst {
                let h = hashmix(pool[src], &mut hash_const);
                pool[dst] = mix(pool[dst], h);
            }
        }
    }
    for &word in words.iter().skip(POOL) {
        for slot in pool.iter_mut() {
            let h = hashmix(word, &mut hash_const);
            *slot = mix(*slot, h);
        }
    }
    let mut state32 = [0u32; 8];
    let mut hash_const = INIT_B;
    for (i, out) in state32.iter_mut().enumerate() {
        let mut v = pool[i % POOL] ^ hash_const;
        hash_const = hash_const.wrapping_mul(MULT_B);
        v = v.wrapping_mul(hash_const);
        *out = v ^ (v >> XSHIFT);
    }
    let mut out = [0u64; 4];
    for (i, o) in out.iter_mut().enumerate() {
        *o = state32[2 * i] as u64 | (state32[2 * i + 1] as u64) << 32;
    }
    out
}

const MULTIPLIER: u128 = 0x2360_ED05_1FC6_5DA4_4385_DF64_9FCC_F645;

pub struct Pcg64 {
    state: u128,
    inc: u128,
    has_uint32: bool,
    uinteger: u32,
}

impl Pcg64 {
    /// `np.random.default_rng(seed)` for an integer seed below 2^64.
    pub fn new(seed: u64) -> Pcg64 {
        let v = seed_state(seed);
        let initstate = (v[0] as u128) << 64 | v[1] as u128;
        let initseq = (v[2] as u128) << 64 | v[3] as u128;
        let mut rng = Pcg64 {
            state: 0,
            inc: (initseq << 1) | 1,
            has_uint32: false,
            uinteger: 0,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(initstate);
        rng.step();
        rng
    }

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
    }

    pub fn next_u64(&mut self) -> u64 {
        self.step();
        let s = self.state;
        ((s >> 64) as u64 ^ s as u64).rotate_right((s >> 122) as u32)
    }

    fn next_u32(&mut self) -> u32 {
        if self.has_uint32 {
            self.has_uint32 = false;
            return self.uinteger;
        }
        let next = self.next_u64();
        self.has_uint32 = true;
        self.uinteger = (next >> 32) as u32;
        next as u32
    }

    /// `rng.random()`: a double in [0, 1).
    pub fn random(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9007199254740992.0)
    }

    /// `random_bounded_uint64(0, rng)` by Lemire's method (the non-masked default).
    fn bounded(&mut self, rng: u64) -> u64 {
        if rng == 0 {
            return 0;
        }
        if rng <= 0xffff_ffff {
            if rng == 0xffff_ffff {
                return self.next_u32() as u64;
            }
            let rng_excl = rng as u32 + 1;
            let mut m = self.next_u32() as u64 * rng_excl as u64;
            let mut leftover = m as u32;
            if leftover < rng_excl {
                let threshold = (u32::MAX - rng as u32) % rng_excl;
                while leftover < threshold {
                    m = self.next_u32() as u64 * rng_excl as u64;
                    leftover = m as u32;
                }
            }
            return m >> 32;
        }
        let rng_excl = rng.wrapping_add(1);
        let mut m = self.next_u64() as u128 * rng_excl as u128;
        let mut leftover = m as u64;
        if leftover < rng_excl {
            let threshold = (u64::MAX - rng) % rng_excl;
            while leftover < threshold {
                m = self.next_u64() as u128 * rng_excl as u128;
                leftover = m as u64;
            }
        }
        (m >> 64) as u64
    }

    /// `rng.choice(len(p), p=p)`: one index drawn from the (already normalised) probabilities.
    pub fn choice_p(&mut self, p: &[f64]) -> usize {
        let mut cdf = Vec::with_capacity(p.len());
        let mut acc = 0.0;
        for &v in p {
            acc += v;
            cdf.push(acc);
        }
        let last = *cdf.last().unwrap();
        for v in &mut cdf {
            *v /= last;
        }
        let u = self.random();
        // searchsorted(side='right'): the first entry above u.
        cdf.partition_point(|&c| c <= u)
    }

    /// `rng.choice(pop_size, size, replace=False)`.
    pub fn choice_without_replacement(&mut self, pop_size: usize, size: usize) -> Vec<i64> {
        if pop_size > 10000 && size > pop_size / 50 {
            // Tail shuffle: shuffle the last `size` positions of arange(pop_size).
            let mut idx: Vec<i64> = (0..pop_size as i64).collect();
            let first = pop_size - size;
            for i in (first..pop_size).rev() {
                let j = self.bounded(i as u64) as usize;
                idx.swap(i, j);
            }
            return idx[pop_size - size..].to_vec();
        }
        // Floyd's algorithm over a hash set, then a shuffle.
        let mut idx = vec![0i64; size];
        let mask = {
            let set_size = (1.2 * size as f64) as u64;
            let mut m = set_size;
            m |= m >> 1;
            m |= m >> 2;
            m |= m >> 4;
            m |= m >> 8;
            m |= m >> 16;
            m |= m >> 32;
            m
        };
        let mut hash_set = vec![u64::MAX; (mask + 1) as usize];
        for j in (pop_size - size)..pop_size {
            let val = self.bounded(j as u64);
            let mut loc = (val & mask) as usize;
            while hash_set[loc] != u64::MAX && hash_set[loc] != val {
                loc = (loc + 1) & mask as usize;
            }
            if hash_set[loc] == u64::MAX {
                hash_set[loc] = val;
                idx[j - (pop_size - size)] = val as i64;
            } else {
                let mut loc = j & mask as usize;
                while hash_set[loc] != u64::MAX {
                    loc = (loc + 1) & mask as usize;
                }
                hash_set[loc] = j as u64;
                idx[j - (pop_size - size)] = j as i64;
            }
        }
        // `_shuffle_int` draws through random_bounded_uint64 (Lemire), unlike `Generator.shuffle`.
        for i in (1..idx.len()).rev() {
            let j = self.bounded(i as u64) as usize;
            idx.swap(i, j);
        }
        idx
    }
}
