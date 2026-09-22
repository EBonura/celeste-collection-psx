#!/usr/bin/env python3
"""Compare actual grapple query against the frozen ascending full-slot scan."""
from pathlib import Path
import subprocess
W=Path(__file__).resolve().parents[1]
import tempfile
_TEMP=tempfile.TemporaryDirectory(prefix='celeste-grapple-')
R=Path(_TEMP.name)
s=(W/'games/celeste2/src/game.rs').read_text();old=(W/'tools/fixtures/celeste2_grapple_check-8ed8.rs').read_text()+'unsafe fn player_bounce('
start=s.index('#[derive(Clone, Copy, PartialEq)]\nenum ObjType')
common=s[start:s.index('static mut OBJ:',start)].replace('#[derive(Clone, Copy)]','#[derive(Clone, Copy, PartialEq)]')
contains=s[s.index('unsafe fn contains('):s.index('unsafe fn check_solid(')]
(R/'fixed.rs').write_text((W/'shared/src/fixed.rs').read_text())
unit='#![allow(dead_code,static_mut_refs)]\n'+f'mod sin_table {{include!("{W}/shared/src/sin_table.rs");}}\n'
unit+='#[path="fixed.rs"]mod fixed;use fixed::{Fix32,fx};fn fi(n:i32)->Fix32{Fix32::from_int(n)}\n'+common
for name,text in [('old',old),('new',s)]:
 query=text[text.index('unsafe fn grapple_check('):text.index('unsafe fn player_bounce(')]
 helper=s[s.index('struct GrappleCandidates {'):s.index('/// grapple_check:')] if name=='new' else ''
 unit+='mod '+name+' {use super::*;pub static mut OBJ:[Obj;MAX_OBJ]=[OBJ0;MAX_OBJ];pub static mut TILE:i32=0;fn tile_at(_:i32,_:i32)->i32{unsafe{TILE}}mod backend{pub fn fget(t:i32,f:i32)->bool{t&(1<<f)!=0}}\n'+contains+helper.replace('struct GrappleCandidates {','pub struct GrappleCandidates {')+query
 unit+=('pub type Cache=GrappleCandidates;pub fn cache()->Cache{Cache::new()}pub unsafe fn probe(i:usize,x:Fix32,y:Fix32,c:&mut Cache)->i32{grapple_check(i,x,y,c)}'if name=='new'else 'pub struct Cache;pub fn cache()->Cache{Cache}pub unsafe fn probe(i:usize,x:Fix32,y:Fix32,_:&mut Cache)->i32{grapple_check(i,x,y)}')+'}\n'
unit+='''#[test]fn randomized_probe_batches_preserve_tile_priority_and_first_slot(){unsafe{let mut seed=0x87654321u32;for batch in 0..10000{for i in 0..MAX_OBJ{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);let mut o=OBJ0;o.exists=seed&1!=0;o.destroyed=seed&2!=0;o.grapple_mode=((seed>>2)%7)as i32-3;o.x=fi(((seed>>8)%32)as i32-16);o.y=fi(((seed>>16)%32)as i32-16);o.hit_x=fi(-3);o.hit_y=fi(-3);o.hit_w=fi(8);o.hit_h=fi(8);if i%3==0{o.otype=ObjType::Snowball;}old::OBJ[i]=o;new::OBJ[i]=o;}
let mut a=old::cache();let mut b=new::cache();for q in 0..18{let tile=if q==17{2|(batch%2)*4}else{0};old::TILE=tile;new::TILE=tile;let x=Fix32::from_bits((q-9)*65536+(batch%65536));let y=fi(batch%32-16);assert_eq!(old::probe(0,x,y,&mut a),new::probe(0,x,y,&mut b));for i in 0..MAX_OBJ{assert!(old::OBJ[i]==new::OBJ[i],"object {i}, batch {batch}, probe {q}");}}}}}
#[test]fn every_slot_and_full_capacity(){unsafe{for count in 0..=MAX_OBJ{for i in 0..MAX_OBJ{let o=Obj{exists:i<count,grapple_mode:if i<count{1}else{0},hit_w:fi(8),hit_h:fi(8),..OBJ0};old::OBJ[i]=o;new::OBJ[i]=o;}for tile in [0,2,6]{old::TILE=tile;new::TILE=tile;let mut a=old::cache();let mut b=new::cache();assert_eq!(old::probe(223,fi(2),fi(2),&mut a),new::probe(223,fi(2),fi(2),&mut b));assert_eq!(old::OBJ[223].grapple_hit,new::OBJ[223].grapple_hit);}}}}
'''
(R/'grapple.rs').write_text(unit);subprocess.run(['rustc','--edition=2021','--test','-O',str(R/'grapple.rs'),'-o',str(R/'grapple-tests')],check=True);subprocess.run([str(R/'grapple-tests'),'--test-threads=1','--nocapture'],check=True)

_TEMP.cleanup()
