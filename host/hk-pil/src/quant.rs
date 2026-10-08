//! `Image.quantize(colors, method=MEDIANCUT, kmeans=0)` on an RGB image
//! (libImaging/Quant.c `quantize`, QuantHeap.c).
//!
//! Two properties of the C code the port relies on and keeps:
//! * The pixel hash compares keys by their PIXEL_HASH value, so two colours
//!   with the same hash are one entry, keyed by the first one seen.
//! * Which entries land in which box depends only on channel values and
//!   counts, not on the order the hash table lists them in, so the lists are
//!   built from first-seen order instead of the C table's bucket order.

use std::collections::HashMap;

type Px = [u8; 3];

const MAX_HASH_ENTRIES: usize = 65536;

fn pixel_hash(p: Px) -> u32 {
    (p[0] as u32).wrapping_mul(463) ^ ((p[1] as u32) << 8).wrapping_mul(10069) ^ ((p[2] as u32) << 16).wrapping_mul(64997)
}

fn dist(a: Px, b: Px) -> u32 {
    let d = |i: usize| {
        let x = a[i] as i32 - b[i] as i32;
        (x * x) as u32
    };
    d(0).wrapping_add(d(1)).wrapping_add(d(2))
}

struct Node {
    l: Option<usize>,
    r: Option<usize>,
    /// Entry ids per channel, highest value first.
    lists: [Vec<usize>; 3],
    pixel_count: u32,
    volume: i32,
}

struct Cut<'a> {
    keys: &'a [Px],
    counts: &'a [u32],
    nodes: Vec<Node>,
}

impl Cut<'_> {
    fn volume(&mut self, n: usize) -> i32 {
        if self.nodes[n].volume >= 0 {
            return self.nodes[n].volume;
        }
        let node = &self.nodes[n];
        let v = if node.lists[0].is_empty() {
            0
        } else {
            let range = |i: usize| {
                let l = &node.lists[i];
                self.keys[l[0]][i] as i32 - self.keys[*l.last().unwrap()][i] as i32 + 1
            };
            range(0) * range(1) * range(2)
        };
        self.nodes[n].volume = v;
        v
    }

    fn split(&mut self, n: usize) {
        let keys = self.keys;
        let node = &self.nodes[n];
        let range = |i: usize| keys[node.lists[i][0]][i] as i32 - keys[*node.lists[i].last().unwrap()][i] as i32;
        let f = [range(0) * 77, range(1) * 150, range(2) * 29];
        let mut axis = 0;
        let mut best = f[0];
        for (i, &v) in f.iter().enumerate().skip(1) {
            if best < v {
                best = v;
                axis = i;
            }
        }
        // splitlists
        let list = &node.lists[axis];
        let pixel_count = node.pixel_count;
        let mut flag = HashMap::new();
        let (mut n0, mut n1) = (0u32, 0u32);
        let (mut n_left, mut n_right) = (0i32, 0i32);
        let mut left = 0u32;
        let mut c = 0;
        while c < list.len() {
            let e = list[c];
            left = left.wrapping_add(self.counts[e]);
            n0 = n0.wrapping_add(self.counts[e]);
            flag.insert(e, false);
            n_left += 1;
            c += 1;
            if left.wrapping_mul(2) > pixel_count {
                break;
            }
        }
        if c < list.len() {
            let split_value = keys[list[c - 1]][axis];
            while c < list.len() && keys[list[c]][axis] == split_value {
                flag.insert(list[c], false);
                n_left += 1;
                n0 = n0.wrapping_add(self.counts[list[c]]);
                c += 1;
            }
        }
        while c < list.len() {
            flag.insert(list[c], true);
            n_right += 1;
            n1 = n1.wrapping_add(self.counts[list[c]]);
            c += 1;
        }
        if n_right == 0 {
            let tail_value = keys[*list.last().unwrap()][axis];
            for &e in list.iter().rev() {
                if keys[e][axis] != tail_value {
                    break;
                }
                flag.insert(e, true);
                n_right += 1;
                n_left -= 1;
                n0 = n0.wrapping_sub(self.counts[e]);
                n1 = n1.wrapping_add(self.counts[e]);
            }
        }
        let _ = (n_left, n_right);
        let mut lists_l: [Vec<usize>; 3] = Default::default();
        let mut lists_r: [Vec<usize>; 3] = Default::default();
        for i in 0..3 {
            for &e in &self.nodes[n].lists[i] {
                if flag[&e] {
                    lists_r[i].push(e);
                } else {
                    lists_l[i].push(e);
                }
            }
        }
        let li = self.nodes.len();
        self.nodes.push(Node { l: None, r: None, lists: lists_l, pixel_count: n0, volume: -1 });
        self.nodes.push(Node { l: None, r: None, lists: lists_r, pixel_count: n1, volume: -1 });
        let node = &mut self.nodes[n];
        node.lists = Default::default();
        node.l = Some(li);
        node.r = Some(li + 1);
    }
}

