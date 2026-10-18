//! Cook Greenpath's Hornet (Fungus1_04_boss) to `data/hornet3.hkf3`, `data/hornet3_fsm3.rs`, `data/hornet3_names.txt` and
//! `data/hornet3_families.txt`. The build does the same through `cook3::cook_all`; this runs it alone.
//!
//! fsm3-boss ROOT
//!
//! ROOT is the repository root: its `.hkpsx/doctor.json` names the Steam data directory.
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 2 {
        return Err("usage: fsm3-boss ROOT".into());
    }
    hk_fsm_cook::cook3::cook_hornet3(std::path::Path::new(&a[1]))?;
    println!("hornet3 cooked");
    Ok(())
}
