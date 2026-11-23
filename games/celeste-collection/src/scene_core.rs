//! The seam between the collection and the SDK's scene core: every place the collection touches the
//! core goes through this module, so when the core lands only this file changes.
//!
//! The core (scene-system CORE-ABI.md and PLAN.md "Modular residency") owns a window at the top of
//! RAM and the handoff block at 0x801FFF00, and loads images that hold scenes. The collection is one
//! image, resident from boot, holding three scenes (the menu, Classic, Lani), so a switch between
//! them is a call with no disc read, as before the core. The core does not exist yet, so today:
//! - [`switch_to`] records the switch in the handoff block, in the ABI's layout, where the core
//!   would; the scene dispatch in `main` then calls the scene's entry. Under the core a resident
//!   scene's switch is the core's jump to its entry.
//! - a cold entry (no valid block left by a switch: a BIOS boot, a reset, a foreign loader) reads
//!   the memory card and writes a fresh block, so every setting has one home a later scene or
//!   image can read.
//!
//! What the collection keeps of its own (the core never holds it): exception handler, VBlank
//! counter, presenter, pad, card, SPU driver.

use pico8::settings_codec::{Settings, RECORD_LEN};
use pico8::{backend, debug, save};

/// A scene id: the row of the scene table (CORE-ABI.md section 1).
pub type SceneId = u16;
/// The cover menu, the settings screen and the credits.
pub const MENU: SceneId = 0;
/// Celeste Classic.
pub const CLASSIC: SceneId = 1;
/// Celeste 2: Lani's Trek.
pub const LANI: SceneId = 2;
/// "No scene": the previous scene after a cold entry.
const NONE: SceneId = 0xffff;

/// The handoff block, 0x801FFF00..0x80200000 (CORE-ABI.md section 2).
const BLOCK: usize = 0x801f_ff00;
const MAGIC: u32 = 0x4243_5350; // "PSCB"
const VERSION: u32 = 1;
const LENGTH: u32 = 0x100;
const OFF_MAGIC: usize = 0x00;
const OFF_VERSION_LENGTH: usize = 0x04;
const OFF_CORE_HASH: usize = 0x08;
const OFF_LBA_OFFSET: usize = 0x0c;
const OFF_TRACK_BASE: usize = 0x10;
const OFF_SCENES: usize = 0x14; // current (low half), previous (high half)
const OFF_FLAGS: usize = 0x18;
const OFF_SWITCHES: usize = 0x1c;
const OFF_SETTINGS: usize = 0x30;
const SETTINGS_BYTES: usize = 64;
const OFF_PRIVATE: usize = 0x80;
const OFF_NOT_MAGIC: usize = 0xfc;
/// Flag bit 0: the scene was entered by a switch.
const FLAG_SWITCHED: u32 = 1 << 0;

/// The collection's use of the scene-private area: one version byte, then the cover menu's last
/// cart, the debug fly flag and the screen-follow option.
const PRIVATE_VERSION: u8 = 1;
const PRIV_VERSION: usize = 0;
const PRIV_LAST_CART: usize = 1;
const PRIV_FLY: usize = 2;
const PRIV_FOLLOW: usize = 3;

const _: () = assert!(RECORD_LEN <= SETTINGS_BYTES);

/// Top of RAM the collection links for, the one build parameter (games/scene-ram.ld).
const RAM_SIZE: u32 = parse_hex(env!("SCENE_RAM_SIZE"));
/// The initial stack pointer: 100h below the top of RAM, as psoxide.ld puts it.
const STACK_INIT: u32 = 0x8000_0000 + RAM_SIZE - 0x100;
/// `STACK_INIT` as `lui` + `addiu` halves (the assembler's `%hi` of a constant this large
/// evaluated to 0).
const STACK_HI: u32 = STACK_INIT.wrapping_add(0x8000) >> 16;
const STACK_LO: i32 = STACK_INIT as u16 as i16 as i32;

/// The handoff block as the link map names it (games/scene-block.ld places `.scene_block` at
/// 0x801FFF00, NOLOAD): the harness and the journeys read `SCENE_BLOCK` + offset. Only its address
/// is used; every access goes through `BLOCK`.
#[no_mangle]
#[link_section = ".scene_block"]
#[used]
pub static mut SCENE_BLOCK: [u32; 64] = [0; 64];

const fn parse_hex(s: &str) -> u32 {
    let b = s.as_bytes();
    let mut i = 2; // "0x"
    let mut v = 0u32;
    while i < b.len() {
        let d = match b[i] {
            c @ b'0'..=b'9' => c - b'0',
            c @ b'a'..=b'f' => c - b'a' + 10,
            c @ b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("bad hex digit"),
        };
        v = v * 16 + d as u32;
        i += 1;
    }
    v
}

fn read(off: usize) -> u32 {
    // SAFETY: the handoff block is RAM above the stack, owned by the scene system.
    unsafe { core::ptr::read_volatile((BLOCK + off) as *const u32) }
}

fn write(off: usize, value: u32) {
    // SAFETY: as `read`.
    unsafe { core::ptr::write_volatile((BLOCK + off) as *mut u32, value) }
}

fn read_byte(off: usize) -> u8 {
    // SAFETY: as `read`.
    unsafe { core::ptr::read_volatile((BLOCK + off) as *const u8) }
}

fn write_byte(off: usize, value: u8) {
    // SAFETY: as `read`.
    unsafe { core::ptr::write_volatile((BLOCK + off) as *mut u8, value) }
}

/// Whether the block is one this image can read (magic, `!magic`, version, private layout).
fn valid() -> bool {
    read(OFF_MAGIC) == MAGIC
        && read(OFF_NOT_MAGIC) == !MAGIC
        && read(OFF_VERSION_LENGTH) & 0xffff >= VERSION
        && read_byte(OFF_PRIVATE + PRIV_VERSION) == PRIVATE_VERSION
}

