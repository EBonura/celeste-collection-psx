//! PSoXide rendering backend for the PICO-8 platform API.
//!
//! Re-implements PICO-8's draw callbacks (spr/map/rectfill/line/circfill/
//! print/pal/camera/mget/fget) on PSoXide's GPU, using immediate-mode draws.
//! PICO-8 is a painter's-order renderer and so is immediate-mode GP0, so call
//! order == layer order, no ordering table.
//!
//! Display model: the PICO-8 128x128 image is drawn at 2x (256x256) because
//! the spritesheet is pre-doubled. The 256-wide field is centred in a 320x240
//! NTSC framebuffer; the 16px vertical overflow is absorbed by `OFS_Y`.
//!
//! The spritesheet and tilemap are game-specific, so they come from a [`Cart`]
//! the game registers via [`set_cart`] / [`upload_assets`]; the font and CLUT
//! are universal PICO-8 data and live in this crate.

use crate::font::FONT_DATA;
use crate::palette::{PICO8_CLUT, PICO8_RGB, TEXT_CLUTS};
use psx_gpu::{self as gpu};
use psx_hw::gpu::{pack_color, pack_texcoord, pack_vertex, pack_xy};
use psx_io::gpu::{wait_cmd_ready, write_gp0};
use psx_vram::{upload_16bpp, Clut, TexDepth, Tpage, VramRect};

/// A game's PICO-8 graphics data: the doubled 256x256 4bpp spritesheet and the
/// tilemap (cells + per-sprite flags). Registered once via [`set_cart`]; the
/// backend's `spr`/`map`/`mget`/`fget` read from the active cart.
#[derive(Clone, Copy)]
pub struct Cart {
    /// 256x256 @ 4bpp spritesheet, 64 halfwords/row (== [u16; 16384]).
    pub gfx: &'static [u16],
    /// Map cells, `map_w` wide.
    pub tilemap: &'static [u8],
    /// Per-sprite flag byte, indexed by sprite id.
    pub tile_flags: &'static [u8],
    /// Map width in cells.
    pub map_w: usize,
}

const EMPTY_CART: Cart = Cart {
    gfx: &[],
    tilemap: &[],
    tile_flags: &[],
    map_w: 128,
};

// ---- VRAM layout (off-screen, right of the framebuffers) ----
const GFX_TPAGE: Tpage = Tpage::new(640, 0, TexDepth::Bit4); // 256x256 4bpp -> 64 halfwords wide
const FONT_TPAGE: Tpage = Tpage::new(704, 0, TexDepth::Bit4); // 256x170 4bpp
                                                              // Two ping-pong slots for the sprite palette. The PS1 GPU caches the CLUT and
                                                              // reloads it only when the clut *word* changes -- NOT when its VRAM is
                                                              // overwritten -- so a pal() recolour must land in the OTHER slot to force a
                                                              // reload, else the sprite keeps the stale palette on hardware (Madeline's hair
                                                              // stuck red). Both 16 entries, side by side on the free row below the framebuffers.
const SPRITE_CLUT_A: Clut = Clut::new(0, 480);
const SPRITE_CLUT_B: Clut = Clut::new(16, 480);
/// Second sprite palette for `map()`: tiles carrying MAP_ALT_FLAG draw through
/// it (a level-wide `pal()` swap without a second pass or a GPU flush).
const MAP_ALT_CLUT: Clut = Clut::new(32, 480);
const TEXT_CLUT: Clut = Clut::new(0, 481); // one row, re-uploaded per print colour
const FILLP_TPAGE: Tpage = Tpage::new(768, 0, TexDepth::Bit4); // 8x8 dither patterns side-by-side
                                                               // Two fillp CLUT slots (entry 0 = transparent, 1 = the fill colour), ping-ponged
                                                               // on every colour change: the GPU caches a CLUT by its clut word, so rewriting
                                                               // one slot in place mid-frame (fog in white, then columns in dark blue) left the
                                                               // columns drawn in the fog's white.
const FILL_CLUT_A: Clut = Clut::new(0, 482);
const FILL_CLUT_B: Clut = Clut::new(16, 482);

// ---- Screen transform ----
// Pixel scale: 1 = native 128x128 (whole screen visible, centred in 320x240),
// 2 = doubled 256x256 (fills more, overflows the frame by 16px -> 8px clipped top
// and bottom, mitigated by the follow-pan). Runtime so the menu can switch it.
static mut SCALE: i16 = 2; // default: 2x
#[inline]
fn play_w() -> i16 {
    128 * unsafe { SCALE }
}
#[inline]
fn ofs_x() -> i16 {
    (320 - play_w()) / 2 // 1x: 96, 2x: 32
}
/// Vertical offset that centres the field: 1x -> +56 (fits), 2x -> -8 (clips 8/8).
#[inline]
fn ofs_y_center() -> i16 {
    (240 - 128 * unsafe { SCALE }) / 2
}

// ---- Mutable PICO-8 draw state ----
static mut CART: Cart = EMPTY_CART;
static mut CAM_X: i16 = 0;
static mut CAM_Y: i16 = 0;
// Live vertical offset (eased toward V_TARGET each frame) and the follow toggle.
// Follow only applies at 2x (1x never clips); the pause menu can switch it off.
static mut V_OFS: i16 = -8; // 2x centre; kept in sync by set_pixel_scale
static mut V_TARGET: i16 = -8;
static mut V_FOLLOW: bool = true;
/// PICO-8 `pal()` colour remap (draw index -> palette index).
static mut PAL: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// Register the active cart (spritesheet + map). Call before drawing.
pub fn set_cart(cart: Cart) {
    unsafe { CART = cart };
}

/// Register `cart` and upload its spritesheet plus the universal font and
/// sprite CLUT to VRAM. Call once after `gpu::init`.
pub fn upload_assets(cart: Cart) {
    set_cart(cart);
    unsafe { MAP_ALT_FLAG = -1 };
    upload_16bpp(
        VramRect::new(GFX_TPAGE.x(), GFX_TPAGE.y(), 64, 256),
        cart.gfx,
    );
    upload_16bpp(
        VramRect::new(FONT_TPAGE.x(), FONT_TPAGE.y(), 64, 170),
        &FONT_DATA,
    );
    upload_16bpp(
        VramRect::new(sprite_clut().x(), sprite_clut().y(), 16, 1),
        &PICO8_CLUT,
    );
    upload_fillp();
}

/// Upload just the universal PICO-8 font (and reset the colour remap), so a
/// screen with no cart -- e.g. the launcher's intro/credits -- can use [`print`]
/// for PICO-8-font text. Coordinates are PICO-8 128-space (scaled 2x, centred);
/// set `camera(0, 0)` first. Call once after `gpu::init`.
pub fn upload_font() {
    upload_16bpp(
        VramRect::new(FONT_TPAGE.x(), FONT_TPAGE.y(), 64, 170),
        &FONT_DATA,
    );
    pal_reset();
}

