//! Test the actual no-std backend geometry without building console test
//! support.
//!
//! The production predicate and existing circle-run walker are read directly
//! from backend.rs. No alternate production implementation is maintained by
//! this test.

mod common;
use common::index;

#[test]
fn circle_visibility_against_existing_raster_spans() {
    let source = common::read("shared/src/backend.rs");
    let predicate = &source[index(&source, "fn disc_outside_view(")..];
    let predicate = &predicate[..index(predicate, "/// The packed merged runs")];
    let walker = &source[index(&source, "enum DiscRuns {")..];
    let walker = &walker[..index(walker, "/// PICO-8 `circ(x,y,r,c)`")];
    let tests = common::read("shared/src/backend_visibility_tests.rs");
    let mut unit =
        String::from("#![allow(dead_code, static_mut_refs)]\ntype ClipRect = (i16,i16,i16,i16);\n");
    unit += "const NO_CLIP: ClipRect = (i16::MIN,i16::MIN,i16::MAX,i16::MAX);\n";
    unit += predicate;
    unit += walker;
    unit += &tests;
    let scratch = common::Scratch::new("pico8-disc-visibility-");
    common::compile_and_run(
        &scratch,
        "tests",
        &unit,
        &["--edition=2021", "--test", "-O"],
        &["--nocapture"],
    );
}
