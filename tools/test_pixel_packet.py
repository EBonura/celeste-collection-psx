#!/usr/bin/env python3
"""Compare actual shared pset emission with actual inclusive rectangle emission.

Both use the same palette/transform/word-packing shims; the production functions
are extracted verbatim. The shader, primitive kind, coordinates and packet
order must remain identical, including i16 wrap and signed GP0 coordinates.
"""
from pathlib import Path
import subprocess
import tempfile
root=Path(__file__).resolve().parents[1]
s=(root/'shared/src/backend.rs').read_text()
def fn(name):
    a=s.index('pub fn '+name+'(');b=s.index('{',a);depth=1;i=b+1
    while depth:
        depth += (s[i]=='{')-(s[i]=='}');i+=1
    return s[a:i]
trans=s[s.index('fn sx(px: i16)'):s.index('/// Pixel scale:')]
unit=r'''#![allow(static_mut_refs)]
static mut SCALE:i16=2;static mut CAM_X:i16=0;static mut CAM_Y:i16=0;static mut V_OFS:i16=-8;
static mut PAL:[u8;16]=[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15];
static mut OUTPUT:[u32;3]=[0;3];
fn ofs_x()->i16{(320-128*unsafe{SCALE})/2}
fn rgb(c:i32)->(u8,u8,u8){let i=unsafe{PAL[(c as usize)&15]};(i*17,i*11,i*7)}
fn pack_color(r:u8,g:u8,b:u8)->u32{r as u32|((g as u32)<<8)|((b as u32)<<16)}
fn pack_vertex(x:i16,y:i16)->u32{x as u16 as u32|((y as u16 as u32)<<16)}
fn pack_xy(x:u16,y:u16)->u32{x as u32|((y as u32)<<16)}
fn emit(words:[u32;3]){unsafe{OUTPUT=words;}}
'''+trans+fn('rectfill')+'\n'+fn('pset')+r'''
unsafe fn check(x:i16,y:i16,c:i32){rectfill(x,y,x,y,c);let a=OUTPUT;pset(x,y,c);let b=OUTPUT;assert_eq!(a,b,"x={x}, y={y}, c={c}");}
#[test]fn all_coordinate_words_and_wrap_boundaries(){unsafe{for scale in [1,2]{SCALE=scale;for cam in [i16::MIN,-1,0,1,i16::MAX]{CAM_X=cam;CAM_Y=cam.wrapping_neg();for ofs in [i16::MIN,-8,0,56,i16::MAX]{V_OFS=ofs;for x in i16::MIN..=i16::MAX{check(x,x.wrapping_neg(),x as i32);}}}}}}
#[test]fn random_xy_palette_and_camera_states(){unsafe{let mut seed=0x57924613u32;fn next(s:&mut u32)->u32{*s=s.wrapping_mul(1664525).wrapping_add(1013904223);*s}for _ in 0..100000{SCALE=(next(&mut seed)%2+1)as i16;CAM_X=next(&mut seed)as i16;CAM_Y=next(&mut seed)as i16;V_OFS=next(&mut seed)as i16;for p in &mut PAL{*p=(next(&mut seed)%16)as u8;}let x=next(&mut seed)as i16;let y=next(&mut seed)as i16;let c=next(&mut seed)as i32;check(x,y,c);}}}
'''
with tempfile.TemporaryDirectory(prefix='celeste-pixel-packet-') as d:
    p=Path(d);(p/'test.rs').write_text(unit)
    subprocess.run(['rustc','--edition=2021','--test','-O','-C','overflow-checks=off',str(p/'test.rs'),'-o',str(p/'test')],check=True)
    subprocess.run([str(p/'test'),'--test-threads=1'],check=True)
