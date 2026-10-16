//! Shared helpers for the source-contract tests: the repository root, string
//! slicing that panics like Python's `str.index`, brace matching, and compiling
//! and running a generated Rust test program with the pinned toolchain.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root (this crate sits in `tools/source-contracts`).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

pub fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `text.index(pattern)`: byte offset of the first match, or a panic.
pub fn index(text: &str, pattern: &str) -> usize {
    text.find(pattern)
        .unwrap_or_else(|| panic!("substring not found: {pattern:?}"))
}

/// `text.index(pattern, from)`.
pub fn index_from(text: &str, pattern: &str, from: usize) -> usize {
    from + text[from..]
        .find(pattern)
        .unwrap_or_else(|| panic!("substring not found after {from}: {pattern:?}"))
}

/// A scratch directory removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(prefix: &str) -> Scratch {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path = std::env::temp_dir().join(format!("{prefix}{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&path).expect("scratch directory");
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Build the SDK's psx-math for the host into `scratch` and return the rustc
/// arguments that make `psx_math` visible to a generated program. The shared
/// fixed-point code uses its 64-bit-free helpers, so any test that includes
/// `shared/src/fixed.rs` needs it. The crate comes from the imported
/// components (`make psoxide`).
pub fn psx_math_args(scratch: &Scratch) -> Vec<String> {
    let lib = root().join(".psoxide/sdk/crates/psx-math/src/lib.rs");
    assert!(
        lib.exists(),
        "{} is missing: run `make psoxide` first",
        lib.display()
    );
    let built = Command::new("rustc")
        .args([
            "--edition=2021",
            "--crate-type=rlib",
            "--crate-name=psx_math",
        ])
        .args(["-O", "--cap-lints=allow", "--out-dir"])
        .arg(&scratch.0)
        .arg(&lib)
        .output()
        .expect("run rustc for psx-math");
    assert!(
        built.status.success(),
        "psx-math failed to build:\n{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    vec![
        "-L".to_string(),
        scratch.0.display().to_string(),
        "--extern".to_string(),
        format!("psx_math={}", scratch.0.join("libpsx_math.rlib").display()),
    ]
}

/// Write `source` as `<name>.rs` in `scratch`, compile it with `rustc` and run
/// the binary with `run_args`; panic with the output if either step fails.
pub fn compile_and_run(
    scratch: &Scratch,
    name: &str,
    source: &str,
    compile_args: &[&str],
    run_args: &[&str],
) {
    let program = scratch.0.join(format!("{name}.rs"));
    let binary = scratch.0.join(format!("{name}-tests"));
    std::fs::write(&program, source).expect("write generated program");
    let compiled = Command::new("rustc")
        .args(compile_args)
        .arg(&program)
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("run rustc");
    assert!(
        compiled.status.success(),
        "rustc failed:\n{}{}",
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new(&binary)
        .args(run_args)
        .output()
        .expect("run generated program");
    print!("{}", String::from_utf8_lossy(&ran.stdout));
    assert!(
        ran.status.success(),
        "generated tests failed:\n{}{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
}
