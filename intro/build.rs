//! Links the intro high in RAM.
//!
//! The SDK's linker script puts every program at 0x80010000, which is where
//! the game's image loads, so an intro linked there would be overwritten by
//! the very read that loads the game. The intro links at `HK_INTRO_LOAD_ADDR`
//! instead (hk-build sets it from the game's own image end) through the SDK's
//! own `LOAD_ADDR_OVERRIDE` hook: a `-T` file in OUT_DIR holding the one line,
//! then the pinned script unchanged. An SDK without the hook fails loudly.

use std::{env, fs, path::PathBuf};

const SDK_HOOK: &str = "DEFINED(LOAD_ADDR_OVERRIDE)";

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let psoxide = env::var("PSOXIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest.join("../.psoxide"));
    let ld = psoxide.join("sdk/psoxide.ld");
    let ld = ld.canonicalize().unwrap_or(ld);
    println!("cargo:rerun-if-changed={}", ld.display());
    println!("cargo:rerun-if-env-changed=HK_INTRO_LOAD_ADDR");
    println!("cargo:rerun-if-env-changed=HKPSX_LINK_MAP");

    let base = env::var("HK_INTRO_LOAD_ADDR").unwrap_or_else(|_| "0x801DB000".to_string());
    let sdk = fs::read_to_string(&ld).expect("read PSoXide linker script");
    assert!(
        sdk.contains(SDK_HOOK),
        "the SDK linker script has no LOAD_ADDR_OVERRIDE hook"
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("hk-intro-load.ld");
    fs::write(&out, format!("LOAD_ADDR_OVERRIDE = {base};\n"))
        .expect("write the intro's load address");
    println!("cargo:rustc-link-arg=-T{}", out.display());
    println!("cargo:rustc-link-arg=-T{}", ld.display());
    println!("cargo:rustc-link-arg=--oformat=binary");
    if let Ok(map) = env::var("HKPSX_LINK_MAP") {
        if !map.is_empty() {
            println!("cargo:rustc-link-arg=-Map={map}");
        }
    }
}
