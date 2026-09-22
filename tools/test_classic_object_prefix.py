#!/usr/bin/env python3
"""Check packed live-object lifecycle against the frozen full-tail removal.

Actual production object types and initialization are shared; only the old
removal function is frozen. Dispatch initialization is mocked because it does
not change active-slot ownership. Live fields, order, capacity and update-slot
reiteration must remain exact; stale inactive payload is not a game contract.
"""
from pathlib import Path
import subprocess
W=Path(__file__).resolve().parents[1]
import tempfile
_TEMP=tempfile.TemporaryDirectory(prefix="celeste-object-prefix-")
R=Path(_TEMP.name)
s=(W/'games/celeste/src/game.rs').read_text()
legacy=(W/'tools/fixtures/classic_destroy_object-8ed8.rs').read_text()
a=s.index('unsafe fn destroy_object(');b=s.index('// ---- hair',a)
old=s[:a]+legacy+s[b:]
common=s[s.index('// ---- value types'):s.index('// ---- global game state')].replace('#[derive(Clone, Copy)]','#[derive(Clone, Copy, PartialEq, Eq)]')
fixed=(W/'shared/src/fixed.rs').read_text();(R/'fixed.rs').write_text(fixed)
unit='#![allow(dead_code,static_mut_refs)]\nuse core::ptr::addr_of_mut;\n'
unit+=f'mod sin_table {{include!("{W}/shared/src/sin_table.rs");}}\n'
unit+='#[path="fixed.rs"] mod fixed;use fixed::{Fix32,fx};fn fi(n:i32)->Fix32{Fix32::from_int(n)}\nconst MAX_OBJECTS:usize=30;\n'+common
for name,text in [('old',old),('new',s)]:
 funcs=text[text.index('unsafe fn init_object('):text.index('// ---- hair')]
 unit+='mod '+name+' {use super::*;pub static mut OBJECTS:[Obj;30]=[OBJ0;30];static mut NEXT_ID:i16=0;static mut GOT_FRUIT:[bool;30]=[false;30];fn level_index()->i32{0}unsafe fn objp(i:usize)->*mut Obj{addr_of_mut!(OBJECTS[i])}unsafe fn dispatch_init(_: *mut Obj){}\n'+funcs
 unit+='''pub unsafe fn insert(ty:ObjType,x:i32)->i32 {let p=init_object(ty,fi(x),fi(-x));if p.is_null(){-1}else{p.offset_from(objp(0))as i32}}
 pub unsafe fn remove(i:usize){destroy_object(objp(i));}
 pub unsafe fn clear(){for i in 0..MAX_OBJECTS{OBJECTS[i].active=false;}}
 pub unsafe fn count()->usize{OBJECTS.iter().take_while(|o|o.active).count()}
}\n'''
unit+='''unsafe fn compare(){let n=old::count();assert_eq!(n,new::count());for i in 0..n{assert!(old::OBJECTS[i]==new::OBJECTS[i],"live object {i}");}for i in n..30{assert!(!old::OBJECTS[i].active&&!new::OBJECTS[i].active);}}
#[test]fn every_capacity_and_removal_slot(){unsafe{for n in 1..=30{for slot in 0..n{old::clear();new::clear();for i in 0..n{assert_eq!(old::insert(OBJ_ORDER[i%18],i as i32),new::insert(OBJ_ORDER[i%18],i as i32));}old::remove(slot);new::remove(slot);compare();for j in 0..32{assert_eq!(old::insert(Smoke,j),new::insert(Smoke,j));compare();}}}}}
#[test]fn randomized_reentrant_slot_lifecycle(){unsafe{old::clear();new::clear();let mut seed=0x1324abcd_u32;for n in 0..50000{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let count=old::count();match seed%17{0=>{old::clear();new::clear()},1..=7 if count>0=>{let i=(seed as usize>>5)%count;old::remove(i);new::remove(i);},_=>{let t=OBJ_ORDER[(seed as usize>>5)%18];assert_eq!(old::insert(t,n),new::insert(t,n));}}if old::count()>0{let i=(seed as usize)%old::count();let k=(seed as usize>>8)%50;let part=Particle{x:fi(n),y:fi(-n),active:n%2==0,spd:fi((seed>>16)as i32),..PARTICLE0};old::OBJECTS[i].particles[k]=part;new::OBJECTS[i].particles[k]=part;}compare();}}}
#[test]fn removal_during_update_keeps_slot_reiteration(){unsafe{old::clear();new::clear();for i in 0..30{old::insert(Smoke,i);new::insert(Smoke,i);}let mut i=0;while i<30{if !old::OBJECTS[i].active{break;}let oldid=old::OBJECTS[i].id;let newid=new::OBJECTS[i].id;old::remove(i);new::remove(i);if i%3==0{old::insert(Player,i as i32);new::insert(Player,i as i32);}compare();assert_eq!(oldid!=old::OBJECTS[i].id&&old::OBJECTS[i].active,newid!=new::OBJECTS[i].id&&new::OBJECTS[i].active);i+=1;}compare();}}
'''
(R/'prefix.rs').write_text(unit)
subprocess.run(['rustc','--edition=2021','--test','-O',str(R/'prefix.rs'),'-o',str(R/'prefix-tests')],check=True)
subprocess.run([str(R/'prefix-tests'),'--test-threads=1','--nocapture'],check=True)

_TEMP.cleanup()
