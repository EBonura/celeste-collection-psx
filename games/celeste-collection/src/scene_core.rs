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

/// A scene id: the row of the disc's scene table (SCENES.TBL v2, written by `mkisopsx --scenes`
/// from `games/celeste-collection/scenes.txt`).
pub type SceneId = u16;
/// The Program's start scene: the core starts the image through `__core_boot`.
pub const START: SceneId = 0;
/// The cover menu, the settings screen and the credits (in-program).
pub const MENU: SceneId = 1;
/// Celeste Classic (in-program).
pub const CLASSIC: SceneId = 2;
/// Celeste 2: Lani's Trek (in-program).
pub const LANI: SceneId = 3;
/// "No scene": the previous scene after a cold entry.
const NONE: SceneId = 0xffff;

/// The handoff block, 0x801FFF00..0x80200000 (CORE-ABI.md section 2).
const BLOCK: usize = 0x801f_ff00;
const MAGIC: u32 = 0x4243_5350; // "PSCB"
const VERSION: u32 = 1;
const OFF_MAGIC: usize = 0x00;
const OFF_VERSION_LENGTH: usize = 0x04;
const OFF_SCENES: usize = 0x14; // current (low half), previous (high half)
const OFF_SWITCHES: usize = 0x1c;
const OFF_SETTINGS: usize = 0x30;
const SETTINGS_BYTES: usize = 64;
const OFF_PRIVATE: usize = 0x80;
const OFF_NOT_MAGIC: usize = 0xfc;

/// The collection's use of the scene-private area: one version byte, then the cover menu's last
/// cart, the debug fly flag and the screen-follow option.
const PRIVATE_VERSION: u8 = 1;
const PRIV_VERSION: usize = 0;
const PRIV_LAST_CART: usize = 1;
const PRIV_FLY: usize = 2;
const PRIV_FOLLOW: usize = 3;
const PRIVATE_BYTES: usize = 124;

const _: () = assert!(RECORD_LEN <= SETTINGS_BYTES);

/// The handoff block as the link map names it (games/scene-block.ld places `.scene_block` at
/// 0x801FFF00, NOLOAD): the harness and the journeys read `SCENE_BLOCK` + offset. Only its address
/// is used; every access goes through `BLOCK`.
#[no_mangle]
#[link_section = ".scene_block"]
#[used]
pub static mut SCENE_BLOCK: [u32; 64] = [0; 64];

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

/// Call first in `main`. The core's `__core_boot` attached before `_start` and wrote the block's
/// own fields (magic, version, core hash, disc base, scenes, flags, loaded mask). On a BIOS boot
/// (previous scene 0xFFFF, or no valid block from the private layout's view) this reads the memory
/// card, as the collection always did at boot, and writes the settings record and the collection's
/// private bytes; otherwise it applies the block's settings. It also reads the scene table into
/// RAM once ([`table`]). `previous` is `None` on a BIOS boot, which is when the menu plays the
/// splash.
#[link_section = ".boot_only.scene_entered"]
#[inline(never)]
pub fn scene_entered() -> Entry {
    read_table();
    let previous = (read(OFF_SCENES) >> 16) as SceneId;
    if psx_core::link::block_valid() && previous != NONE && valid() {
        settings();
        return Entry {
            previous: Some(previous),
        };
    }
    save::load(); // no card or no file keeps the defaults
    for off in (OFF_PRIVATE..OFF_PRIVATE + PRIVATE_BYTES).step_by(4) {
        write(off, 0);
    }
    write_byte(OFF_PRIVATE + PRIV_VERSION, PRIVATE_VERSION);
    store_settings();
    Entry { previous: None }
}

/// The program's RAM copy of its scene table (the core's v2 rule: read once at boot, passed to
/// every `psx_core_switch`). The collection's switches are in-program calls, so nothing passes it
/// yet; it is here for a switch to another program.
static mut TABLE: [u32; TABLE_WORDS] = [0; TABLE_WORDS];
/// Header plus four rows (the start scene and three in-program scenes; no image rows).
const TABLE_WORDS: usize = 16 + 8 * 4;

