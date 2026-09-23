use crate::image_processor::WatermarkRegion;

/// Axis-aligned rectangle in image pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Intersects a user-drawn region with the image bounds.
pub fn clamp_region(r: &WatermarkRegion, img_w: u32, img_h: u32) -> Result<Rect, String> {
    if r.width <= 0 || r.height <= 0 {
        return Err("Region must have a positive size".to_string());
    }
    // i64 so that x + width can't overflow for any i32 input.
    let clamp_x = |v: i64| v.clamp(0, img_w as i64) as u32;
    let clamp_y = |v: i64| v.clamp(0, img_h as i64) as u32;
    let (x0, y0) = (clamp_x(r.x as i64), clamp_y(r.y as i64));
    let x1 = clamp_x(r.x as i64 + r.width as i64);
    let y1 = clamp_y(r.y as i64 + r.height as i64);
    if x0 >= x1 || y0 >= y1 {
        return Err(format!("Region lies outside the {}x{} image", img_w, img_h));
    }
    Ok(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// Grows `inner` by `margin` on every side, clamped to the image.
pub fn expand(inner: Rect, margin: u32, img_w: u32, img_h: u32) -> Rect {
    let x0 = inner.x.saturating_sub(margin);
    let y0 = inner.y.saturating_sub(margin);
    let x1 = (inner.x + inner.w + margin).min(img_w);
    let y1 = (inner.y + inner.h + margin).min(img_h);
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// How much surrounding context an engine gets around the hole.
pub fn context_margin(hole: Rect) -> u32 {
    (hole.w.max(hole.h) / 2).max(32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x: i32, y: i32, width: i32, height: i32) -> WatermarkRegion {
        WatermarkRegion { x, y, width, height }
    }

    #[test]
    fn clamps_region_overhanging_right_edge() {
        let r = clamp_region(&region(90, 10, 20, 5), 100, 50).unwrap();
        assert_eq!(r, Rect { x: 90, y: 10, w: 10, h: 5 });
    }

    #[test]
    fn clamps_negative_origin() {
        let r = clamp_region(&region(-5, -5, 20, 20), 100, 50).unwrap();
        assert_eq!(r, Rect { x: 0, y: 0, w: 15, h: 15 });
    }

    #[test]
    fn rejects_region_outside_image() {
        assert!(clamp_region(&region(200, 10, 20, 5), 100, 50).is_err());
    }

    #[test]
    fn rejects_empty_region() {
        assert!(clamp_region(&region(10, 10, 0, 5), 100, 50).is_err());
    }

    #[test]
    fn clamps_region_with_huge_width() {
        let r = clamp_region(&region(10, 10, i32::MAX, i32::MAX), 100, 50).unwrap();
        assert_eq!(r, Rect { x: 10, y: 10, w: 90, h: 40 });
    }

    #[test]
    fn rejects_region_starting_at_i32_max() {
        assert!(clamp_region(&region(i32::MAX, 0, i32::MAX, 5), 100, 50).is_err());
    }

    #[test]
    fn expand_stops_at_borders() {
        let r = expand(Rect { x: 2, y: 2, w: 10, h: 10 }, 5, 15, 100);
        assert_eq!(r, Rect { x: 0, y: 0, w: 15, h: 17 });
    }

    #[test]
    fn context_margin_has_floor_and_scales() {
        assert_eq!(context_margin(Rect { x: 0, y: 0, w: 10, h: 10 }), 32);
        assert_eq!(context_margin(Rect { x: 0, y: 0, w: 200, h: 40 }), 100);
    }
}
