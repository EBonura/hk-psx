//! Median-cut colour quantisation of RGB pixels.
//!
//! Distinct colours are counted, then boxes of colours are split until the
//! requested number of boxes exists: always the most populated box that can
//! still be split, along its widest channel, at the median pixel.
//! Each box becomes the average of its pixels, and every pixel takes the
//! nearest palette colour.

use std::collections::HashMap;

type Px = [u8; 3];

struct Entry {
    color: Px,
    count: u64,
}

struct BoxOf {
    entries: Vec<Entry>,
}

impl BoxOf {
    fn pixels(&self) -> u64 {
        self.entries.iter().map(|e| e.count).sum()
    }

    /// The channel whose range of values, weighted by its luminance share
    /// (0.299, 0.587, 0.114 as integers), is widest; the first on a tie.
    fn widest(&self) -> usize {
        const WEIGHT: [u32; 3] = [77, 150, 29];
        let mut best = (0, 0u32);
        for c in 0..3 {
            let lo = self.entries.iter().map(|e| e.color[c]).min().unwrap();
            let hi = self.entries.iter().map(|e| e.color[c]).max().unwrap();
            let span = (hi - lo) as u32 * WEIGHT[c];
            if span > best.1 {
                best = (c, span);
            }
        }
        best.0
    }

    fn average(&self) -> Px {
        let n = self.pixels();
        let mut out = [0u8; 3];
        for c in 0..3 {
            let sum: u64 = self.entries.iter().map(|e| e.color[c] as u64 * e.count).sum();
            out[c] = ((sum + n / 2) / n) as u8;
        }
        out
    }
}

/// Max-heap of (pixel count, box id), strict comparisons throughout, so among
/// equal counts the box that has been waiting longer is served first.
struct Heap {
    items: Vec<(u64, usize)>,
}

impl Heap {
    fn push(&mut self, item: (u64, usize)) {
        self.items.push(item);
        let mut at = self.items.len() - 1;
        while at > 0 {
            let parent = (at - 1) / 2;
            if self.items[parent].0 < self.items[at].0 {
                self.items.swap(parent, at);
                at = parent;
            } else {
                break;
            }
        }
    }

    fn pop(&mut self) -> Option<(u64, usize)> {
        if self.items.is_empty() {
            return None;
        }
        let last = self.items.len() - 1;
        self.items.swap(0, last);
        let top = self.items.pop();
        let n = self.items.len();
        let mut at = 0;
        loop {
            let (l, r) = (2 * at + 1, 2 * at + 2);
            if l >= n {
                break;
            }
            let mut child = l;
            if r < n {
                let right_wins = self.items[l].0 < self.items[r].0;
                if right_wins {
                    child = r;
                }
            }
            if self.items[at].0 < self.items[child].0 {
                self.items.swap(at, child);
                at = child;
            } else {
                break;
            }
        }
        top
    }
}

/// Median-cut quantization of RGB pixels into at most `colors` entries:
/// (palette, one index per pixel). None where `colors` is zero.
pub fn median_cut(pixels: &[Px], colors: u32) -> Option<(Vec<Px>, Vec<u8>)> {
    if colors == 0 || colors > 256 || pixels.is_empty() {
        return None;
    }
    let mut seen: HashMap<Px, usize> = HashMap::new();
    let mut entries: Vec<Entry> = Vec::new();
    for &p in pixels {
        let at = *seen.entry(p).or_insert_with(|| {
            entries.push(Entry { color: p, count: 0 });
            entries.len() - 1
        });
        entries[at].count += 1;
    }
    // Boxes live in an arena; `order` lists them as they will appear in the
    // palette, a split box giving way to its two halves in place.
    let mut arena = vec![BoxOf { entries }];
    let mut order = vec![0usize];
    let mut heap = Heap { items: Vec::new() };
    if arena[0].entries.len() > 1 {
        heap.push((arena[0].pixels(), 0));
    }
    // Boxes of a single colour cannot be split and stay out of the heap.
    while (order.len() as u32) < colors {
        let Some((_, id)) = heap.pop() else { break };
        let axis = arena[id].widest();
        let mut sorted = std::mem::take(&mut arena[id].entries);
        sorted.sort_by_key(|e| std::cmp::Reverse(e.color[axis]));
        let total: u64 = sorted.iter().map(|e| e.count).sum();
        // Cut after the run of equal channel values that holds the median
        // pixel; equal values stay together, so when that run is the last one
        // the cut moves back before it.
        let mut run = 0;
        let mut cut = sorted.len();
        let mut k = 0;
        while k < sorted.len() {
            let value = sorted[k].color[axis];
            while k < sorted.len() && sorted[k].color[axis] == value {
                run += sorted[k].count;
                k += 1;
            }
            if run * 2 > total {
                cut = k;
                break;
            }
        }
        if cut == sorted.len() {
            let last = sorted[sorted.len() - 1].color[axis];
            cut = sorted.iter().position(|e| e.color[axis] == last).unwrap();
        }
        let upper = sorted.split_off(cut);
        let (first, second) = (arena.len(), arena.len() + 1);
        arena.push(BoxOf { entries: sorted });
        arena.push(BoxOf { entries: upper });
        for id_new in [first, second] {
            if arena[id_new].entries.len() > 1 {
                heap.push((arena[id_new].pixels(), id_new));
            }
        }
        let at = order.iter().position(|&o| o == id).unwrap();
        order.splice(at..=at, [first, second]);
    }
    let boxes: Vec<&BoxOf> = order.iter().map(|&o| &arena[o]).collect();
    let palette: Vec<Px> = boxes.iter().map(|b| b.average()).collect();
    let mut home: HashMap<Px, usize> = HashMap::new();
    for (k, b) in boxes.iter().enumerate() {
        for e in &b.entries {
            home.insert(e.color, k);
        }
    }
    let index: Vec<u8> = pixels
        .iter()
        .map(|p| {
            let dist = |q: &Px| -> u32 { (0..3).map(|c| (p[c] as i32 - q[c] as i32).pow(2) as u32).sum() };
            let best = palette.iter().map(dist).min().unwrap();
            let own = home[p];
            if dist(&palette[own]) == best {
                return own as u8;
            }
            let mut tied = (0..palette.len()).filter(|&k| dist(&palette[k]) == best);
            tied.next().unwrap() as u8
        })
        .collect();
    Some((palette, index))
}