/// QuantHeap: a 1-based max-heap of box ids by pixel count.
struct Heap {
    heap: Vec<usize>,
}

impl Heap {
    fn cmp(cut: &Cut, a: usize, b: usize) -> i32 {
        (cut.nodes[a].pixel_count as i32).wrapping_sub(cut.nodes[b].pixel_count as i32)
    }
    fn add(&mut self, cut: &Cut, val: usize) {
        self.heap.push(0);
        let mut k = self.heap.len() - 1;
        while k != 1 {
            if Self::cmp(cut, val, self.heap[k / 2]) <= 0 {
                break;
            }
            self.heap[k] = self.heap[k / 2];
            k >>= 1;
        }
        self.heap[k] = val;
    }
    fn remove(&mut self, cut: &Cut) -> Option<usize> {
        let count = self.heap.len() - 1;
        if count == 0 {
            return None;
        }
        let r = self.heap[1];
        let v = self.heap.pop().unwrap();
        let count = count - 1;
        let mut k = 1;
        while k * 2 <= count {
            let mut l = k * 2;
            if l < count && Self::cmp(cut, self.heap[l], self.heap[l + 1]) < 0 {
                l += 1;
            }
            if Self::cmp(cut, v, self.heap[l]) > 0 {
                break;
            }
            self.heap[k] = self.heap[l];
            k = l;
        }
        if k <= count {
            self.heap[k] = v;
        }
        Some(r)
    }
}

