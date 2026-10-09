//! Median-cut colour quantisation of RGB pixels.
//!
//! Distinct colours are counted, then boxes of colours are split until the
//! requested number of boxes exists: the most populated box first, along its
//! widest channel, at the median pixel. Each box becomes the average of its
//! pixels, and every pixel takes the nearest palette colour.
//!
//! Where several palette colours are equally near, the order of preference is
//! fixed by the tables below. The rules were fitted to the behaviour of the
//! tool this crate stands in for, observed from its outputs alone.

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
            let sum: u64 = self
                .entries
                .iter()
                .map(|e| e.color[c] as u64 * e.count)
                .sum();
            out[c] = ((sum + n / 2) / n) as u8;
        }
        out
    }
}

/// Binary max-heap of (pixel count, box id). Which of several equally
/// populated boxes comes out first follows from how the heap is kept:
/// a new item rises only past a strictly smaller parent, the larger child
/// (the left one on a tie) is picked by a strict comparison, and an item sinks
/// past a child that is larger or equal.
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
            let child = if r < n && self.items[l].0 < self.items[r].0 {
                r
            } else {
                l
            };
            if self.items[at].0 <= self.items[child].0 {
                self.items.swap(at, child);
                at = child;
            } else {
                break;
            }
        }
        top
    }
}

fn dist(a: &Px, b: &Px) -> u32 {
    (0..3)
        .map(|c| (a[c] as i32 - b[c] as i32).pow(2) as u32)
        .sum()
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
    let mut heap = Heap {
        items: vec![(arena[0].pixels(), 0)],
    };
    while (order.len() as u32) < colors {
        let Some((_, id)) = heap.pop() else { break };
        // A box of one colour waits in the heap like any other and is passed
        // over when its turn comes.
        if arena[id].entries.len() < 2 {
            continue;
        }
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
        heap.push((arena[first].pixels(), first));
        heap.push((arena[second].pixels(), second));
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
            let best = palette.iter().map(|q| dist(p, q)).min().unwrap();
            // Of the nearest entries the one closest to the entry of the box
            // the colour came from wins, the lowest slot when those tie too.
            let from = &palette[home[p]];
            (0..palette.len())
                .filter(|&k| dist(p, &palette[k]) == best)
                .min_by_key(|&k| (dist(from, &palette[k]), k))
                .unwrap() as u8
        })
        .collect();
    Some((palette, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eight reds 0, 3 .. 21, one pixel each: the palettes the reference tool
    /// gives for 2 to 8 colours. They fix which of several equally populated
    /// boxes is split next.
    #[test]
    fn equal_boxes_split_in_the_reference_order() {
        let px: Vec<Px> = (0..8).map(|i| [i * 3, 0, 0]).collect();
        let expected: [(u32, &[u8]); 7] = [
            (2, &[15, 3]),
            (3, &[18, 11, 3]),
            (4, &[18, 11, 5, 0]),
            (5, &[20, 15, 11, 5, 0]),
            (6, &[20, 15, 12, 9, 5, 0]),
            (7, &[21, 18, 15, 12, 9, 5, 0]),
            (8, &[21, 18, 15, 12, 9, 6, 3, 0]),
        ];
        for (colors, reds) in expected {
            let (palette, _) = median_cut(&px, colors).unwrap();
            assert_eq!(
                palette.iter().map(|p| p[0]).collect::<Vec<_>>(),
                reds,
                "{colors} colours"
            );
        }
    }

    #[test]
    fn every_pixel_takes_a_nearest_entry() {
        let px: Vec<Px> = vec![[0, 0, 0], [3, 0, 0], [5, 0, 0], [8, 0, 0], [2, 0, 0]];
        let (palette, index) = median_cut(&px, 3).unwrap();
        for (p, &i) in px.iter().zip(&index) {
            let d = |q: &Px| (p[0] as i32 - q[0] as i32).abs();
            assert_eq!(
                d(&palette[i as usize]),
                palette.iter().map(d).min().unwrap()
            );
        }
    }

    #[test]
    fn degenerate_inputs() {
        assert!(median_cut(&[], 4).is_none());
        assert!(median_cut(&[[1, 2, 3]], 0).is_none());
        let (palette, index) = median_cut(&[[9, 9, 9]; 5], 16).unwrap();
        assert_eq!((palette, index), (vec![[9, 9, 9]], vec![0; 5]));
    }
}