/// Re-apply the gfx tpage as the current draw mode. GP0 0x64 sprites carry no
/// tpage word, so this must precede sprite/map draws each frame.
#[inline]
pub fn begin_sprite_pass() {
    emit([draw_mode_word(GFX_TPAGE)]);
}

// --------------------------------------------------------------------
// Display list
// --------------------------------------------------------------------
// In deferred mode (the games' frame loops) every primitive, CLUT upload and
// draw-mode change is appended to a GPU linked list instead of written to GP0,
// and `submit` hands the whole frame to DMA channel 2 in one go. The CPU no
// longer blocks on the 16-word GP0 FIFO while the GPU rasterises (that stall
// was a fifth of every frame in Celeste 2's tiled levels): it renders the
// next frame's audio in the VBlank wait while the GPU draws. Immediate mode
// (the launcher, the pause overlay, boot-time uploads) writes GP0 directly as
// before, so text drawn through the SDK's font keeps its order.

/// Words of list storage: a frame is ~450 primitives (~2k words) plus CLUT
/// uploads; a full list is submitted and waited on, then refilled.
const LIST_WORDS: usize = 8192;
#[repr(C, align(16))]
struct List([u32; LIST_WORDS]);
static mut LIST: List = List([0; LIST_WORDS]);
static mut STREAM: Option<gpu::ordered::OrderedCommandStream> = None;
static mut DEFERRED: bool = false;

#[inline]
unsafe fn stream() -> &'static mut gpu::ordered::OrderedCommandStream {
    let slot = &mut *core::ptr::addr_of_mut!(STREAM);
    slot.get_or_insert_with(|| {
        gpu::ordered::OrderedCommandStream::new(&mut *core::ptr::addr_of_mut!(LIST.0))
    })
}

/// Select SDK ordered DMA output or immediate GP0 output. Switching to
/// immediate mode drains the list before SDK font or upload calls can run.
pub fn set_deferred(on: bool) {
    unsafe {
        if DEFERRED && !on {
            draw_sync();
        }
        DEFERRED = on;
    }
}

#[inline(always)]
fn emit<const N: usize>(words: [u32; N]) {
    unsafe {
        if DEFERRED {
            stream().push_packet(words);
        } else {
            wait_cmd_ready();
            for word in words {
                write_gp0(word);
            }
        }
    }
}

#[inline(never)]
fn emit_upload(rect: VramRect, pixels: &[u16]) {
    unsafe {
        if DEFERRED {
            stream().push_upload(rect.x, rect.y, rect.w, rect.h, pixels);
        } else {
            upload_16bpp(rect, pixels);
        }
    }
}

/// Submit the SDK ordered stream while retaining its DMA storage.
pub fn submit() {
    unsafe {
        if DEFERRED {
            stream().submit();
        }
    }
}

/// Drain both DMA and drawing before immediate commands or framebuffer swaps.
pub fn draw_sync() {
    unsafe {
        if DEFERRED {
            stream().draw_sync();
        } else {
            gpu::draw_sync();
        }
    }
}

/// The GP0(E1h) draw-mode word for a texture page (what
/// `Tpage::apply_as_draw_mode` writes).
#[inline]
fn draw_mode_word(tpage: Tpage) -> u32 {
    0xE100_0000
        | (tpage.x() as u32 / 64)
        | (if tpage.y() == 256 { 1 } else { 0 }) << 4
        | (tpage.depth() as u32) << 7
        | 1 << 10
}

#[inline]
fn sx(px: i16) -> i16 {
    (px - unsafe { CAM_X }) * unsafe { SCALE } + ofs_x()
}
#[inline]
fn sy(py: i16) -> i16 {
    (py - unsafe { CAM_Y }) * unsafe { SCALE } + unsafe { V_OFS }
}

/// Pixel scale: 1 = native 128x128, 2 = doubled 256x256. Changing it re-centres
/// the vertical offset. A player setting (the pause-menu "Pixel" row).
pub fn set_pixel_scale(s: i16) {
    unsafe {
        SCALE = if s <= 1 { 1 } else { 2 };
        V_OFS = ofs_y_center();
        V_TARGET = V_OFS;
    }
}
pub fn pixel_scale() -> i16 {
    unsafe { SCALE }
}

/// Snap the vertical offset back to centre immediately. The pause overlay calls
/// this so its `backend`-drawn shapes line up with its own (centred) font, rather
/// than inheriting the live follow-pan offset of the frozen game behind it.
pub fn center_screen() {
    unsafe {
        V_OFS = ofs_y_center();
        V_TARGET = V_OFS;
    }
}

/// Enable/query "follow" vertical panning (a player setting, off = classic centre).
pub fn set_screen_follow(on: bool) {
    unsafe { V_FOLLOW = on };
}
pub fn screen_follow() -> bool {
    unsafe { V_FOLLOW }
}

/// Per-frame, before drawing. `rel_py` is the player's PICO-8 row relative to the
/// camera (player_y - camera_y, ~0..128); pass 64 (centre) when there's no player
/// (title/menus). In follow mode the offset eases toward showing the top when the
/// player is high or the bottom when low, with a centred deadzone + hysteresis so
/// it stays put while the player is mid-screen. Otherwise it eases back to centre.
pub fn track_vofs(rel_py: i16) {
    unsafe {
        if SCALE == 1 {
            V_OFS = ofs_y_center(); // 1x never clips -> no pan
            V_TARGET = V_OFS;
            return;
        }
        let centre = ofs_y_center(); // -8 at 2x
        if !V_FOLLOW {
            V_TARGET = centre;
        } else if rel_py < 36 {
            V_TARGET = 0; // player high in view -> reveal the top
        } else if rel_py > 92 {
            V_TARGET = -16; // player low in view -> reveal the bottom
        } else if rel_py > 52 && rel_py < 76 {
            V_TARGET = centre; // back to centre only once clearly mid-screen
        }
        // ease 1px/frame toward the target (gaps 36..52 / 76..92 hold = hysteresis)
        if V_OFS < V_TARGET {
            V_OFS += 1;
        } else if V_OFS > V_TARGET {
            V_OFS -= 1;
        }
    }
}

/// Resolve a PICO-8 colour index through the `pal()` remap to RGB888.
#[inline]
fn rgb(c: i32) -> (u8, u8, u8) {
    let idx = unsafe { PAL[(c as usize) & 15] } as usize;
    let e = PICO8_RGB[idx];
    (e[0], e[1], e[2])
}

// --------------------------------------------------------------------
// Sprite / map
// --------------------------------------------------------------------

/// An `sz`x`sz` primitive at screen `(x,y)` is fully outside the 320x240 frame.
/// The GP0 vertex word is 11-bit signed, so a primitive at screen x >= 1024 wraps
/// to the left edge (a wide level's far-right objects reappear floating on the
/// left). PICO-8's spr()/map() clip to the screen; we cull so the wrap can't happen.
#[inline]
fn offscreen_cell(x: i16, y: i16, sz: i16) -> bool {
    x <= -sz || x >= 320 || y <= -sz || y >= 240
}

