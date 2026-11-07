//! Embeds every model in assets/models (built by art/build.py).

use std::fmt::Write;

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/models");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "glb")).collect())
        .unwrap_or_default();
    names.sort();
    let mut out = String::from("&[\n");
    for path in names {
        println!("cargo:rerun-if-changed={}", path.display());
        let pointer = std::fs::read(&path).map(|b| b.starts_with(b"version https://git-lfs")).unwrap_or(false);
        if pointer {
            panic!("{} is a Git LFS pointer, not a model: install Git LFS and run `git lfs pull`", path.display());
        }
        let name = path.file_stem().unwrap().to_string_lossy();
        writeln!(out, "    ({name:?}, include_bytes!({:?})),", path.display().to_string()).unwrap();
    }
    out.push(']');
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("models.rs"), out).unwrap();
}
