// Frozen from pico8-psx 048a8a8596deeecf524a8cf403c4e80456c09565,
// shared/src/backend.rs. Original multiplication-based packet generator.
unsafe fn disc_fill_list<const DITHER: bool>(
    cx: i16,
    cy: i16,
    radius: i16,
    clip: ClipRect,
    cmd: u32,
    tex: u32,
) {
    let scale = SCALE as i32;
    // Screen x = px * scale + sx_off (camera and centring folded in).
    let ox = ofs_x() as i32;
    let oy = V_OFS as i32;
    let sx_off = ox - CAM_X as i32 * scale;
    let sy_off = oy - CAM_Y as i32 * scale;
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
        let x = lx * scale + sx_off;
        let w = (rx - lx) * scale;
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
                let y = t * scale + sy_off;
                let vertex = pack_vertex(x as i16, y as i16);
                let size = pack_xy(w as u16, ((b - t) * scale) as u16);
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