/// Draw one doubled (16x16-in-VRAM) cell at screen `(x,y)`. At 2x non-flipped it
/// uses the fast GP0 0x64 rect (the cell's draw-mode tpage must already be set);
/// otherwise (1x, or flipped) a textured quad sized `8*SCALE` with a 16-texel UV
/// span. At 1x the span downsamples the doubled cell back to native 8x8 -- exact,
/// because the sheet is a nearest-neighbour 2x double (each texel pair is equal).
#[inline]
fn draw_cell(
    x: i16,
    y: i16,
    u0: u8,
    v0: u8,
    clut_word: u16,
    tpage: Tpage,
    flip_x: bool,
    flip_y: bool,
) {
    let sc = unsafe { SCALE };
    let sz = 8 * sc;
    if offscreen_cell(x, y, sz) {
        return;
    }
    if sc == 2 && !flip_x && !flip_y {
        emit([
            0x6400_0000 | pack_color(0x80, 0x80, 0x80),
            pack_vertex(x, y),
            pack_texcoord(u0, v0, clut_word),
            pack_xy(16, 16),
        ]);
        return;
    }
    // Clamp the far UV edge to 255 so a last-column cell (u0=240) doesn't wrap.
    let u_hi = (u0 as u16 + 16).min(255) as u8;
    let v_hi = (v0 as u16 + 16).min(255) as u8;
    let (ul, ur) = if flip_x { (u_hi, u0) } else { (u0, u_hi) };
    let (vt, vb) = if flip_y { (v_hi, v0) } else { (v0, v_hi) };
    let tp = tpage.uv_tpage_word(0);
    emit([
        0x2C00_0000 | pack_color(0x80, 0x80, 0x80),
        pack_vertex(x, y),
        pack_texcoord(ul, vt, clut_word),
        pack_vertex(x + sz, y),
        pack_texcoord(ur, vt, tp),
        pack_vertex(x, y + sz),
        pack_texcoord(ul, vb, 0),
        pack_vertex(x + sz, y + sz),
        pack_texcoord(ur, vb, 0),
    ]);
}

/// PICO-8 `spr()`. Draws 8x8 PICO-8 sprite `n` at PICO-8 `(x,y)`.
pub fn spr(n: i32, x: i16, y: i16, flip_x: bool, flip_y: bool) {
    if n < 0 {
        return;
    }
    let u0 = ((n % 16) * 16) as u8;
    let v0 = ((n / 16) * 16) as u8;
    begin_sprite_pass();
    draw_cell(
        sx(x),
        sy(y),
        u0,
        v0,
        sprite_clut().uv_clut_word(),
        GFX_TPAGE,
        flip_x,
        flip_y,
    );
}

/// `mget(x,y)` -- raw map fetch (NOT camera/room relative).
#[inline]
pub fn mget(x: i32, y: i32) -> i32 {
    let cart = unsafe { CART };
    if x < 0 || y < 0 || x >= cart.map_w as i32 {
        return 0;
    }
    let i = x as usize + y as usize * cart.map_w;
    if i >= cart.tilemap.len() {
        return 0;
    }
    cart.tilemap[i] as i32
}

/// `fget(t,f)` -- tile flag bit `f` of tile `t`.
#[inline]
pub fn fget(t: i32, f: i32) -> bool {
    let cart = unsafe { CART };
    if t < 0 || t as usize >= cart.tile_flags.len() {
        return false;
    }
    (cart.tile_flags[t as usize] >> f) & 1 != 0
}

/// PICO-8 `map()`. Draws map cells `[mx,mx+mw) x [my,my+mh)` at screen
/// `(tx,ty)`, filtered by `mask`: 0 = all; 4 = flags==4 exactly; else flag bit
/// `mask-1`. PICO-8 `map()` never draws sprite 0 (treated as empty).
pub fn map(mx: i32, my: i32, tx: i16, ty: i16, mw: i32, mh: i32, mask: i32) {
    begin_sprite_pass();
    let clut_word = sprite_clut().uv_clut_word();
    let alt_word = MAP_ALT_CLUT.uv_clut_word();
    let alt_flag = unsafe { MAP_ALT_FLAG };
    let cart = unsafe { CART };
    for j in 0..mh {
        for i in 0..mw {
            let t = mget(mx + i, my + j);
            if t == 0 {
                continue;
            }
            let flags = cart.tile_flags.get(t as usize).copied().unwrap_or(0) as i32;
            // PICO-8 `map(..., layer)`: draw the tiles whose flags include every
            // bit of `mask` (a bitfield of flag values, not a flag index).
            if mask != 0 && flags & mask != mask {
                continue;
            }
            let clut_word = if alt_flag >= 0 && (flags >> alt_flag) & 1 != 0 {
                alt_word
            } else {
                clut_word
            };
            let u0 = ((t % 16) * 16) as u8;
            let v0 = ((t / 16) * 16) as u8;
            let px = sx(tx + (i as i16) * 8);
            let py = sy(ty + (j as i16) * 8);
            draw_cell(px, py, u0, v0, clut_word, GFX_TPAGE, false, false);
        }
    }
}

// --------------------------------------------------------------------
// Flat shapes
// --------------------------------------------------------------------

/// PICO-8 `rectfill(x,y,x2,y2,c)` -- inclusive, camera-relative.
pub fn rectfill(x: i16, y: i16, x2: i16, y2: i16, c: i32) {
    let (lx, rx) = if x <= x2 { (x, x2) } else { (x2, x) };
    let (ty, by) = if y <= y2 { (y, y2) } else { (y2, y) };
    let x0 = sx(lx);
    let y0 = sy(ty);
    let x1 = sx(rx + 1); // inclusive -> +1 px (then *2 in transform)
    let y1 = sy(by + 1);
    let (r, g, b) = rgb(c);
    emit([
        0x6000_0000 | pack_color(r, g, b),
        pack_vertex(x0, y0),
        pack_xy((x1 - x0) as u16, (y1 - y0) as u16),
    ]);
}

// ---- fillp (PICO-8 fill patterns / dither) -------------------------------
// PICO-8's `fillp` overlays a screen-fixed 4x4 dither over fills. The PS1 has no
// such mode, so we tile a tiny 8x8 pattern (2x2 copies of the 4x4) via the GPU's
// texture window: the pattern's set texels draw the fill colour, unset texels map
// to a transparent CLUT entry so the background shows through. UVs follow screen
// position, so the pattern stays screen-locked as the camera scrolls, like the cart.

/// fillp pattern ids (index into FILLP_VALUES; the cart's 16-bit 4x4 patterns).
pub const FILLP_COLUMNS: usize = 0; // sparse dots (background pillars)
pub const FILLP_FOG: usize = 1; // 50% checkerboard (fog)
pub const FILLP_CRUMBLE: usize = 2; // checkerboard, opposite phase (crumble crack)
const FILLP_VALUES: [u16; 3] = [
    0b0000_1000_0000_0010,
    0b0101_1010_0101_1010,
    0b1010_0101_1010_0101,
];

