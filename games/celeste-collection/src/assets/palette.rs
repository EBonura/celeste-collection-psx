// PICO-8 16-colour sprite/map CLUT (RGB555, entry 0 = transparent).
//
// The launcher only needs this CLUT (to draw the cart-label covers, which
// are 4bpp in the PICO-8 palette); the games carry the full palette table
// (PICO8_RGB / TEXT_CLUTS) for their own flat-shape and text rendering.
pub static PICO8_CLUT: [u16; 16] = [
    0x0000, 0x28a3, 0x288f, 0x2a00, 0x1955, 0x254b, 0x6318, 0x77df, 0x241f, 0x029f, 0x13bf, 0x1b80,
    0x7ea5, 0x4dd0, 0x55df, 0x573f,
];
