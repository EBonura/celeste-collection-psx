#!/usr/bin/env python3
"""Compare actual deferred circle packets with the frozen multiplication path.

Run from the repository root: python3 tools/test_disc_packets.py
Only the DMA sink is replaced. Both functions use the production circle table,
clip predicate and GP0 packing contract; the legacy transform is a frozen source
excerpt, not a reconstruction from the optimized implementation.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / "shared/src/backend.rs").read_text()


def section(start, end):
    return source[source.index(start):source.index(end, source.index(start))]


common = section("fn disc_outside_view(", "/// The packed merged runs")
common += section("unsafe fn disc_table(", "/// The immediate-mode disc")
common += section("enum DiscRuns {", "/// PICO-8 `circ(x,y,r,c)`")
legacy = (ROOT / "tools/fixtures/disc_fill_list-048a8.rs").read_text()
current = section("unsafe fn disc_fill_list<", "/// Reject only discs")
sink = r"""
static mut SCALE:i16=2;
static mut CAM_X:i16=0;
static mut CAM_Y:i16=0;
static mut V_OFS:i16=-8;
fn ofs_x()->i16 {(320-128*unsafe{SCALE})/2}
fn pack_vertex(x:i16,y:i16)->u32 {(x as u16 as u32)|((y as u16 as u32)<<16)}
fn pack_xy(x:u16,y:u16)->u32 {(x as u32)|((y as u32)<<16)}
struct Stream(Vec<Vec<u32>>);
static mut STREAM:Stream=Stream(Vec::new());
impl Stream {
    fn push_packet<const N:usize>(&mut self,p:[u32;N]) {self.0.push(p.to_vec());}
}
unsafe fn stream()->&'static mut Stream {&mut *core::ptr::addr_of_mut!(STREAM)}
pub unsafe fn draw(cx:i16,cy:i16,r:i16,clip:ClipRect,scale:i16,
                   camx:i16,camy:i16,ofs:i16,dither:bool)->Vec<Vec<u32>> {
    SCALE=scale; CAM_X=camx; CAM_Y=camy; V_OFS=ofs; STREAM.0.clear();
    if dither {
        disc_fill_list::<true>(cx,cy,r,clip,0x64808080,0x78200000)
    } else {
        disc_fill_list::<false>(cx,cy,r,clip,0x60efab45,0)
    }
    STREAM.0.clone()
}
"""
unit = "#![allow(dead_code,static_mut_refs)]\ntype ClipRect=(i16,i16,i16,i16);\n"
unit += "const NO_CLIP:ClipRect=(i16::MIN,i16::MIN,i16::MAX,i16::MAX);\n"
for name, implementation in [("old", legacy), ("new", current)]:
    unit += f"mod {name} {{ use super::*;" + sink + common + implementation + "}\n"
unit += r"""
#[test]
fn actual_renderer_packets_identical() { unsafe {
    let mut seed=0x12345678u32;
    for i in 0..50000 {
        seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let cx=seed as i16;
        seed=seed.rotate_left(7);
        let cy=seed as i16;
        let radius=(i%49)as i16;
        let scale=1+(i%2)as i16;
        let clip=if i%3==0 {NO_CLIP} else {(-128,-96,192,240)};
        let camx=if i%4==0 {cx} else {(i%260)as i16};
        let camy=if i%5==0 {cy} else {(i%280)as i16};
        assert_eq!(old::draw(cx,cy,radius,clip,scale,camx,camy,-8,i%2==0),
                   new::draw(cx,cy,radius,clip,scale,camx,camy,-8,i%2==0));
    }
    for coordinate in i16::MIN..=i16::MAX {
        for scale in [1,2] {
            assert_eq!(old::draw(coordinate,coordinate,2,NO_CLIP,scale,coordinate,coordinate,-8,true),
                       new::draw(coordinate,coordinate,2,NO_CLIP,scale,coordinate,coordinate,-8,true));
        }
    }
}}
"""
with tempfile.TemporaryDirectory(prefix="pico8-disc-packets-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(unit)
    subprocess.run(["rustc", "--edition=2021", "--test", "-O", str(path / "tests.rs"),
                    "-o", str(path / "tests")], check=True)
    subprocess.run([str(path / "tests"), "--nocapture"], check=True)