/// Build an 8x8 4bpp tile from a cart 4x4 fillp pattern: texel 1 (drawn) where the
/// pattern bit is CLEAR, 0 (transparent) where it is set -- PICO-8's `.1` patterns
/// (all three here) paint the colour on the 0 bits and leave the 1 bits
/// transparent, e.g. the columns' sparse pattern is solid colour with a few holes.
/// 16 halfwords (2 wide x 8 tall).
fn build_pattern(p: u16, scale: usize) -> [u16; 16] {
    let mut out = [0u16; 16];
    let mut y = 0usize;
    while y < 8 {
        let mut hw = [0u16; 2];
        let mut x = 0usize;
        while x < 8 {
            let bit = 15 - (((y / scale) % 4) * 4 + ((x / scale) % 4));
            if (p >> bit) & 1 == 0 {
                hw[x / 4] |= 1u16 << ((x % 4) * 4);
            }
            x += 1;
        }
        out[y * 2] = hw[0];
        out[y * 2 + 1] = hw[1];
        y += 1;
    }
    out
}

/// Upload the dither patterns side-by-side in FILLP_TPAGE as 8x8 tiles: pattern i
/// doubled (one dither pixel = 2x2 texels, for the 2x pixel scale) at U = i*8,
/// and tiled 1:1 (for the 1x scale) at U = 24 + i*8. Dithered fills are textured
/// RECTANGLES that sample one texel per screen pixel, so the texel size has to
/// match the pixel scale. Called from [`upload_assets`]; re-run per game boot
/// since VRAM is shared.
fn upload_fillp() {
    unsafe { FILL_CLUT_COL = 0xFFFF };
    let mut i = 0;
    while i < FILLP_VALUES.len() {
        let doubled = build_pattern(FILLP_VALUES[i], 2);
        upload_16bpp(
            VramRect::new(FILLP_TPAGE.x() + (i as u16) * 2, FILLP_TPAGE.y(), 2, 8),
            &doubled,
        );
        let native = build_pattern(FILLP_VALUES[i], 1);
        upload_16bpp(
            VramRect::new(FILLP_TPAGE.x() + 6 + (i as u16) * 2, FILLP_TPAGE.y(), 2, 8),
            &native,
        );
        i += 1;
    }
}

/// Set the GPU texture window (GP0 0xE2). Masks/offsets are in 8-pixel units.
#[inline]
unsafe fn set_tex_window(mask_x: u32, mask_y: u32, off_x: u32, off_y: u32) {
    emit([0xE200_0000
        | (mask_x & 0x1F)
        | ((mask_y & 0x1F) << 5)
        | ((off_x & 0x1F) << 10)
        | ((off_y & 0x1F) << 15)]);
}

/// Colour currently in the active fillp CLUT slot (so a run of same-colour
/// dithered fills, e.g. a level's columns or fog clouds, uploads it once, not per
/// primitive) and which slot holds it.
static mut FILL_CLUT_COL: u16 = 0xFFFF;
static mut FILL_CLUT_SLOT: bool = false;

#[inline]
fn fill_clut() -> Clut {
    if unsafe { FILL_CLUT_SLOT } {
        FILL_CLUT_B
    } else {
        FILL_CLUT_A
    }
}

/// Set up dithered drawing in colour `c` with pattern `pattern`: upload the fill
/// CLUT (1 = colour, 0 = transparent) if it changed, select the pattern tpage as
/// the draw mode (rectangles sample the draw-mode tpage), and set an 8x8 texture
/// window on the pattern tile for the current pixel scale. Draw with
/// [`fillp_prim`], then [`fillp_end`] to restore the window for sprites/map.
///
/// Dithered fills are textured RECTANGLES (GP0 0x64), not polygons: on the GPU
/// a textured polygon costs ~2.7x per pixel what a textured rectangle does, and
/// the fog clouds + background columns of the column levels were enough to drop
/// them from 60 to 30 fps as polygons.
unsafe fn fillp_begin(c: i32, pattern: usize) {
    let col = PICO8_CLUT[(PAL[(c as usize) & 15] as usize) & 15];
    if col != FILL_CLUT_COL {
        FILL_CLUT_COL = col;
        FILL_CLUT_SLOT = !FILL_CLUT_SLOT; // new clut word -> the GPU reloads it
        let clut = fill_clut();
        emit_upload(VramRect::new(clut.x(), clut.y(), 2, 1), &[0u16, col]);
    }
    emit([draw_mode_word(FILLP_TPAGE)]);
    let tile = pattern as u32 + if SCALE == 1 { 3 } else { 0 };
    set_tex_window(31, 31, tile, 0); // 8x8 window at U = tile*8
}

/// One dithered rectangle in SCREEN pixels; `(u, v)` = the screen-locked pattern
/// phase (128-space position * scale; the window keeps the low 3 bits).
#[inline]
unsafe fn fillp_prim(x: i16, y: i16, w: u16, h: u16, u: u8, v: u8) {
    if w == 0 || h == 0 {
        return;
    }
    emit([
        0x6400_0000 | pack_color(0x80, 0x80, 0x80),
        pack_vertex(x, y),
        pack_texcoord(u, v, fill_clut().uv_clut_word()),
        pack_xy(w, h),
    ]);
}

#[inline]
unsafe fn fillp_end() {
    set_tex_window(0, 0, 0, 0);
}

/// PICO-8 `fillp(pattern) rectfill(x0,y0,x1,y1,c)`: a dithered rectangle. The
/// pattern is screen-fixed, so the rect is clamped to the visible camera window
/// and the pattern phase follows screen position. `pattern` is one of FILLP_*.
pub fn fillp_rect(x0: i16, y0: i16, x1: i16, y1: i16, c: i32, pattern: usize) {
    let (lx0, lx1) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let (ly0, ly1) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    unsafe {
        // clamp to the 128px camera window (fillp is screen-space)
        let lx = lx0.max(CAM_X);
        let rx = (lx1 + 1).min(CAM_X + 128);
        let ty = ly0.max(CAM_Y);
        let by = (ly1 + 1).min(CAM_Y + 128);
        if rx <= lx || by <= ty {
            return;
        }
        fillp_begin(c, pattern);
        let (u, v) = (((lx - CAM_X) * SCALE) as u8, ((ty - CAM_Y) * SCALE) as u8);
        fillp_prim(
            sx(lx),
            sy(ty),
            (sx(rx) - sx(lx)) as u16,
            (sy(by) - sy(ty)) as u16,
            u,
            v,
        );
        fillp_end();
    }
}

