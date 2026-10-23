//! The curves of the title styles' animation clips (`sharedassets1.assets`: `GG_menu_disc_ebb`,
//! `waterfall`, `caustic_small_0`, `infected_blob_anim`), decoded from the streamed clip format:
//! every key carries the cubic of the segment that starts at it, `((c0 t + c1) t + c2) t + c3` with
//! `t` the time since the key. The same decoding the lab's `font-lab anim ... clip` prints.
use hk_unity::Value;
use std::collections::BTreeMap;

/// One animated property: the hash of the transform path it targets (CRC-32 of the path from the
/// animator), its binding attribute and the `(time, cubic)` of every key, in time order.
pub struct Curve {
    pub path: u32,
    pub attribute: u32,
    pub sub: usize,
    pub pptr: bool,
    pub keys: Vec<(f32, [f32; 4])>,
}

/// A decoded clip: its length in seconds, its sprite table (the object ids of its `pptrCurveMapping`)
/// and its streamed curves.
pub struct Clip {
    pub length: f32,
    pub sprites: Vec<i64>,
    pub curves: Vec<Curve>,
}

/// CRC-32 (the hash Unity gives animation paths and attributes).
pub fn crc32(s: &str) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in s.bytes() {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

fn words(v: Option<&Value>) -> Vec<u32> {
    v.and_then(|l| l.list()).map(|l| l.iter().map(|x| x.int().unwrap_or(0) as u32).collect()).unwrap_or_default()
}

/// Decode the streamed curves of an `AnimationClip`.
pub fn read(clip: &Value) -> Clip {
    let mc = clip.get("m_MuscleClip");
    let start = mc.and_then(|m| m.get("m_StartTime")).and_then(Value::float).unwrap_or(0.0);
    let stop = mc.and_then(|m| m.get("m_StopTime")).and_then(Value::float).unwrap_or(0.0);
    // Binding order: a transform position, rotation or scale binding owns 3 (or 4) consecutive curves.
    struct Binding {
        path: u32,
        attribute: u32,
        ty: i64,
        pptr: bool,
    }
    let bindings: Vec<Binding> = clip
        .get("m_ClipBindingConstant")
        .and_then(|b| b.get("genericBindings"))
        .and_then(|l| l.list())
        .map(|l| {
            l.iter()
                .map(|b| Binding {
                    path: b.get("path").and_then(Value::int).unwrap_or(0) as u32,
                    attribute: b.get("attribute").and_then(Value::int).unwrap_or(0) as u32,
                    ty: b.get("typeID").and_then(Value::int).unwrap_or(0),
                    pptr: b.get("isPPtrCurve").and_then(Value::int).unwrap_or(0) != 0,
                })
                .collect()
        })
        .unwrap_or_default();
    let mut slots: Vec<(usize, usize)> = Vec::new();
    for (i, b) in bindings.iter().enumerate() {
        let n = if b.ty == 4 {
            match b.attribute {
                1 | 3 | 4 => 3,
                2 => 4,
                _ => 1,
            }
        } else {
            1
        };
        for k in 0..n {
            slots.push((i, k));
        }
    }
    let data = mc.and_then(|m| m.get("m_Clip")).and_then(|c| c.get("data"));
    let streamed = words(data.and_then(|d| d.get("m_StreamedClip")).and_then(|s| s.get("data")));
    let mut per: BTreeMap<u32, Vec<(f32, [f32; 4])>> = BTreeMap::new();
    let mut p = 0usize;
    while p + 2 <= streamed.len() {
        let t = f32::from_bits(streamed[p]);
        let n = streamed[p + 1] as usize;
        p += 2;
        for _ in 0..n {
            if p + 5 > streamed.len() {
                break;
            }
            let c = |k: usize| f32::from_bits(streamed[p + k]);
            per.entry(streamed[p]).or_default().push((t, [c(1), c(2), c(3), c(4)]));
            p += 5;
        }
    }
    let sprites = clip
        .get("m_ClipBindingConstant")
        .and_then(|b| b.get("pptrCurveMapping"))
        .and_then(|l| l.list())
        .map(|l| l.iter().map(|p| p.get("m_PathID").and_then(Value::int).unwrap_or(0)).collect())
        .unwrap_or_default();
    let curves = per
        .into_iter()
        .filter_map(|(index, keys)| {
            let &(bi, sub) = slots.get(index as usize)?;
            let b = &bindings[bi];
            Some(Curve { path: b.path, attribute: b.attribute, sub, pptr: b.pptr, keys })
        })
        .collect();
    Clip { length: (stop - start) as f32, sprites, curves }
}

impl Curve {
    /// The value at `t` seconds (before the first key the first key's value; after the last, the
    /// last key's cubic carries on).
    pub fn at(&self, t: f32) -> f32 {
        let mut chosen = &self.keys[0];
        for key in &self.keys {
            if key.0 <= t {
                chosen = key;
            }
        }
        let dt = if chosen.0.is_finite() { t - chosen.0 } else { 0.0 }.max(0.0);
        let c = chosen.1;
        ((c[0] * dt + c[1]) * dt + c[2]) * dt + c[3]
    }
    /// The sprite table index a sprite curve shows at `t`.
    pub fn frame_at(&self, t: f32) -> usize {
        let mut shown = self.keys[0].1[3];
        for key in &self.keys {
            if key.0 <= t {
                shown = key.1[3];
            }
        }
        shown as usize
    }
}
