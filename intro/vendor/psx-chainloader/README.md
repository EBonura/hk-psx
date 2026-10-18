Vendored from EBonura/PSoXide-editor `engine/crates/psx-chainloader` at ff9b1723 (the transport the demo disc,
Arcade and hl-psx's intro share). Only Cargo.toml differs: the dependency paths point into this repository's hydrated
`.psoxide` tree. hk-psx pins the SDK but not the editor, so the crate is carried here until the editor becomes a locked
component; replace this directory with that pin then.

Two changes from the editor crate, both for hk-psx's intro: `runtime::quiesce` is `pub` (the intro is entered from the running
game, not from the BIOS, and tears the machine down itself first), and a `payload_fnv` of zero skips the checksum comparison
(the intro loads the disc's own boot executable back, whose checksum the disc builder does not know).
