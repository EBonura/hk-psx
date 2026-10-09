//! The common per-tick trace both games are normalised into.
//!
//! One CSV per game: `tick` then channels. A channel is numeric unless its name
//! starts with `s.` (text). A blank cell means "not observed this tick".
//! Events are a second CSV: `tick,kind,name,value`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Default, Clone)]
pub struct Trace {
    pub ticks: Vec<i64>,
    pub num: BTreeMap<String, Vec<f64>>,
    pub text: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub tick: i64,
    pub kind: String,
    pub name: String,
    pub value: f64,
}

impl Trace {
    pub fn push_tick(&mut self, tick: i64) {
        self.ticks.push(tick);
        for v in self.num.values_mut() {
            v.push(f64::NAN);
        }
        for v in self.text.values_mut() {
            v.push(String::new());
        }
    }
    pub fn set(&mut self, chan: &str, v: f64) {
        let n = self.ticks.len();
        let col = self
            .num
            .entry(chan.to_string())
            .or_insert_with(|| vec![f64::NAN; n]);
        if let Some(l) = col.last_mut() {
            *l = v;
        }
    }
    pub fn set_text(&mut self, chan: &str, v: &str) {
        let n = self.ticks.len();
        let col = self
            .text
            .entry(chan.to_string())
            .or_insert_with(|| vec![String::new(); n]);
        if let Some(l) = col.last_mut() {
            *l = v.to_string();
        }
    }
    pub fn len(&self) -> usize {
        self.ticks.len()
    }
    pub fn index_of(&self, tick: i64) -> Option<usize> {
        self.ticks.binary_search(&tick).ok()
    }
    pub fn get(&self, chan: &str, tick: i64) -> Option<f64> {
        let i = self.index_of(tick)?;
        let v = *self.num.get(chan)?.get(i)?;
        if v.is_nan() {
            None
        } else {
            Some(v)
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let mut s = String::from("tick");
        for c in self.num.keys() {
            s.push(',');
            s.push_str(c);
        }
        for c in self.text.keys() {
            s.push(',');
            s.push_str(c);
        }
        s.push('\n');
        for i in 0..self.len() {
            s.push_str(&self.ticks[i].to_string());
            for v in self.num.values() {
                s.push(',');
                if !v[i].is_nan() {
                    s.push_str(&format!("{:.5}", v[i]));
                }
            }
            for v in self.text.values() {
                s.push(',');
                s.push_str(&v[i].replace(',', ";"));
            }
            s.push('\n');
        }
        fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
    }
    pub fn load(path: &Path) -> Result<Trace, String> {
        let t = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut lines = t.lines();
        let head: Vec<&str> = lines.next().ok_or("empty trace")?.split(',').collect();
        let mut tr = Trace::default();
        for h in &head[1..] {
            if h.starts_with("s.") {
                tr.text.insert(h.to_string(), vec![]);
            } else {
                tr.num.insert(h.to_string(), vec![]);
            }
        }
        for line in lines {
            let c: Vec<&str> = line.split(',').collect();
            if c.len() != head.len() {
                continue;
            }
            tr.ticks.push(c[0].parse().map_err(|_| "bad tick")?);
            for (i, h) in head.iter().enumerate().skip(1) {
                if h.starts_with("s.") {
                    tr.text.get_mut(*h).unwrap().push(c[i].to_string());
                } else {
                    tr.num
                        .get_mut(*h)
                        .unwrap()
                        .push(c[i].parse().unwrap_or(f64::NAN));
                }
            }
        }
        Ok(tr)
    }
}

pub fn save_events(path: &Path, ev: &[Event]) -> Result<(), String> {
    let mut s = String::from("tick,kind,name,value\n");
    for e in ev {
        s.push_str(&format!(
            "{},{},{},{}\n",
            e.tick,
            e.kind,
            e.name.replace(',', ";"),
            e.value
        ));
    }
    fs::write(path, s).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_events(path: &Path) -> Result<Vec<Event>, String> {
    let t = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(t.lines()
        .skip(1)
        .filter_map(|l| {
            let c: Vec<&str> = l.split(',').collect();
            if c.len() < 4 {
                return None;
            }
            Some(Event {
                tick: c[0].parse().ok()?,
                kind: c[1].into(),
                name: c[2].into(),
                value: c[3].parse().unwrap_or(0.0),
            })
        })
        .collect())
}