/// PICO-8 `fillp(pattern) circfill(cx,cy,r,c)`: a dithered filled circle, drawn as
/// dithered 1px scanlines (same midpoint span walk as [`circfill`]). Used for the
/// fog clouds. Screen-fixed dither, clamped to the camera window per scanline.
pub fn fillp_circfill(cx: i16, cy: i16, radius: i16, c: i32, pattern: usize) {
    fillp_circfill_clip(cx, cy, radius, c, pattern, NO_CLIP);
}

/// A `clip()` rectangle in 128-space: `(x0, y0, x1, y1)`, exclusive far edges.
pub type ClipRect = (i16, i16, i16, i16);
/// No clipping (the whole 128-space).
pub const NO_CLIP: ClipRect = (i16::MIN, i16::MIN, i16::MAX, i16::MAX);

/// [`fillp_circfill`] under a PICO-8 `clip()` rectangle.
pub fn fillp_circfill_clip(cx: i16, cy: i16, radius: i16, c: i32, pattern: usize, clip: ClipRect) {
    unsafe {
        fillp_begin(c, pattern);
        let (cam_x, cam_y) = (CAM_X, CAM_Y);
        let clip = (
            clip.0.max(cam_x),
            clip.1.max(cam_y),
            clip.2.min(cam_x + 128),
            clip.3.min(cam_y + 128),
        );
        let tex = pack_texcoord(0, 0, fill_clut().uv_clut_word()) & 0xFFFF_0000;
        disc_fill(
            cx,
            cy,
            radius,
            clip,
            0x6400_0000 | pack_color(0x80, 0x80, 0x80),
            Some(tex),
        );
        fillp_end();
    }
}

/// PICO-8 `line(x,y,x2,y2,c)`.
pub fn line(x: i16, y: i16, x2: i16, y2: i16, c: i32) {
    let (r, g, b) = rgb(c);
    emit([
        0x4000_0000 | pack_color(r, g, b),
        pack_vertex(sx(x), sy(y)),
        pack_vertex(sx(x2), sy(y2)),
    ]);
}

/// Flat-filled quad with arbitrary corners, in PICO-8 128-space. Corner order is a
/// strip (v0,v1,v2,v3), same as `rectfill`. Use for diagonal shapes that GP0 lines
/// render unreliably on hardware-accelerated PS1 emulators (e.g. the grapple-pickup
/// rays drawn as thin quads).
pub fn quad(p: [(i16, i16); 4], c: i32) {
    let (r, g, b) = rgb(c);
    emit([
        0x2800_0000 | pack_color(r, g, b),
        pack_vertex(sx(p[0].0), sy(p[0].1)),
        pack_vertex(sx(p[1].0), sy(p[1].1)),
        pack_vertex(sx(p[2].0), sy(p[2].1)),
        pack_vertex(sx(p[3].0), sy(p[3].1)),
    ]);
}

// ---- Side-margin gradient (the 32px bars beside the 256-wide field) ----
// (top RGB, bottom RGB) per preset. Preset 0 = solid black (the classic look).
// (top RGB, bottom RGB) per preset, in 8-bit channels. The originals were
// ~15-20% brightness, which a PC monitor shows but a real CRT/TV crushes to
// black (the gradient was invisible on console). Brightened ~3x so the top
// reads clearly on a TV while the bottom still fades toward the dark playfield.
const SIDE_PRESETS: [((u8, u8, u8), (u8, u8, u8)); 5] = [
    ((0, 0, 0), (0, 0, 0)),                   // Off (black)
    ((0x66, 0x3c, 0x96), (0x12, 0x0c, 0x30)), // Dusk (purple)
    ((0x30, 0x6c, 0x96), (0x0c, 0x18, 0x36)), // Ocean (teal)
    ((0x84, 0x36, 0x36), (0x24, 0x0c, 0x0c)), // Ember (red)
    ((0x3c, 0x66, 0x42), (0x0c, 0x18, 0x12)), // Forest (green)
];
static mut SIDE_PRESET: u8 = 1; // default: the Dusk gradient

pub fn set_side_preset(p: u8) {
    unsafe { SIDE_PRESET = p % SIDE_PRESETS.len() as u8 };
}
pub fn side_preset() -> u8 {
    unsafe { SIDE_PRESET }
}
pub fn side_preset_count() -> u8 {
    SIDE_PRESETS.len() as u8
}
pub fn side_preset_name(p: u8) -> &'static str {
    match p {
        0 => "Off",
        1 => "Dusk",
        2 => "Ocean",
        3 => "Ember",
        _ => "Forest",
    }
}

/// Fill the screen margins with the selected gradient. The left/right side bars
/// run HORIZONTALLY -- the preset's bright colour at the screen edge fading in to
/// the dark colour toward the playfield (mirrored left vs right, so both glow
/// from the outside in). The 1x top/bottom bars run vertically, same edge->in
/// fade. Screen-space (raw GP0), independent of the camera/vertical-pan; call
/// last in the game's draw to cover the field overdraw.
pub fn side_bars() {
    let (edge, inner) = SIDE_PRESETS[unsafe { SIDE_PRESET } as usize];
    // Playfield rect on screen. At 2x it spans the full height (top/bottom clipped,
    // no margin there); at 1x it's centred 128x128, so all four sides have margin.
    let px0 = ofs_x();
    let px1 = px0 + play_w();
    let py0 = ofs_y_center().max(0);
    let py1 = (ofs_y_center() + 128 * unsafe { SCALE }).min(240);
    side_strip_h(0, px0, 0, 240, edge, inner); // left: bright at x=0 -> dark inward
    side_strip_h(px1, 320, 0, 240, inner, edge); // right: dark inward -> bright at x=320
    if py0 > 0 {
        side_strip_v(px0, px1, 0, py0, edge, inner); // top (1x): bright at y=0
    }
    if py1 < 240 {
        side_strip_v(px0, px1, py1, 240, inner, edge); // bottom (1x): bright at y=240
    }
}
/// Gouraud triangle (GP0 0x30).
fn gouraud_tri(v: [(i16, i16); 3], c: [(u8, u8, u8); 3]) {
    emit([
        0x3000_0000 | pack_color(c[0].0, c[0].1, c[0].2),
        pack_vertex(v[0].0, v[0].1),
        pack_color(c[1].0, c[1].1, c[1].2),
        pack_vertex(v[1].0, v[1].1),
        pack_color(c[2].0, c[2].1, c[2].2),
        pack_vertex(v[2].0, v[2].1),
    ]);
}

/// Strip with a HORIZONTAL gradient: `c0` at the left edge `x0`, `c1` at `x1`.
fn side_strip_h(x0: i16, x1: i16, y0: i16, y1: i16, c0: (u8, u8, u8), c1: (u8, u8, u8)) {
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    gouraud_tri([(x0, y0), (x1, y0), (x0, y1)], [c0, c1, c0]);
    gouraud_tri([(x1, y0), (x0, y1), (x1, y1)], [c1, c0, c1]);
}
/// Strip with a VERTICAL gradient: `c0` at the top edge `y0`, `c1` at `y1`.
fn side_strip_v(x0: i16, x1: i16, y0: i16, y1: i16, c0: (u8, u8, u8), c1: (u8, u8, u8)) {
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    gouraud_tri([(x0, y0), (x1, y0), (x0, y1)], [c0, c0, c1]);
    gouraud_tri([(x1, y0), (x0, y1), (x1, y1)], [c0, c1, c1]);
}