/// Median-cut quantization of RGB pixels into at most `colors` entries:
/// (palette, one index per pixel). None where Pillow fails.
pub fn median_cut(pixels: &[Px], colors: u32) -> Option<(Vec<Px>, Vec<u8>)> {
    if !(1..=256).contains(&colors) || pixels.is_empty() {
        return None;
    }
    // create_pixel_hash: entries keyed by PIXEL_HASH, first-seen key kept.
    let mut by_hash: HashMap<u32, usize> = HashMap::new();
    let mut keys: Vec<Px> = Vec::new();
    let mut counts: Vec<u32> = Vec::new();
    for &p in pixels {
        match by_hash.get(&pixel_hash(p)) {
            Some(&e) => counts[e] += 1,
            None => {
                by_hash.insert(pixel_hash(p), keys.len());
                keys.push(p);
                counts.push(1);
            }
        }
    }
    if keys.len() > MAX_HASH_ENTRIES {
        return None; // the rescaling path is not ported
    }
    let mut lists: [Vec<usize>; 3] = Default::default();
    for (i, l) in lists.iter_mut().enumerate() {
        *l = (0..keys.len()).collect();
        l.sort_by(|&a, &b| keys[b][i].cmp(&keys[a][i]));
    }
    let mut cut = Cut { keys: &keys, counts: &counts, nodes: vec![Node { l: None, r: None, lists, pixel_count: pixels.len() as u32, volume: -1 }] };
    let mut heap = Heap { heap: vec![0] };
    heap.add(&cut, 0);
    let mut n = colors as i32;
    'outer: loop {
        n -= 1;
        if n == 0 {
            break;
        }
        let this = loop {
            match heap.remove(&cut) {
                None => break 'outer,
                Some(b) if cut.volume(b) == 1 => continue,
                Some(b) => break b,
            }
        };
        cut.split(this);
        let (l, r) = (cut.nodes[this].l.unwrap(), cut.nodes[this].r.unwrap());
        heap.add(&cut, l);
        heap.add(&cut, r);
    }
    // annotate_hash_table: leaves in tree order get consecutive ids.
    let mut box_of: HashMap<u32, u32> = HashMap::new();
    let mut leaves: Vec<usize> = Vec::new();
    fn walk(cut: &Cut, n: usize, leaves: &mut Vec<usize>) {
        let node = &cut.nodes[n];
        if let (Some(l), Some(r)) = (node.l, node.r) {
            walk(cut, l, leaves);
            walk(cut, r, leaves);
        } else {
            leaves.push(n);
        }
    }
    walk(&cut, 0, &mut leaves);
    let mut entries = 0u32;
    for &leaf in &leaves {
        let list = &cut.nodes[leaf].lists[0];
        for &e in list {
            box_of.insert(pixel_hash(keys[e]), entries);
        }
        if !list.is_empty() {
            entries += 1;
        }
    }
    // compute_palette_from_median_cut (each pixel must sit in exactly one leaf box).
    let contained = |p: Px| {
        leaves
            .iter()
            .filter(|&&leaf| {
                let l = &cut.nodes[leaf].lists;
                !l[0].is_empty() && (0..3).all(|i| p[i] <= keys[l[i][0]][i] && p[i] >= keys[*l[i].last().unwrap()][i])
            })
            .count()
    };
    let mut sums = vec![[0u32; 3]; entries as usize];
    let mut count = vec![0u32; entries as usize];
    let mut contained_cache: HashMap<Px, usize> = HashMap::new();
    for &p in pixels {
        let c = *contained_cache.entry(p).or_insert_with(|| contained(p));
        if c > 1 {
            return None;
        }
        let e = *box_of.get(&pixel_hash(p))? as usize;
        for i in 0..3 {
            sums[e][i] = sums[e][i].wrapping_add(p[i] as u32);
        }
        count[e] += 1;
    }
    let palette: Vec<Px> = (0..entries as usize)
        .map(|e| {
            let c = |i: usize| (0.5 + sums[e][i] as f64 / count[e] as f64) as i32 as u8;
            [c(0), c(1), c(2)]
        })
        .collect();
    // build_distance_tables
    let ne = palette.len();
    let mut avg = vec![0u32; ne * ne];
    for i in 0..ne {
        for j in 0..i {
            let d = dist(palette[i], palette[j]);
            avg[i * ne + j] = d;
            avg[j * ne + i] = d;
        }
    }
    let mut order = vec![0usize; ne * ne];
    for i in 0..ne {
        let mut row: Vec<usize> = (0..ne).collect();
        row.sort_by(|&a, &b| avg[i * ne + a].cmp(&avg[i * ne + b]).then(a.cmp(&b)));
        order[i * ne..(i + 1) * ne].copy_from_slice(&row);
    }
    // map_image_pixels_from_median_box
    let mut cache: HashMap<Px, u8> = HashMap::new();
    let mut out = Vec::with_capacity(pixels.len());
    for &p in pixels {
        if let Some(&v) = cache.get(&p) {
            out.push(v);
            continue;
        }
        let start = *box_of.get(&pixel_hash(p))? as usize;
        let mut initial = dist(palette[start], p);
        let mut best = initial;
        let mut best_match = start;
        initial <<= 2;
        for &idx in &order[start * ne..(start + 1) * ne] {
            if avg[start * ne + idx] <= initial {
                let d = dist(palette[idx], p);
                if d < best {
                    best = d;
                    best_match = idx;
                }
            } else {
                break;
            }
        }
        cache.insert(p, best_match as u8);
        out.push(best_match as u8);
    }
    Some((palette, out))
}
