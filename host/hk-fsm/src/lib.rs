mod cook;
pub use cook::cook;
/// Regenerate the original Gruzzer graph used by the guest.
pub fn cook_gruzzer(root:&std::path::Path)->Result<(),Box<dyn std::error::Error>> {
 let source=hk_unity::Source::from_doctor(root)?;
 let file=source.file("level43")?;
 let object=source.object(&file,6396)?;
 let tree=source.read(&object)?;
 let bytes=cook(tree.get("fsm").ok_or("original Gruzzer FSM")?)?;
 std::fs::create_dir_all(root.join("data"))?;
 std::fs::write(root.join("data/gruzzer.fsm"),bytes)?;
 Ok(())
}
