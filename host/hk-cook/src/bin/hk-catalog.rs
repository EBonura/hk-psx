//! The committed scene catalog (docs/CATALOG.md, docs/catalog/scenes.tsv).
//!
//!   hk-catalog extract <world-import dir> <out tsv>
//!   hk-catalog render <in tsv> <out md>
//!   hk-catalog all <world-import dir>
//!
//! `all` writes both committed files under the repository root.
use hk_cook::catalog::{extract, Catalog};
use hk_cook::quality::SCENE_TABLE;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn extract_to(dir: &Path) -> Result<Catalog, String> {
    let names: Vec<&str> = SCENE_TABLE.iter().map(|r| r.scene_name).collect();
    extract(dir, &names)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let w = |path: &Path, text: &str| std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()));
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["extract", dir, out] => w(Path::new(out), &extract_to(Path::new(dir))?.to_tsv()),
        ["render", tsv, out] => {
            let text = std::fs::read_to_string(tsv).map_err(|e| format!("{tsv}: {e}"))?;
            w(Path::new(out), &Catalog::from_tsv(&text)?.render_markdown())
        }
        ["all", dir] => {
            let cat = extract_to(Path::new(dir))?;
            let tsv = root().join("docs/catalog/scenes.tsv");
            std::fs::create_dir_all(tsv.parent().unwrap()).map_err(|e| e.to_string())?;
            w(&tsv, &cat.to_tsv())?;
            // Render from the file just written, so the Markdown can only ever be a function of it.
            let text = std::fs::read_to_string(&tsv).map_err(|e| e.to_string())?;
            w(&root().join("docs/CATALOG.md"), &Catalog::from_tsv(&text)?.render_markdown())
        }
        _ => Err("usage: hk-catalog extract <world-import dir> <out tsv> | render <in tsv> <out md> | all <world-import dir>".into()),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hk-catalog: {e}");
        std::process::exit(1);
    }
}
