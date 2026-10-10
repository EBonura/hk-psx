//! Optional instruction-cache placement for the renderer's hot code.
//!
//! The R3000's I-cache is 4 KiB and direct-mapped, so text placement can
//! change frame pacing. A comment-only list selects ordinary linker placement;
//! an explicit list places its symbols before the rest of the text.
//!
//! For a nonempty list, hk-build checks that all mangled symbols were placed
//! in that order. The file is copied under a content-hashed name to force a
//! relink when it changes. `PSOXIDE_LINK_ORDER` (psoxide-pgo's `+order`)
//! replaces the built-in list, as the SDK's hook asks.
fn main() {
    println!("cargo:rerun-if-env-changed=PSOXIDE_LINK_ORDER");
    if let Some(order) = std::env::var_os("PSOXIDE_LINK_ORDER") {
        println!("cargo:rerun-if-changed={}", order.to_string_lossy());
        println!(
            "cargo:rustc-link-arg=--symbol-ordering-file={}",
            order.to_string_lossy()
        );
        return;
    }
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("hot-text-order.txt");
    println!("cargo:rerun-if-changed={}", source.display());
    let text = std::fs::read(&source).expect("game/hot-text-order.txt");
    let has_symbols = std::str::from_utf8(&text)
        .expect("game/hot-text-order.txt is UTF-8")
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    if !has_symbols {
        return;
    }
    // FNV-1a over the list: the copy's name changes whenever its contents do.
    let hash = text.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join(format!("hot-text-order-{hash:016x}.txt"));
    std::fs::write(&out, &text).expect("copy of hot-text-order.txt");
    println!(
        "cargo:rustc-link-arg=--symbol-ordering-file={}",
        out.display()
    );
}
