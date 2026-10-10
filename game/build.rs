//! Instruction-cache placement for the renderer's hot code.
//!
//! The R3000's I-cache is 4 KiB and direct-mapped, so where the linker happens
//! to put `render::scenery_range` against the leaves its draw loop calls decides
//! how often they evict each other. Left to the default order, an unrelated
//! change elsewhere moved them into the same cache sets and cost gruz-fight 119
//! frames over two vblanks (I-cache refill stalls up 4%, the same instructions).
//! `hot-text-order.txt` lists those functions in the order LLD places them,
//! ahead of the rest of `.text`, so their relative layout no longer moves with
//! the rest of the program. The order came from per-instruction counts of
//! gruz-fight, chosen so the functions share as few hot cache sets as possible.
//!
//! The names are mangled and carry the crate's hash; hk-build checks after each
//! link that every listed symbol was placed in that order, and prints the list
//! under a new hash when the crate's hash changes. The file is copied under a
//! name holding a hash of its contents, so a new order changes the link
//! argument and cargo relinks. `PSOXIDE_LINK_ORDER` (psoxide-pgo's `+order`)
//! replaces it, as the SDK's hook asks.
fn main() {
    println!("cargo:rerun-if-env-changed=PSOXIDE_LINK_ORDER");
    if let Some(order) = std::env::var_os("PSOXIDE_LINK_ORDER") {
        println!(
            "cargo:rustc-link-arg=--symbol-ordering-file={}",
            order.to_string_lossy()
        );
        return;
    }
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("hot-text-order.txt");
    println!("cargo:rerun-if-changed={}", source.display());
    let text = std::fs::read(&source).expect("game/hot-text-order.txt");
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
