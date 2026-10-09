//! Compare actual shared pset emission with actual inclusive rectangle emission.
//!
//! Both use the same palette/transform/word-packing shims; the production
//! functions are extracted verbatim. The shader, primitive kind, coordinates and
//! packet order must remain identical, including i16 wrap and signed GP0
//! coordinates.

mod common;
use common::{index, index_from};

const PRELUDE: &str = r####"#![allow(static_mut_refs)]
static mut SCALE:i16=2;static mut CAM_X:i16=0;static mut CAM_Y:i16=0;static mut V_OFS:i16=-8;
static mut PAL:[u8;16]=[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15];
static mut OUTPUT:[u32;3]=[0;3];
fn ofs_x()->i16{(320-128*unsafe{SCALE})/2}
fn rgb(c:i32)->(u8,u8,u8){let i=unsafe{PAL[(c as usize)&15]};(i*17,i*11,i*7)}
fn pack_color(r:u8,g:u8,b:u8)->u32{r as u32|((g as u32)<<8)|((b as u32)<<16)}
fn pack_vertex(x:i16,y:i16)->u32{x as u16 as u32|((y as u16 as u32)<<16)}
fn pack_xy(x:u16,y:u16)->u32{x as u32|((y as u32)<<16)}
fn emit(words:[u32;3]){unsafe{OUTPUT=words;}}
"####;

const TESTS: &str = r####"
unsafe fn check(x:i16,y:i16,c:i32){rectfill(x,y,x,y,c);let a=OUTPUT;pset(x,y,c);let b=OUTPUT;assert_eq!(a,b,"x={x}, y={y}, c={c}");}
#[test]fn all_coordinate_words_and_wrap_boundaries(){unsafe{for scale in [1,2]{SCALE=scale;for cam in [i16::MIN,-1,0,1,i16::MAX]{CAM_X=cam;CAM_Y=cam.wrapping_neg();for ofs in [i16::MIN,-8,0,56,i16::MAX]{V_OFS=ofs;for x in i16::MIN..=i16::MAX{check(x,x.wrapping_neg(),x as i32);}}}}}}
#[test]fn random_xy_palette_and_camera_states(){unsafe{let mut seed=0x57924613u32;fn next(s:&mut u32)->u32{*s=s.wrapping_mul(1664525).wrapping_add(1013904223);*s}for _ in 0..100000{SCALE=(next(&mut seed)%2+1)as i16;CAM_X=next(&mut seed)as i16;CAM_Y=next(&mut seed)as i16;V_OFS=next(&mut seed)as i16;for p in &mut PAL{*p=(next(&mut seed)%16)as u8;}let x=next(&mut seed)as i16;let y=next(&mut seed)as i16;let c=next(&mut seed)as i32;check(x,y,c);}}}
"####;

/// The `pub fn <name>(...) { ... }` item, through its matching brace.
fn function<'a>(source: &'a str, name: &str) -> &'a str {
    let a = index(source, &format!("pub fn {name}("));
    let b = index_from(source, "{", a);
    let mut depth = 1i32;
    let mut i = b + 1;
    let bytes = source.as_bytes();
    while depth != 0 {
        depth += i32::from(bytes[i] == b'{') - i32::from(bytes[i] == b'}');
        i += 1;
    }
    &source[a..i]
}

#[test]
fn shared_pset_matches_inclusive_rectangle() {
    let s = common::read("shared/src/backend.rs");
    let trans = &s[index(&s, "fn sx(px: i16)")..index(&s, "/// Pixel scale:")];
    let unit = format!(
        "{PRELUDE}{trans}{}\n{}{TESTS}",
        function(&s, "rectfill"),
        function(&s, "pset")
    );
    let scratch = common::Scratch::new("celeste-pixel-packet-");
    common::compile_and_run(
        &scratch,
        "test",
        &unit,
        &["--edition=2021", "--test", "-O", "-C", "overflow-checks=off"],
        &["--test-threads=1"],
    );
}
