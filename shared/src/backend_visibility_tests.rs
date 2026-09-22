#[cfg(test)]
mod disc_visibility_tests {
    use super::*;

    // Walk the existing renderer's rectangles, including its merged centre
    // span and mirrored halves. This oracle does not use the new bounding box.
    fn old_path_touches_view(
        cx: i32,
        cy: i32,
        radius: i32,
        clip: ClipRect,
        scale: i32,
        ox: i32,
        oy: i32,
    ) -> bool {
        let mut runs = DiscRuns::computed(radius);
        while let Some((dx, dy0, dy1)) = runs.next() {
            let left = (cx - dx).max(clip.0 as i32);
            let right = (cx + dx + 1).min(clip.2 as i32);
            if right <= left {
                continue;
            }
            let halves = if dy0 == 0 {
                [(cy - dy1, cy + dy1 + 1), (0, 0)]
            } else {
                [(cy + dy0, cy + dy1 + 1), (cy - dy1, cy - dy0 + 1)]
            };
            for (top, bottom) in halves {
                let top = top.max(clip.1 as i32);
                let bottom = bottom.min(clip.3 as i32);
                if bottom <= top {
                    continue;
                }
                // GP0 vertices use signed 11-bit coordinates. Conservatively
                // count the entire rectangle, independent of dither transparency.
                let x = ((left * scale + ox) << 21) >> 21;
                let y = ((top * scale + oy) << 21) >> 21;
                let w = (right - left) * scale;
                let h = (bottom - top) * scale;
                if x < 320 && y < 240 && x + w > 0 && y + h > 0 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn rejects_recorded_empty_fog_and_offscreen_cloud_bounds() {
        assert!(disc_outside_view(
            210,
            0,
            8,
            (197, -13, 222, -1),
            2,
            -202,
            -16
        ));
        assert!(disc_outside_view(
            50,
            400,
            15,
            (0, 390, 128, 207),
            2,
            32,
            -166
        ));
    }

    #[test]
    fn retains_edge_pixels_and_wrapping_coordinates() {
        assert!(!disc_outside_view(0, 0, 0, NO_CLIP, 1, 0, 0));
        assert!(!disc_outside_view(319, 239, 0, NO_CLIP, 1, 0, 0));
        assert!(disc_outside_view(320, 239, 0, NO_CLIP, 1, 0, 0));
        assert!(disc_outside_view(319, 240, 0, NO_CLIP, 1, 0, 0));
        // 2048 maps back to screen zero in GP0. Never discard this path based
        // on its unwrapped position, even though normal carts stay much closer.
        assert!(!disc_outside_view(2048, 2048, 0, NO_CLIP, 1, 0, 0));
        assert!(old_path_touches_view(2048, 2048, 0, NO_CLIP, 1, 0, 0));
    }

    #[test]
    fn every_rejected_disc_has_no_old_visible_rectangle() {
        let clips = [
            NO_CLIP,
            (0, 0, 128, 128),
            (-17, -13, 45, 19),
            (0, 90, 128, 40),
        ];
        for scale in [1, 2] {
            for (ox, oy) in [(96, 56), (32, -8), (-202, -16), (32, -166)] {
                for radius in [0, 1, 2, 7, 15, 16, 31, 32, 48] {
                    for cx in (-160..=384).step_by(8) {
                        for cy in (-160..=320).step_by(8) {
                            for clip in clips {
                                if disc_outside_view(cx, cy, radius, clip, scale, ox, oy) {
                                    assert!(!old_path_touches_view(cx, cy, radius, clip, scale, ox, oy),
                                        "visible disc rejected: {cx},{cy} r{radius} {clip:?} scale{scale} offset{ox},{oy}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