/// How a scene was entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The scene that switched here, or `None` on a cold entry.
    pub previous: Option<SceneId>,
}

/// Call first in `main`, with the scene the image starts in. With a valid block left by a switch
/// (flag bit 0, this scene current) it applies the block's settings; otherwise (a BIOS boot, a
/// reset with a stale block in RAM, a foreign loader) it reads the memory card, as the collection
/// always did at boot, and writes a fresh block. `previous` is `None` exactly on that second path,
/// which is when the menu plays the splash.
// Runs once, at the start of `main`: cold code, with `settings`.
#[link_section = ".psx_cold.scene_entered"]
#[inline(never)]
pub fn scene_entered(id: SceneId) -> Entry {
    if valid() && read(OFF_FLAGS) & FLAG_SWITCHED != 0 && current_scene() == id {
        settings();
        write(OFF_FLAGS, read(OFF_FLAGS) & !FLAG_SWITCHED);
        let previous = (read(OFF_SCENES) >> 16) as SceneId;
        return Entry {
            previous: (previous != NONE).then_some(previous),
        };
    }
    save::load(); // no card or no file keeps the defaults
    for off in (0..LENGTH as usize).step_by(4) {
        write(off, 0);
    }
    write(OFF_MAGIC, MAGIC);
    write(OFF_VERSION_LENGTH, VERSION | LENGTH << 16);
    write(OFF_CORE_HASH, 0); // no core yet
    write(OFF_LBA_OFFSET, psx_io::disc_base::lba_offset());
    write(OFF_TRACK_BASE, psx_io::disc_base::cdda_track_base() as u32);
    write(OFF_SCENES, id as u32 | (NONE as u32) << 16);
    write_byte(OFF_PRIVATE + PRIV_VERSION, PRIVATE_VERSION);
    store_settings();
    write(OFF_NOT_MAGIC, !MAGIC);
    Entry { previous: None }
}

/// Make the block's settings the live ones: the psx-settings record (volumes, pixel scale,
/// borders, brightness) and the private bytes (fly, screen follow). `false` when the block is not
/// valid or its record does not decode, leaving the live settings as they are. Only
/// [`scene_entered`] calls it (cold code too); the card is then taken as not synced, so the next
/// save after a change writes it.
#[link_section = ".psx_cold.settings"]
#[inline(never)]
pub fn settings() -> bool {
    if !valid() {
        return false;
    }
    let mut record = [0u8; RECORD_LEN];
    for (i, b) in record.iter_mut().enumerate() {
        *b = read_byte(OFF_SETTINGS + i);
    }
    let Some((s, _)) = Settings::decode(&record) else {
        return false;
    };
    save::apply(s);
    debug::set_fly(read_byte(OFF_PRIVATE + PRIV_FLY) != 0);
    backend::set_screen_follow(read_byte(OFF_PRIVATE + PRIV_FOLLOW) != 0);
    save::set_synced(false);
    true
}

/// Write the live settings into the block. [`switch_to`] calls it, so a change made anywhere (the
/// settings screen or a cart's pause menu) is in the block for whatever runs next.
#[inline(never)]
pub fn store_settings() {
    let Some(record) = save::current().encode() else {
        return;
    };
    for i in 0..SETTINGS_BYTES {
        write_byte(OFF_SETTINGS + i, record.get(i).copied().unwrap_or(0));
    }
    write_byte(OFF_PRIVATE + PRIV_FLY, debug::fly_enabled() as u8);
    write_byte(OFF_PRIVATE + PRIV_FOLLOW, backend::screen_follow() as u8);
}

/// The cover menu's last cart (0 Classic, 1 Lani).
pub fn last_cart() -> usize {
    read_byte(OFF_PRIVATE + PRIV_LAST_CART).min(1) as usize
}

/// Record the cover menu's highlighted cart, for the next time the menu opens.
pub fn set_last_cart(cart: usize) {
    write_byte(OFF_PRIVATE + PRIV_LAST_CART, cart.min(1) as u8);
}

/// The scene the block says is running.
pub fn current_scene() -> SceneId {
    read(OFF_SCENES) as SceneId
}

/// Switch to `scene`. Every scene of the collection is in this one resident image, so the switch
/// reads nothing from the disc: it records the switch in the block (current, previous, counter,
/// the settings) and returns, and the caller runs the scene. Under the core, a scene whose image is
/// not resident would be loaded here first.
pub fn switch_to(scene: SceneId) {
    store_settings();
    let current = read(OFF_SCENES) & 0xffff;
    write(OFF_SCENES, scene as u32 | current << 16);
    write(OFF_SWITCHES, read(OFF_SWITCHES).wrapping_add(1));
}

// The image's entry (games/scene-ram.ld sets psoxide.ld's `__initial_pc` to it): `$sp` to
// `STACK_INIT` before anything uses the stack, then psx-rt's `_start`. The BIOS starts a disc's
// program with `$sp` = 0x801FFFF0 (SYSTEM.CNF's STACK), inside the handoff block; the core's
// `__core_boot` does this for every program (CORE-ABI.md 4.3). `$a0..$a3` untouched.
#[cfg(target_arch = "mips")]
core::arch::global_asm!(
    r#"
    .section .text.__scene_boot,"ax",@progbits
    .globl __scene_boot
    .set push
    .set noreorder
__scene_boot:
    lui   $sp, {stack_hi}
    j     _start
    addiu $sp, $sp, {stack_lo}
    .set pop
"#,
    stack_hi = const STACK_HI,
    stack_lo = const STACK_LO,
);
