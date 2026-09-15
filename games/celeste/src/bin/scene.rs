//! Offline scene-capture disc: start in room SCENE="x,y" (build-time env,
//! default "0,0") for the cart-vs-port frame comparison. Not shipped in the game.
#![no_std]
#![no_main]
extern crate psx_rt;
const SCENE: &str = match option_env!("SCENE") {
    Some(s) => s,
    None => "0,0",
};
fn num(s: &str) -> i32 {
    s.bytes().fold(0, |n, b| n * 10 + (b - b'0') as i32)
}
#[no_mangle]
fn main() {
    let mut it = SCENE.split(',');
    let (x, y) = (num(it.next().unwrap_or("0")), num(it.next().unwrap_or("0")));
    loop {
        celeste::run_scene(x, y);
    }
}