fn read_table() {
    // SAFETY: single-threaded boot; nothing else holds the table.
    let table = unsafe { &mut *core::ptr::addr_of_mut!(TABLE) };
    if psx_core::link::read_table(table).is_err() {
        psx_core::link::fatal(psx_core::abi::code::TABLE_MAGIC);
    }
}

/// The scene table as read at boot.
pub fn table() -> &'static [u32] {
    // SAFETY: written once by `read_table` before any reader.
    unsafe { &*core::ptr::addr_of!(TABLE) }
}

/// Make the block's settings the live ones: the psx-settings record (volumes, pixel scale,
/// borders, brightness) and the private bytes (fly, screen follow). `false` when the block is not
/// valid or its record does not decode, leaving the live settings as they are. Only
/// [`scene_entered`] calls it (boot-only too); the card is then taken as not synced, so the next
/// save after a change writes it.
#[link_section = ".boot_only.settings"]
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

/// Switch to `scene`. Every scene of the collection is in-program (its SCENES.TBL rows point at this
/// Program), so the switch reads nothing from the disc and does not call the core: it records the
/// switch in the block (current, previous, counter, the settings) and returns, and the caller runs
/// the scene. (The SDK lane's psx-core helper for the in-program bookkeeping replaces the writes
/// here when it lands.)
pub fn switch_to(scene: SceneId) {
    store_settings();
    let current = read(OFF_SCENES) & 0xffff;
    write(OFF_SCENES, scene as u32 | current << 16);
    write(OFF_SWITCHES, read(OFF_SWITCHES).wrapping_add(1));
}

// The boot-only memory's mover (psoxide-boot.ld names `__boot_entry`; `__core_boot` goes on to it):
// `.boot` copied from where the file stores it (after .data and the core's image) to where it
// runs (after .bss), away from any overlap, `.boot_bss` zeroed, the I-cache invalidated over the
// run range, then psx-rt's `_start`. The core SDK's psx-rt carries this mover; the pinned psx-rt
// does not, so the collection has it until the pin moves. `$sp` is already STACK_INIT
// (`__core_boot`); `$a0..$a2` are kept.
#[cfg(target_arch = "mips")]
extern "C" {
    static __boot_start: u8;
    static __boot_load_end: u8;
    static __boot_end: u8;
    static __boot_lma: u8;
}

#[cfg(target_arch = "mips")]
#[no_mangle]
#[link_section = ".text.__boot_entry_rs"]
extern "C" fn __boot_entry_rs() {
    let run = &raw const __boot_start as *mut u32;
    let load_end = &raw const __boot_load_end as usize;
    let end = &raw const __boot_end as usize;
    let stored = &raw const __boot_lma as *const u32;
    let words = (load_end - run as usize) / 4;
    // SAFETY: psoxide-boot.ld lays these out word aligned: `words` words stored at `__boot_lma`
    // inside the loaded image and as many at `__boot_start`, memory nothing uses yet; no code runs
    // from either. The copy runs away from the overlap, a word at a time.
    unsafe {
        if (run as usize) < (stored as usize) {
            for i in 0..words {
                core::ptr::write_volatile(run.add(i), core::ptr::read_volatile(stored.add(i)));
            }
        } else {
            for i in (0..words).rev() {
                core::ptr::write_volatile(run.add(i), core::ptr::read_volatile(stored.add(i)));
            }
        }
        let mut at = load_end;
        while at < end {
            core::ptr::write_volatile(at as *mut u32, 0);
            at += 4;
        }
    }
    psx_rt::cache::flush_instruction_cache_range(run as u32, load_end as u32);
}

#[cfg(target_arch = "mips")]
core::arch::global_asm!(
    ".section .text.__boot_entry,\"ax\",@progbits",
    ".globl __boot_entry",
    ".set noreorder",
    "__boot_entry:",
    "move  $s0, $a0",
    "move  $s1, $a1",
    "move  $s2, $a2",
    "jal   __boot_entry_rs",
    "nop",
    "move  $a0, $s0",
    "move  $a1, $s1",
    "j     _start",
    "move  $a2, $s2",
    ".set reorder",
);
