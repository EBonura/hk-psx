use std::{env, fs};

fn main() {
    let mut checked = 0usize;
    for path in env::args().skip(1) {
        let bytes = fs::read(&path).expect("metadata bank");
        let bank = hk_format::WorldMeta::parse(&bytes).expect("checked HKWMTA01 bank");
        println!("{} scene={} regions={} objects={} bytes={}", path, bank.scene_id(), bank.region_count(), bank.object_count(), bytes.len());
        checked += 1;
    }
    assert!(checked != 0, "pass one or more HKWMTA01 banks");
}
