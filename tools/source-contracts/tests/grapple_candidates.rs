//! Compare actual grapple query against the frozen ascending full-slot scan.

mod common;
use common::{index, index_from};

const TESTS: &str = r####"#[test]fn randomized_probe_batches_preserve_tile_priority_and_first_slot(){unsafe{let mut seed=0x87654321u32;for batch in 0..10000{for i in 0..MAX_OBJ{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let mut o=OBJ0;o.exists=seed&1!=0;o.destroyed=seed&2!=0;o.grapple_mode=((seed>>2)%7)as i32-3;o.x=fi(((seed>>8)%32)as i32-16);o.y=fi(((seed>>16)%32)as i32-16);o.hit_x=fi(-3);o.hit_y=fi(-3);o.hit_w=fi(8);o.hit_h=fi(8);if i%3==0{o.otype=ObjType::Snowball;}old::OBJ[i]=o;new::OBJ[i]=o;}
let mut a=old::cache();let mut b=new::cache();for q in 0..18{let tile=if q==17{2|(batch%2)*4}else{0};old::TILE=tile;new::TILE=tile;let x=Fix32::from_bits((q-9)*65536+(batch%65536));let y=fi(batch%32-16);assert_eq!(old::probe(0,x,y,&mut a),new::probe(0,x,y,&mut b));for i in 0..MAX_OBJ{assert!(old::OBJ[i]==new::OBJ[i],"object {i}, batch {batch}, probe {q}");}}}}}
#[test]fn every_slot_and_full_capacity(){unsafe{for count in 0..=MAX_OBJ{for i in 0..MAX_OBJ{let o=Obj{exists:i<count,grapple_mode:if i<count{1}else{0},hit_w:fi(8),hit_h:fi(8),..OBJ0};old::OBJ[i]=o;new::OBJ[i]=o;}for tile in [0,2,6]{old::TILE=tile;new::TILE=tile;let mut a=old::cache();let mut b=new::cache();assert_eq!(old::probe(223,fi(2),fi(2),&mut a),new::probe(223,fi(2),fi(2),&mut b));assert_eq!(old::OBJ[223].grapple_hit,new::OBJ[223].grapple_hit);}}}}
"####;

#[test]
fn ordered_grapple_candidate_queries() {
    let root = common::root();
    let s = common::read("games/celeste2/src/game.rs");
    let old =
        common::read("tools/fixtures/celeste2_grapple_check-8ed8.rs") + "unsafe fn player_bounce(";
    let start = index(&s, "#[derive(Clone, Copy, PartialEq)]\nenum ObjType");
    let common_end = index_from(&s, "static mut OBJ:", start);
    let types = s[start..common_end].replace(
        "#[derive(Clone, Copy)]",
        "#[derive(Clone, Copy, PartialEq)]",
    );
    let contains = &s[index(&s, "unsafe fn contains(")..index(&s, "unsafe fn check_solid(")];

    let scratch = common::Scratch::new("celeste-grapple-");
    std::fs::write(
        scratch.0.join("fixed.rs"),
        common::read("shared/src/fixed.rs"),
    )
    .unwrap();
    let mut unit = String::from("#![allow(dead_code,static_mut_refs)]\n");
    unit += &format!(
        "mod sin_table {{include!(\"{}/shared/src/sin_table.rs\");}}\n",
        root.display()
    );
    unit += "#[path=\"fixed.rs\"]mod fixed;use fixed::{Fix32,fx};fn fi(n:i32)->Fix32{Fix32::from_int(n)}\n";
    unit += &types;
    for (name, text) in [("old", old.as_str()), ("new", s.as_str())] {
        let query =
            &text[index(text, "unsafe fn grapple_check(")..index(text, "unsafe fn player_bounce(")];
        let helper = if name == "new" {
            &s[index(&s, "struct GrappleCandidates {")..index(&s, "/// grapple_check:")]
        } else {
            ""
        };
        unit += &format!(
            "mod {name} {{use super::*;pub static mut OBJ:[Obj;MAX_OBJ]=[OBJ0;MAX_OBJ];pub static mut TILE:i32=0;fn tile_at(_:i32,_:i32)->i32{{unsafe{{TILE}}}}mod backend{{pub fn fget(t:i32,f:i32)->bool{{t&(1<<f)!=0}}}}\n"
        );
        unit += contains;
        unit += &helper.replace(
            "struct GrappleCandidates {",
            "pub struct GrappleCandidates {",
        );
        unit += query;
        unit += if name == "new" {
            "pub type Cache=GrappleCandidates;pub fn cache()->Cache{Cache::new()}pub unsafe fn probe(i:usize,x:Fix32,y:Fix32,c:&mut Cache)->i32{grapple_check(i,x,y,c)}"
        } else {
            "pub struct Cache;pub fn cache()->Cache{Cache}pub unsafe fn probe(i:usize,x:Fix32,y:Fix32,_:&mut Cache)->i32{grapple_check(i,x,y)}"
        };
        unit += "}\n";
    }
    unit += TESTS;
    common::compile_and_run(
        &scratch,
        "grapple",
        &unit,
        &["--edition=2021", "--test", "-O"],
        &["--test-threads=1", "--nocapture"],
    );
}
