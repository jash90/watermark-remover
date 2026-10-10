use super::geometry::Rect;
use image::{GrayImage, Luma};
use imageproc::distance_transform::Norm;
use imageproc::morphology;

/// Mask value marking pixels an engine must fill.
pub const HOLE: u8 = 255;

/// Crop-sized mask with `hole` (given in image coordinates) set to HOLE.
pub fn rect_mask(crop: Rect, hole: Rect) -> GrayImage {
    GrayImage::from_fn(crop.w, crop.h, |x, y| {
        let (ix, iy) = (crop.x + x, crop.y + y);
        let inside = ix >= hole.x && ix < hole.x + hole.w && iy >= hole.y && iy < hole.y + hole.h;
        Luma([if inside { HOLE } else { 0 }])
    })
}

/// Grows the hole by `radius` pixels (square structuring element).
pub fn dilate(mask: &GrayImage, radius: u8) -> GrayImage {
    if radius == 0 {
        return mask.clone();
    }
    morphology::dilate(mask, Norm::LInf, radius)
}

pub fn hole_count(mask: &GrayImage) -> usize {
    mask.pixels().filter(|p| p[0] == HOLE).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_mask_marks_exactly_the_hole_in_crop_coordinates() {
        let crop = Rect { x: 10, y: 10, w: 20, h: 20 };
        let hole = Rect { x: 15, y: 12, w: 4, h: 3 };
        let m = rect_mask(crop, hole);
        assert_eq!(m.dimensions(), (20, 20));
        assert_eq!(hole_count(&m), 12);
        assert_eq!(m.get_pixel(5, 2)[0], HOLE);
        assert_eq!(m.get_pixel(8, 4)[0], HOLE);
        assert_eq!(m.get_pixel(9, 4)[0], 0);
        assert_eq!(m.get_pixel(5, 5)[0], 0);
    }

    #[test]
    fn dilate_grows_square_by_radius() {
        let crop = Rect { x: 0, y: 0, w: 20, h: 20 };
        let m = rect_mask(crop, Rect { x: 10, y: 10, w: 1, h: 1 });
        assert_eq!(hole_count(&dilate(&m, 2)), 25);
        assert_eq!(hole_count(&dilate(&m, 0)), 1);
    }
}
