//! Bounded entry point for the canonical complete Mossman audio cooker.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    hk_cook::mossman_audio::run(&std::env::args().collect::<Vec<_>>())
}