/// PICO-8 `circfill(x,y,r,c)`: the rows of the disc, drawn symmetrically about
/// the centre, with runs of rows that share a half-width merged into one flat
/// rectangle (GP0 0x60, three words). The row half-width only ever shrinks as
/// `dy` grows, so the span work is O(r). Clouds issue dozens of these per frame
/// and the one-quad-per-row version was the biggest GPU FIFO stall in the game.
pub fn circfill(cx: i16, cy: i16, radius: i16, c: i32) {
    circfill_clip(cx, cy, radius, c, NO_CLIP);
}

/// [`circfill`] under a PICO-8 `clip()` rectangle (e.g. the flat-bottomed clouds).
pub fn circfill_clip(cx: i16, cy: i16, radius: i16, c: i32, clip: ClipRect) {
    let (r, g, b) = rgb(c);
    disc_fill(
        cx,
        cy,
        radius,
        clip,
        0x6000_0000 | pack_color(r, g, b),
        None,
    );
}

/// Draw a clipped disc as flat rectangles (`tex` = None) or as dithered
/// textured rectangles (`tex` = the texcoord word's CLUT half; the pattern
/// phase is derived from the screen position).
#[inline(always)]
fn disc_fill(cx: i16, cy: i16, radius: i16, clip: ClipRect, cmd: u32, tex: Option<u32>) {
    if radius < 0 {
        return;
    }
    unsafe {
        if !DEFERRED {
            disc_fill_immediate(cx, cy, radius, clip, cmd, tex);
        } else if let Some(t) = tex {
            disc_fill_list::<true>(cx, cy, radius, clip, cmd, t);
        } else {
            disc_fill_list::<false>(cx, cy, radius, clip, cmd, 0);
        }
    }
}

/// The listed disc: the hot path (the clouds are ~84 of these a frame). The
/// merged runs come from the per-radius table through a raw pointer, the
/// rectangle packets go through the SDK ordered stream,
/// and the loop keeps few enough values live to stay out of the stack (the
/// PS1 has no data cache, so every spill is a RAM stall).
#[inline(never)]
unsafe fn disc_fill_list<const DITHER: bool>(
    cx: i16,
    cy: i16,
    radius: i16,
    clip: ClipRect,
    cmd: u32,
    tex: u32,
) {
    let scale = SCALE as i32;
    // SCALE is initialized to 2 and its setter only admits 1 or 2. Shifts keep
    // the signed transform exact without a multiply interlock for every span.
    let scale_shift = (scale - 1) as u32;
    // Screen x = px * scale + sx_off (camera and centring folded in).
    let ox = ofs_x() as i32;
    let oy = V_OFS as i32;
    let sx_off = ox - ((CAM_X as i32) << scale_shift);
    let sy_off = oy - ((CAM_Y as i32) << scale_shift);
    let (cx, cy) = (cx as i32, cy as i32);
    let (clx, cty, crx, cby) = (clip.0 as i32, clip.1 as i32, clip.2 as i32, clip.3 as i32);
    if disc_outside_view(cx, cy, radius as i32, clip, scale, sx_off, sy_off) {
        return;
    }
    let (mut p, end) = disc_table(radius as i32);
    let list = stream();
    while p != end {
        let run = *p;
        p = p.add(1);
        let dx = (run & 0xFF) as i32;
        let lx = (cx - dx).max(clx);
        let rx = (cx + dx + 1).min(crx);
        if rx <= lx {
            continue;
        }
        let x = (lx << scale_shift) + sx_off;
        let w = (rx - lx) << scale_shift;
        let dy0 = ((run >> 8) & 0xFF) as i32;
        let dy1 = (run >> 16) as i32;
        // The run below the centre (or straddling it), then its mirror above.
        let mut ty = if dy0 == 0 { cy - dy1 } else { cy + dy0 };
        let mut by = cy + dy1 + 1;
        let mut halves = if dy0 == 0 { 1 } else { 2 };
        loop {
            let t = ty.max(cty);
            let b = by.min(cby);
            if b > t {
                let y = (t << scale_shift) + sy_off;
                let vertex = pack_vertex(x as i16, y as i16);
                let size = pack_xy(w as u16, ((b - t) << scale_shift) as u16);
                if DITHER {
                    let uv = tex | (((y - oy) as u32 & 0xFF) << 8) | ((x - ox) as u32 & 0xFF);
                    list.push_packet([cmd, vertex, uv, size]);
                } else {
                    list.push_packet([cmd, vertex, size]);
                }
            }
            halves -= 1;
            if halves == 0 {
                break;
            }
            ty = cy - dy1;
            by = cy - dy0 + 1;
        }
    }
}

/// Reject only discs whose clipped bounding box cannot touch the framebuffer.
/// Keep the original packet path for coordinates that may wrap in GP0's signed
/// 11-bit vertex fields. Palette uploads and texture-window changes remain in
/// the caller, even when the disc itself contributes no pixels.
/// Both deferred cart loops use a 320x240 framebuffer and matching draw area;
/// the backend's scale setter restricts the positive transform to 1x or 2x.
#[inline(always)]
fn disc_outside_view(
    cx: i32,
    cy: i32,
    radius: i32,
    clip: ClipRect,
    scale: i32,
    sx_off: i32,
    sy_off: i32,
) -> bool {
    let left = (cx - radius).max(clip.0 as i32);
    let right = (cx + radius + 1).min(clip.2 as i32);
    let top = (cy - radius).max(clip.1 as i32);
    let bottom = (cy + radius + 1).min(clip.3 as i32);
    if right <= left || bottom <= top {
        return true;
    }
    let (left, right) = (left * scale + sx_off, right * scale + sx_off);
    let (top, bottom) = (top * scale + sy_off, bottom * scale + sy_off);
    if left < -1024 || top < -1024 || right > 1024 || bottom > 1024 {
        return false;
    }
    right <= 0 || left >= 320 || bottom <= 0 || top >= 240
}

/// The packed merged runs of a disc of radius `r`: the table's for the radii
/// it covers, else computed into a static scratch (a stack array here cost a
/// `memset` per disc).
#[inline(always)]
unsafe fn disc_table(r: i32) -> (*const u32, *const u32) {
    if r as usize <= DISC_LUT_RADIUS && DISC_LUT_BUILT {
        let (n, runs) = &DISC_LUT[r as usize];
        let p = runs.as_ptr();
        return (p, p.add(*n as usize));
    }
    disc_table_slow(r)
}

