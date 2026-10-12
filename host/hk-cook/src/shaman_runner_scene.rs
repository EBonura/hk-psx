//! Admit the original initially stopped Runner in the already cooked Temple.
use std::path::Path;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn cook(root: &Path) -> Result<()> {
    crate::mossman_scene::cook_selected(root, &[("level76", 44, &[933])], "shaman_runner_bank_adopted", "shaman_runner_actor_specs")
}
pub fn bind_generated_specs(root: &Path) -> Result<()> {
    crate::mossman_scene::bind_selected_specs(root, "shaman_runner_actor_specs", |s| s.to_string())
}
