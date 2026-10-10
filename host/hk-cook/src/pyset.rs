//! CPython `set` iteration order, for the places the Python cookers iterate a set of small ints or
//! int pairs and the order reaches the output.

const PERTURB_SHIFT: u32 = 5;
const LINEAR_PROBES: usize = 9;
const MIN_SIZE: usize = 8;

/// A CPython `set` of keys with explicit hashes, enough to reproduce its iteration order.
#[derive(Clone)]
pub struct PySet<K: Clone + PartialEq> {
    table: Vec<Option<(u64, K)>>,
    fill: usize,
}

impl<K: Clone + PartialEq> PySet<K> {
    pub fn new() -> Self {
        PySet {
            table: vec![None; MIN_SIZE],
            fill: 0,
        }
    }

    fn mask(&self) -> usize {
        self.table.len() - 1
    }

    pub fn len(&self) -> usize {
        self.fill
    }

    pub fn contains(&self, key: &K, hash: u64) -> bool {
        self.table
            .iter()
            .flatten()
            .any(|e| e.0 == hash && e.1 == *key)
    }

    /// `set_insert_clean`: the key is known to be absent.
    fn insert_clean(table: &mut [Option<(u64, K)>], hash: u64, key: K) {
        let mask = table.len() - 1;
        let mut perturb = hash;
        let mut i = (hash as usize) & mask;
        loop {
            if table[i].is_none() {
                table[i] = Some((hash, key));
                return;
            }
            if i + LINEAR_PROBES <= mask {
                for j in 1..=LINEAR_PROBES {
                    if table[i + j].is_none() {
                        table[i + j] = Some((hash, key));
                        return;
                    }
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i
                .wrapping_mul(5)
                .wrapping_add(1)
                .wrapping_add(perturb as usize))
                & mask;
        }
    }

    /// `set_table_resize`.
    fn resize(&mut self, minused: usize) {
        let mut size = MIN_SIZE;
        while size <= minused {
            size <<= 1;
        }
        let old = std::mem::replace(&mut self.table, vec![None; size]);
        for (hash, key) in old.into_iter().flatten() {
            Self::insert_clean(&mut self.table, hash, key);
        }
    }

    /// `set_add_entry`.
    pub fn add(&mut self, key: K, hash: u64) {
        let mask = self.mask();
        let mut perturb = hash;
        let mut i = (hash as usize) & mask;
        loop {
            let probes = if i + LINEAR_PROBES <= mask {
                LINEAR_PROBES
            } else {
                0
            };
            for j in 0..=probes {
                match &self.table[i + j] {
                    None => {
                        self.table[i + j] = Some((hash, key));
                        self.fill += 1;
                        if self.fill * 5 >= mask * 3 {
                            self.resize(self.fill * 4);
                        }
                        return;
                    }
                    Some((h, k)) if *h == hash && *k == key => return,
                    _ => {}
                }
            }
            perturb >>= PERTURB_SHIFT;
            i = (i
                .wrapping_mul(5)
                .wrapping_add(1)
                .wrapping_add(perturb as usize))
                & mask;
        }
    }

    /// `set_merge` into a non-empty set (`a |= b`).
    pub fn merge(&mut self, other: &PySet<K>) {
        if other.fill == 0 {
            return;
        }
        if (self.fill + other.fill) * 5 >= self.mask() * 3 {
            self.resize((self.fill + other.fill) * 2);
        }
        for (hash, key) in other.table.iter().flatten() {
            self.add(key.clone(), *hash);
        }
    }

    /// Iteration order: the table, slot by slot.
    pub fn iter(&self) -> impl Iterator<Item = &K> {
        self.table.iter().flatten().map(|e| &e.1)
    }
}

impl<K: Clone + PartialEq> Default for PySet<K> {
    fn default() -> Self {
        Self::new()
    }
}

/// `hash(i)` for an int: the value modulo 2**61 - 1, with the sign kept and -1 mapped to -2.
pub fn int_hash(i: i64) -> u64 {
    const M: i64 = (1 << 61) - 1;
    let h = if i >= 0 { i % M } else { -((-i) % M) };
    (if h == -1 { -2 } else { h }) as u64
}
