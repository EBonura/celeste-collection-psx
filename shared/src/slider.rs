//! One volume slider for the pause overlay and the collection's settings screen: a track with a
//! filled part (eighths of the width) and, optionally, a knob at the end of the fill. Each screen
//! passes its geometry, its colours and the rectangle painter it draws with.

/// Where and in what colours a slider draws.
#[derive(Copy, Clone)]
pub struct Style<C: Copy> {
    /// Track length in pixels (the fill is `vol / 8` of it).
    pub width: i16,
    /// First and last track row, relative to the slider's `y`.
    pub rows: (i16, i16),
    /// The track.
    pub track: C,
    /// The fill when the row is selected, and when it is not.
    pub fill: (C, C),
    /// The knob at the end of the fill: rows above and below the slider's `y` it spans, its half
    /// width, and its colour when selected and when not.
    pub knob: Option<Knob<C>>,
}

/// The handle at the end of the fill.
#[derive(Copy, Clone)]
pub struct Knob<C: Copy> {
    /// First and last row, relative to the slider's `y`.
    pub rows: (i16, i16),
    /// Pixels either side of the fill's end.
    pub half_width: i16,
    /// Colour when selected, and when not.
    pub colour: (C, C),
}

/// Draw a slider at `(x, y)` showing `vol` of 8 steps. `rect(x0, y0, x1, y1, colour)` paints the
/// rectangle between the two corners as the screen's own primitive does.
pub fn draw<C: Copy>(
    x: i16,
    y: i16,
    vol: u16,
    lit: bool,
    style: &Style<C>,
    mut rect: impl FnMut(i16, i16, i16, i16, C),
) {
    let (top, bottom) = (y + style.rows.0, y + style.rows.1);
    rect(x, top, x + style.width, bottom, style.track);
    let fw = vol as i16 * style.width / 8;
    if fw > 0 {
        rect(
            x,
            top,
            x + fw,
            bottom,
            if lit { style.fill.0 } else { style.fill.1 },
        );
    }
    if let Some(knob) = style.knob {
        let kx = x + fw;
        rect(
            kx - knob.half_width,
            y + knob.rows.0,
            kx + knob.half_width,
            y + knob.rows.1,
            if lit { knob.colour.0 } else { knob.colour.1 },
        );
    }
}
