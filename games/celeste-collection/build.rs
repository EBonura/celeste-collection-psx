//! Inject PSoXide's PSX linker scripts into the final link, by absolute path derived from this
//! crate's location, in this order: the top of RAM the collection links for
//! (`games/scene-ram.ld`, under the scene core's window), the scene core (`psoxide-core.ld`), the
//! boot-only memory (`psoxide-boot.ld`), psoxide.ld, and the handoff block's section
//! (`games/scene-block.ld`). The output is an ELF; the Makefile writes the PS-X EXE.

use std::path::PathBuf;

/// The `RAM_SIZE_OVERRIDE` line of `games/scene-ram.ld`, or `PSX_RAM_SIZE_OVERRIDE` when the
/// environment sets it.
fn ram_size(games: &std::path::Path) -> u32 {
    println!("cargo:rerun-if-env-changed=PSX_RAM_SIZE_OVERRIDE");
    let file = games.join("scene-ram.ld");
    println!("cargo:rerun-if-changed={}", file.display());
    let value = match std::env::var("PSX_RAM_SIZE_OVERRIDE") {
        Ok(v) => v,
        Err(_) => std::fs::read_to_string(&file)
            .expect("read games/scene-ram.ld")
            .lines()
            .find_map(|l| l.trim().strip_prefix("RAM_SIZE_OVERRIDE"))
            .and_then(|r| r.trim().strip_prefix('='))
            .map(|r| r.trim().trim_end_matches(';').trim().to_string())
            .expect("games/scene-ram.ld has no `RAM_SIZE_OVERRIDE = 0x...;` line"),
    };
    let size = u32::from_str_radix(value.trim_start_matches("0x"), 16)
        .expect("RAM_SIZE_OVERRIDE is a hex number");
    assert!(
        size % 0x100 == 0 && size <= 0x20_0000 - 0x100,
        "RAM_SIZE_OVERRIDE out of range"
    );
    size
}

fn main() {
    // .../games/celeste-collection  ->  repo root is two levels up.
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let games = manifest
        .parent()
        .expect("crate lives at <repo>/games/<name>");
    let repo_root = games.parent().expect("crate lives at <repo>/games/<name>");
    let sdk = repo_root.join(".psoxide/sdk");
    let core_ld = sdk.join("psoxide-core.ld");
    let boot_ld = sdk.join("psoxide-boot.ld");
    let ld = sdk.join("psoxide.ld");
    let ld = ld.canonicalize().unwrap_or(ld);
    let block_ld = games.join("scene-block.ld");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    // The top of RAM, as a `-T` file ahead of psoxide.ld so the link map records it.
    let ram = ram_size(games);
    let ram_ld = out.join("scene-ram.ld");
    std::fs::write(&ram_ld, format!("RAM_SIZE_OVERRIDE = {ram:#x};\n"))
        .expect("write scene-ram.ld");
    println!("cargo:rustc-env=SCENE_RAM_SIZE={ram:#x}");
    println!("cargo:rustc-link-arg=-T{}", ram_ld.display());

    // The core (`__core_boot` is the entry), then the boot-only memory for the splash
    // (psx-rt's `__boot_entry` moves it up after the core's boot; the script references it).
    println!("cargo:rustc-link-arg=-T{}", core_ld.display());
    println!("cargo:rustc-link-arg=-T{}", boot_ld.display());

    // The in-program scenes' entries (main.rs) are named by SCENES.TBL and referenced by nothing
    // else, so the linker is told to keep them.
    for entry in ["celeste_scene_menu", "celeste_scene_classic", "celeste_scene_lani"] {
        println!("cargo:rustc-link-arg=-u");
        println!("cargo:rustc-link-arg={entry}");
    }

    // `-T` selects the linker script; `--oformat=binary` dumps a flat
    // PSX-EXE image (the script lays out the executable header) instead
    // of an ELF.
    println!("cargo:rustc-link-arg=-T{}", ld.display());
    println!("cargo:rustc-link-arg=-T{}", block_ld.display());
    // The link is an ELF: the core's image and `.boot` are stored in one place and run from
    // another, which `--oformat=binary` cannot write. The Makefile writes the PS-X EXE from it
    // with the SDK's `psx-exe` (every section at its load address).
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
    for f in [&ld, &core_ld, &boot_ld, &block_ld] {
        println!("cargo:rerun-if-changed={}", f.display());
    }
}
