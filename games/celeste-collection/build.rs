//! Inject PSoXide's PSX linker script into the final link, by absolute
//! path derived from this crate's location. This keeps the game
//! buildable from anywhere (no brittle `-T../../..` relative paths in
//! RUSTFLAGS) while the script itself lives in the .psoxide tree that
//! `make psoxide` imports from components.lock.json.

use std::path::PathBuf;

fn main() {
    // .../pico8-psx/games/celeste  ->  repo root is two levels up.
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("game crate must live at <repo>/games/<name>");
    let ld = repo_root.join(".psoxide/sdk/psoxide.ld");
    let ld = ld.canonicalize().unwrap_or(ld);

    // `-T` selects the linker script; `--oformat=binary` dumps a flat
    // PSX-EXE image (the script lays out the executable header) instead
    // of an ELF.
    println!("cargo:rustc-link-arg=-T{}", ld.display());
    // `PSOXIDE_LINK_ELF=1` (set by the SDK's psoxide-pgo driver) keeps the ELF instead of
    // the flat PSX-EXE: same code at the same addresses, it does not boot.
    println!("cargo:rerun-if-env-changed=PSOXIDE_LINK_ELF");
    if std::env::var_os("PSOXIDE_LINK_ELF").is_none() {
        println!("cargo:rustc-link-arg=--oformat=binary");
    }
    // `PSOXIDE_LINK_ORDER` (set by psoxide-pgo for a `+order` variant) names a
    // symbol-ordering file for this link only.
    println!("cargo:rerun-if-env-changed=PSOXIDE_LINK_ORDER");
    if let Some(order) = std::env::var_os("PSOXIDE_LINK_ORDER") {
        println!(
            "cargo:rustc-link-arg=--symbol-ordering-file={}",
            order.to_string_lossy()
        );
    }
    if let Some(map) = std::env::var_os("PSX_LINK_MAP") {
        println!("cargo:rustc-link-arg=-Map={}", PathBuf::from(map).display());
    }
    println!("cargo:rerun-if-env-changed=PSX_LINK_MAP");
    println!("cargo:rerun-if-changed={}", ld.display());
}
