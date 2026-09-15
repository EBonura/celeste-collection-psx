//! Offline scene-capture disc: start in level SCENE (build-time env, default 1)
//! for the cart-vs-port frame comparison. Not shipped in the game.
#![no_std]
#![no_main]
extern crate psx_rt;
const SCENE: &str = match option_env!("SCENE") {
    Some(s) => s,
    None => "1",
};
fn num(s: &str) -> i32 {
    s.bytes().fold(0, |n, b| n * 10 + (b - b'0') as i32)
}
#[no_mangle]
fn main() {
    loop {
        celeste2::run_scene(num(SCENE));
    }
}
