//! Check packed live-object lifecycle against the frozen full-tail removal.
//!
//! Actual production object types and initialization are shared; only the old
//! removal function is frozen. Dispatch initialization is mocked because it
//! does not change active-slot ownership. Live fields, order, capacity and
//! update-slot reiteration must remain exact; stale inactive payload is not a
//! game contract.

mod common;
use common::{index, index_from};

const COMPARE: &str = r####"unsafe fn compare(){let n=old::count();assert_eq!(n,new::count());for i in 0..n{assert!(old::OBJECTS[i]==new::OBJECTS[i],"live object {i}");}for i in n..30{assert!(!old::OBJECTS[i].active&&!new::OBJECTS[i].active);}}
#[test]fn every_capacity_and_removal_slot(){unsafe{for n in 1..=30{for slot in 0..n{old::clear();new::clear();for i in 0..n{assert_eq!(old::insert(OBJ_ORDER[i%18],i as i32),new::insert(OBJ_ORDER[i%18],i as i32));}old::remove(slot);new::remove(slot);compare();for j in 0..32{assert_eq!(old::insert(Smoke,j),new::insert(Smoke,j));compare();}}}}}
#[test]fn randomized_reentrant_slot_lifecycle(){unsafe{old::clear();new::clear();let mut seed=0x1324abcd_u32;for n in 0..50000{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let count=old::count();match seed%17{0=>{old::clear();new::clear()},1..=7 if count>0=>{let i=(seed as usize>>5)%count;old::remove(i);new::remove(i);},_=>{let t=OBJ_ORDER[(seed as usize>>5)%18];assert_eq!(old::insert(t,n),new::insert(t,n));}}if old::count()>0{let i=(seed as usize)%old::count();let k=(seed as usize>>8)%50;let part=Particle{x:fi(n),y:fi(-n),active:n%2==0,spd:fi((seed>>16)as i32),..PARTICLE0};old::OBJECTS[i].particles[k]=part;new::OBJECTS[i].particles[k]=part;}compare();}}}
#[test]fn removal_during_update_keeps_slot_reiteration(){unsafe{old::clear();new::clear();for i in 0..30{old::insert(Smoke,i);new::insert(Smoke,i);}let mut i=0;while i<30{if !old::OBJECTS[i].active{break;}let oldid=old::OBJECTS[i].id;let newid=new::OBJECTS[i].id;old::remove(i);new::remove(i);if i%3==0{old::insert(Player,i as i32);new::insert(Player,i as i32);}compare();assert_eq!(oldid!=old::OBJECTS[i].id&&old::OBJECTS[i].active,newid!=new::OBJECTS[i].id&&new::OBJECTS[i].active);i+=1;}compare();}}
"####;

const INSERT_REMOVE: &str = r####"pub unsafe fn insert(ty:ObjType,x:i32)->i32 {let p=init_object(ty,fi(x),fi(-x));if p.is_null(){-1}else{p.offset_from(objp(0))as i32}}
 pub unsafe fn remove(i:usize){destroy_object(objp(i));}
 pub unsafe fn clear(){for i in 0..MAX_OBJECTS{OBJECTS[i].active=false;}}
 pub unsafe fn count()->usize{OBJECTS.iter().take_while(|o|o.active).count()}
}
"####;

#[test]
fn packed_live_object_lifecycle() {
    let root = common::root();
    let s = common::read("games/celeste/src/game.rs");
    let legacy = common::read("tools/fixtures/classic_destroy_object-8ed8.rs");
    let a = index(&s, "unsafe fn destroy_object(");
    let b = index_from(&s, "// ---- hair", a);
    let old = format!("{}{}{}", &s[..a], legacy, &s[b..]);
    let common_start = index(&s, "// ---- value types");
    let common_end = index(&s, "// ---- global game state");
    let types = s[common_start..common_end].replace(
        "#[derive(Clone, Copy)]",
        "#[derive(Clone, Copy, PartialEq, Eq)]",
    );

    let scratch = common::Scratch::new("celeste-object-prefix-");
    std::fs::write(scratch.0.join("fixed.rs"), common::read("shared/src/fixed.rs")).unwrap();
    let mut unit = String::from("#![allow(dead_code,static_mut_refs)]\nuse core::ptr::addr_of_mut;\n");
    unit += &format!(
        "mod sin_table {{include!(\"{}/shared/src/sin_table.rs\");}}\n",
        root.display()
    );
    unit += "#[path=\"fixed.rs\"] mod fixed;use fixed::{Fix32,fx};fn fi(n:i32)->Fix32{Fix32::from_int(n)}\nconst MAX_OBJECTS:usize=30;\n";
    unit += &types;
    for (name, text) in [("old", old.as_str()), ("new", s.as_str())] {
        let from = index(text, "unsafe fn init_object(");
        let to = index(text, "// ---- hair");
        let funcs = &text[from..to];
        unit += &format!(
            "mod {name} {{use super::*;pub static mut OBJECTS:[Obj;30]=[OBJ0;30];static mut NEXT_ID:i16=0;static mut GOT_FRUIT:[bool;30]=[false;30];fn level_index()->i32{{0}}unsafe fn objp(i:usize)->*mut Obj{{addr_of_mut!(OBJECTS[i])}}unsafe fn dispatch_init(_: *mut Obj){{}}\n"
        );
        unit += funcs;
        unit += INSERT_REMOVE;
    }
    unit += COMPARE;
    common::compile_and_run(
        &scratch,
        "prefix",
        &unit,
        &["--edition=2021", "--test", "-O"],
        &["--test-threads=1", "--nocapture"],
    );
}