#[inline(never)]
unsafe fn disc_table_slow(r: i32) -> (*const u32, *const u32) {
    if r as usize <= DISC_LUT_RADIUS {
        build_disc_lut();
        return disc_table(r);
    }
    static mut SCRATCH: [u32; 2 * (DISC_LUT_RADIUS + 1)] = [0; 2 * (DISC_LUT_RADIUS + 1)];
    let mut runs = DiscRuns::computed(r);
    let mut n = 0usize;
    while let Some((dx, dy0, dy1)) = runs.next() {
        if n == SCRATCH.len() {
            break;
        }
        SCRATCH[n] = dx as u32 | (dy0 as u32) << 8 | (dy1 as u32) << 16;
        n += 1;
    }
    let p = SCRATCH.as_ptr();
    (p, p.add(n))
}

/// The immediate-mode disc (launcher, pause overlay): plain and slow is fine.
unsafe fn disc_fill_immediate(
    cx: i16,
    cy: i16,
    radius: i16,
    clip: ClipRect,
    cmd: u32,
    tex: Option<u32>,
) {
    let scale = SCALE as i32;
    let sx_off = ofs_x() as i32 - CAM_X as i32 * scale;
    let sy_off = V_OFS as i32 - CAM_Y as i32 * scale;
    let (cx, cy) = (cx as i32, cy as i32);
    let (clx, cty, crx, cby) = (clip.0 as i32, clip.1 as i32, clip.2 as i32, clip.3 as i32);
    let mut runs = DiscRuns::new(radius as i32);
    while let Some((dx, dy0, dy1)) = runs.next() {
        let lx = (cx - dx).max(clx);
        let rx = (cx + dx + 1).min(crx);
        if rx <= lx {
            continue;
        }
        let x = (lx * scale + sx_off) as i16;
        let w = ((rx - lx) * scale) as u16;
        let ranges = if dy0 == 0 {
            [(cy - dy1, cy + dy1 + 1), (0, 0)]
        } else {
            [(cy + dy0, cy + dy1 + 1), (cy - dy1, cy - dy0 + 1)]
        };
        for (ty, by) in ranges {
            let t = ty.max(cty);
            let b = by.min(cby);
            if b <= t {
                continue;
            }
            let y = (t * scale + sy_off) as i16;
            let h = ((b - t) * scale) as u16;
            wait_cmd_ready();
            write_gp0(cmd);
            write_gp0(pack_vertex(x, y));
            if let Some(t) = tex {
                write_gp0(uv_word(t, x, y));
            }
            write_gp0(pack_xy(w, h));
        }
    }
}

/// Texcoord word for a dithered rectangle at screen `(x, y)`: the pattern
/// phase is the 128-space position times the scale, which the texture window
/// reduces to its low 3 bits, recovered from the screen position.
#[inline(always)]
fn uv_word(tex: u32, x: i16, y: i16) -> u32 {
    let (u, v) = (
        (x - ofs_x()) as u32 & 0xFF,
        (y - unsafe { V_OFS }) as u32 & 0xFF,
    );
    tex | (v << 8) | u
}

/// The rows of a PICO-8 disc of radius `r`, lower half, as `(dx, dy0, dy1)`:
/// rows `dy0..=dy1` below the centre (`dy0 == 0`: the run straddles the
/// centre and its mirror is included) span `cx-dx..=cx+dx`, by PICO-8's rule
/// (the largest `dx` with `dx*dx + dy*dy <= r*r`). Rows with the same `dx` are
/// merged. Radii up to `DISC_LUT_RADIUS` read a table built on first use (one
/// packed word per run); larger ones are computed, squares tracked
/// incrementally.
enum DiscRuns {
    Table {
        next: *const u32,
        end: *const u32,
    },
    Computed {
        r: i32,
        r2: i32,
        dx: i32,
        dx2: i32,
        dy: i32,
        dy2: i32,
        run_dx: i32,
        run_start: i32,
        done: bool,
    },
}
const DISC_LUT_RADIUS: usize = 31;
/// Per radius: run count, then runs packed as `dx | dy0 << 8 | dy1 << 16`.
static mut DISC_LUT: [(u32, [u32; DISC_LUT_RADIUS + 1]); DISC_LUT_RADIUS + 1] =
    [(0, [0; DISC_LUT_RADIUS + 1]); DISC_LUT_RADIUS + 1];
static mut DISC_LUT_BUILT: bool = false;

impl DiscRuns {
    #[inline(always)]
    fn new(r: i32) -> Self {
        if r as usize <= DISC_LUT_RADIUS {
            unsafe {
                if !DISC_LUT_BUILT {
                    build_disc_lut();
                }
                let (n, runs) = &DISC_LUT[r as usize];
                let next = runs.as_ptr();
                return DiscRuns::Table {
                    next,
                    end: next.add(*n as usize),
                };
            }
        }
        DiscRuns::computed(r)
    }
    fn computed(r: i32) -> Self {
        DiscRuns::Computed {
            r,
            r2: r * r,
            dx: r,
            dx2: r * r,
            dy: 0,
            dy2: 0,
            run_dx: r,
            run_start: 0,
            done: false,
        }
    }
    #[inline(always)]
    fn next(&mut self) -> Option<(i32, i32, i32)> {
        match self {
            DiscRuns::Table { next, end } => {
                if *next == *end {
                    return None;
                }
                let w = unsafe { **next };
                *next = unsafe { next.add(1) };
                Some((
                    (w & 0xFF) as i32,
                    ((w >> 8) & 0xFF) as i32,
                    (w >> 16) as i32,
                ))
            }
            DiscRuns::Computed {
                r,
                r2,
                dx,
                dx2,
                dy,
                dy2,
                run_dx,
                run_start,
                done,
            } => {
                if *done {
                    return None;
                }
                loop {
                    if *dy > *r {
                        *done = true;
                        return Some((*run_dx, *run_start, *r));
                    }
                    while *dx2 + *dy2 > *r2 {
                        *dx2 -= 2 * *dx - 1;
                        *dx -= 1;
                    }
                    let out = if *dx != *run_dx {
                        let o = (*run_dx, *run_start, *dy - 1);
                        *run_dx = *dx;
                        *run_start = *dy;
                        Some(o)
                    } else {
                        None
                    };
                    *dy2 += 2 * *dy + 1;
                    *dy += 1;
                    if out.is_some() {
                        return out;
                    }
                }
            }
        }
    }
}

unsafe fn build_disc_lut() {
    for r in 0..=DISC_LUT_RADIUS {
        let mut runs = DiscRuns::computed(r as i32);
        let mut n = 0usize;
        while let Some((dx, dy0, dy1)) = runs.next() {
            DISC_LUT[r].1[n] = dx as u32 | (dy0 as u32) << 8 | (dy1 as u32) << 16;
            n += 1;
        }
        DISC_LUT[r].0 = n as u32;
    }
    DISC_LUT_BUILT = true;
}

