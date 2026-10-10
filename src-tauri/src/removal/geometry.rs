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
    let x0 = r.x.max(0) as u32;
    let y0 = r.y.max(0) as u32;
    let x1 = ((r.x + r.width).max(0) as u32).min(img_w);
    let y1 = ((r.y + r.height).max(0) as u32).min(img_h);
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
