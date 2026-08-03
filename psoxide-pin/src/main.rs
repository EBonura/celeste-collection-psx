//! Hydrate the pinned PSoXide into `.psoxide`. Run by `make psoxide`.

use std::path::PathBuf;
use std::process::ExitCode;

/// Must match the rev in Cargo.toml: it is what the hydrated tree is stamped
/// with, so an unchanged pin skips the copy.
const REV: &str = "b53c668911c41b9933110b55a3f6179bebc03467";

fn main() -> ExitCode {
    let into = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("../.psoxide"));
    match psoxide_link::hydrate_pinned(&into, REV, true) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("psoxide-pin: {error}");
            ExitCode::FAILURE
        }
    }
}