/// PICO-8 `circ(x,y,r,c)` -- 1px outline (midpoint circle), each point a 2x2
/// screen dot. Used for the berry pickup flash ring.
pub fn circ(cx: i16, cy: i16, radius: i16, c: i32) {
    if radius < 0 {
        return;
    }
    let (r, g, b) = rgb(c);
    let dot = |x: i16, y: i16| {
        let x0 = sx(x);
        let y0 = sy(y);
        emit([
            0x6000_0000 | pack_color(r, g, b),
            pack_vertex(x0, y0),
            pack_xy(2, 2),
        ]);
    };
    let mut x = radius as i32;
    let mut y = 0i32;
    let mut err = 0i32;
    while x >= y {
        for (ox, oy) in [
            (x, y),
            (y, x),
            (-x, y),
            (-y, x),
            (x, -y),
            (y, -x),
            (-x, -y),
            (-y, -x),
        ] {
            dot(cx + ox as i16, cy + oy as i16);
        }
        y += 1;
        if err <= 0 {
            err += 2 * y + 1;
        }
        if err > 0 {
            x -= 1;
            err -= 2 * x + 1;
        }
    }
}

// --------------------------------------------------------------------
// Text
// --------------------------------------------------------------------

/// PICO-8 `print(str,x,y,c)` -- 4px advance per char, font from the font tpage
/// with a per-colour CLUT.
pub fn print(s: &[u8], x: i16, y: i16, c: i32) {
    let clut_idx = (unsafe { PAL[(c as usize) & 15] }) as usize;
    emit_upload(
        VramRect::new(TEXT_CLUT.x(), TEXT_CLUT.y(), 16, 1),
        &TEXT_CLUTS[clut_idx & 15],
    );
    let clut_word = TEXT_CLUT.uv_clut_word();
    emit([draw_mode_word(FONT_TPAGE)]);

    let mut cx = x;
    for &ch in s {
        let ci = (ch & 0x7F) as i32;
        let u0 = ((ci % 16) * 16) as u8;
        let v0 = ((ci / 16) * 16) as u8;
        draw_cell(sx(cx), sy(y), u0, v0, clut_word, FONT_TPAGE, false, false);
        cx += 4; // PICO-8 4px advance
    }
}

// --------------------------------------------------------------------
// State
// --------------------------------------------------------------------

/// PICO-8 `camera(x,y)`.
pub fn camera(x: i16, y: i16) {
    unsafe {
        CAM_X = x;
        CAM_Y = y;
    }
}

/// Which ping-pong slot the sprite CLUT currently lives in.
static mut SPRITE_CLUT_SLOT: bool = false;

/// The sprite CLUT's active slot. spr()/map() build their clut word from this,
/// and it flips on every `pal()` change so the GPU reloads its CLUT cache.
/// The sprite CLUT slot the next textured draw uses, uploading the current
/// `pal()` remap first if it changed. Lazy on purpose: see [`sync_sprite_clut`].
fn sprite_clut() -> Clut {
    unsafe {
        if CLUT_DIRTY {
            sync_sprite_clut();
        }
        if SPRITE_CLUT_SLOT {
            SPRITE_CLUT_B
        } else {
            SPRITE_CLUT_A
        }
    }
}

/// Deferred-upload flag: `pal()` only records the remap; the CLUT is rebuilt
/// once, by the next sprite/map draw that needs it.
static mut CLUT_DIRTY: bool = false;

/// `map()` draws tiles with this flag bit through [`MAP_ALT_CLUT`]; -1 = off.
static mut MAP_ALT_FLAG: i32 = -1;

/// Give `map()` a second palette: tiles whose flag `flag_bit` is set draw with
/// `pairs` (PICO-8 `pal(a, b)` remaps) applied, the rest with the normal CLUT.
/// This is how a cart's per-level `pal()` inside its tile loop is honoured: the
/// swapped tiles just reference a different CLUT slot, so there is no second
/// tile pass and no GPU flush around a CLUT rewrite. `flag_bit` < 0 turns it off.
pub fn set_map_alt_pal(pairs: &[(i32, i32)], flag_bit: i32) {
    unsafe {
        MAP_ALT_FLAG = flag_bit;
        if flag_bit < 0 {
            return;
        }
        let mut c = [0u16; 16];
        let mut i = 0usize;
        while i < 16 {
            c[i] = PICO8_CLUT[i];
            i += 1;
        }
        for &(a, b) in pairs {
            c[(a as usize) & 15] = PICO8_CLUT[(b as usize) & 15];
        }
        emit_upload(VramRect::new(MAP_ALT_CLUT.x(), MAP_ALT_CLUT.y(), 16, 1), &c);
    }
}

/// Re-upload the sprite CLUT so textured draws (spr/map) honour the current
/// `pal()` remap -- entry i = the palette colour PAL[i] points at. circfill/
/// rectfill already remap via PAL directly; sprites read this CLUT.
///
/// Crucially this ping-pongs to the OTHER slot: the PS1 GPU caches the CLUT
/// keyed on the clut word and does NOT reload it when the same slot's VRAM is
/// overwritten, so re-uploading in place leaves the sprite on the stale palette
/// on real hardware (Madeline's hair stuck red). Moving slots changes the clut
/// word, forcing the GPU to reload. And it must happen ONCE per change, at the
/// next draw, not per `pal()` call: a level swap is two calls (`pal(2,12)
/// pal(5,2)`), and toggling twice landed back on the slot the base tiles had
/// just been drawn with -- same clut word, stale cache, no recolour (Celeste 2's
/// glacial caves / golden valley palettes never showed).
unsafe fn sync_sprite_clut() {
    let mut c = [0u16; 16];
    let mut i = 0usize;
    while i < 16 {
        c[i] = PICO8_CLUT[(PAL[i] as usize) & 15];
        i += 1;
    }
    SPRITE_CLUT_SLOT = !SPRITE_CLUT_SLOT;
    CLUT_DIRTY = false;
    let clut = if SPRITE_CLUT_SLOT {
        SPRITE_CLUT_B
    } else {
        SPRITE_CLUT_A
    };
    emit_upload(VramRect::new(clut.x(), clut.y(), 16, 1), &c);
}

/// Wait for the GPU to finish the queued draws. Call before a palette change
/// that must not retro-actively affect already-issued sprite draws (the sprite
/// CLUT is shared VRAM, so an unset/reset would otherwise recolour them).
pub fn flush() {
    if unsafe { DEFERRED } {
        return; // the list draws in order; CLUT slots ping-pong
    }
    gpu::draw_sync();
}

/// PICO-8 `pal(a,b)` -- remap draw colour `a` to `b`.
pub fn pal(a: i32, b: i32) {
    unsafe {
        PAL[(a as usize) & 15] = (b & 15) as u8;
        CLUT_DIRTY = true;
    }
}

/// PICO-8 `pal()` -- reset the colour remap.
pub fn pal_reset() {
    unsafe {
        let mut i = 0u8;
        while (i as usize) < 16 {
            PAL[i as usize] = i;
            i += 1;
        }
        CLUT_DIRTY = true;
    }
}
