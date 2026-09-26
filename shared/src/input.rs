//! PICO-8 button model: `btn()` (held) and `btnp()` (pressed, with auto-repeat).
//!
//! Buttons are PICO-8's six, by bit: 0 left, 1 right, 2 up, 3 down, 4 O (jump),
//! 5 X (dash/grapple). Call [`set_buttons`] once per frame with the current
//! 6-bit pad mask BEFORE game logic, then use [`btn`] / [`btnp`] exactly like
//! the Lua equivalents.
//!
//! `btnp` matches PICO-8: true on the frame a button is first pressed, then --
//! if held -- again after a 15-frame delay, repeating every 4 frames. Plus one
//! launcher-safe rule: a button that is already held when the cart starts (e.g.
//! the Cross still down from the menu that launched us) is suppressed until it
//! is released and pressed again. Prime that with [`prime`].
//!
//! The bookkeeping (edges, hold counting, auto-repeat, handoff suppression) is
//! the SDK's [`PadTracker`]; this module just keeps PICO-8's bit-indexed API
//! and repeat cadence on top of it.

use psx_pad::{button, poll_port1, ButtonState, PadMode, PadTracker};

static mut TRACKER: PadTracker = PadTracker::new();

/// The last cleanly read buttons (d-pad already merged with the stick), reused
/// when a poll comes back garbled.
static mut LAST_CLEAN: ButtonState = ButtonState::NONE;

/// Left-stick deflection (of 127) that counts as a d-pad press. Per axis, so
/// a diagonal push sets both bits like a diagonal on the d-pad; generous
/// enough that a worn stick still registers, wide enough that centre drift
/// doesn't.
const STICK_THRESHOLD: i16 = 48;

/// Poll port 1 and return its buttons with the left analog stick folded into
/// the d-pad bits. PICO-8 input is digital, so a DualShock in analog mode
/// simply gets its stick read as a second d-pad; in digital mode the sticks
/// are centred and nothing changes. Use this everywhere the games, launcher
/// and pause menu read the pad.
///
/// A poll whose ID handshake stays garbled through the SDK's retries
/// ([`PadMode::Unknown`]) carries button bytes that mean nothing: decoded as
/// active-low they can read as a burst of buttons nobody pressed, which the
/// launcher would take as fresh presses (Select opens the credits) and, if it
/// persists, as buttons held forever (no fresh press ever registers). Such a
/// poll repeats the last clean state instead, so it can neither invent a press
/// nor release a held button.
pub fn poll_buttons() -> ButtonState {
    let pad = poll_port1();
    let clean = match pad.mode {
        PadMode::Unknown => return unsafe { LAST_CLEAN },
        PadMode::Analog | PadMode::Config => with_stick(pad),
        PadMode::Digital | PadMode::Disconnected => pad.buttons,
    };
    unsafe { LAST_CLEAN = clean };
    clean
}

/// `pad`'s buttons with its left stick folded into the d-pad bits.
fn with_stick(pad: psx_pad::PadState) -> ButtonState {
    let (x, y) = pad.sticks.left_centered();
    let mut bits = pad.buttons.bits();
    if x <= -STICK_THRESHOLD {
        bits |= button::LEFT;
    } else if x >= STICK_THRESHOLD {
        bits |= button::RIGHT;
    }
    if y <= -STICK_THRESHOLD {
        bits |= button::UP;
    } else if y >= STICK_THRESHOLD {
        bits |= button::DOWN;
    }
    ButtonState::from_bits(bits)
}

// PICO-8 default auto-repeat (in the cart's frames).
const REPEAT_DELAY: u8 = 15;
const REPEAT_INTERVAL: u8 = 4;

/// Seed the held state from the current pad so buttons already down when the
/// cart starts don't read as a fresh `btnp`. Call once before the frame loop.
pub fn prime(mask: u8) {
    unsafe {
        TRACKER.update(mask as u16);
        TRACKER.prime();
    }
}

/// Latch this frame's 6-bit button mask. Call once per frame before game logic.
pub fn set_buttons(mask: u8) {
    unsafe { TRACKER.update(mask as u16) }
}

/// PICO-8 `btn(b)`: is button `b` held this frame.
#[inline]
pub fn btn(b: i32) -> bool {
    unsafe { TRACKER.is_held(1 << (b & 7)) }
}

/// PICO-8 `btnp(b)`: pressed this frame, or an auto-repeat tick. Suppressed for
/// buttons that were already held when the cart started (see [`prime`]).
#[inline]
pub fn btnp(b: i32) -> bool {
    unsafe { TRACKER.repeats(1 << (b & 7), REPEAT_DELAY, REPEAT_INTERVAL) }
}
