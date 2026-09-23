use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, ImageReader};
use std::io::Cursor;

/// Decodes an image and applies its EXIF orientation, so pixel coordinates
/// match what the webview shows.
pub fn load_oriented(path: &str) -> Result<DynamicImage, String> {
    let mut decoder = ImageReader::open(path)
        .and_then(|r| r.with_guessed_format())
        .map_err(|e| format!("Failed to open image: {}", e))?
        .into_decoder()
        .map_err(|e| format!("Failed to decode image: {}", e))?;
    let orientation = image::ImageDecoder::orientation(&mut decoder)
        .map_err(|e| format!("Failed to read orientation: {}", e))?;
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| format!("Failed to decode image: {}", e))?;
    img.apply_orientation(orientation);
    Ok(img)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Png,
    Jpeg,
    /// The `image` crate's WebP encoder is lossless-only.
    WebpLossless,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
            OutputFormat::WebpLossless => "webp",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            OutputFormat::Png => "image/png",
            OutputFormat::Jpeg => "image/jpeg",
            OutputFormat::WebpLossless => "image/webp",
        }
    }
}

/// Keeps the original container where possible; `lossless` turns JPEG into PNG.
pub fn output_format(original_ext: &str, lossless: bool) -> OutputFormat {
    match (original_ext.to_ascii_lowercase().as_str(), lossless) {
        ("jpg" | "jpeg", false) => OutputFormat::Jpeg,
        ("webp", _) => OutputFormat::WebpLossless,
        _ => OutputFormat::Png,
    }
}

pub fn encode(img: &DynamicImage, format: OutputFormat) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    match format {
        OutputFormat::Jpeg => JpegEncoder::new_with_quality(&mut buf, 95)
            .encode_image(&img.to_rgb8())
            .map_err(|e| format!("Failed to encode JPEG: {}", e))?,
        OutputFormat::Png => img
            .write_to(&mut buf, ImageFormat::Png)
            .map_err(|e| format!("Failed to encode PNG: {}", e))?,
        OutputFormat::WebpLossless => DynamicImage::ImageRgba8(img.to_rgba8())
            .write_to(&mut buf, ImageFormat::WebP)
            .map_err(|e| format!("Failed to encode WebP: {}", e))?,
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgb, RgbImage};

    #[test]
    fn picks_format_from_extension() {
        assert_eq!(output_format("JPG", false), OutputFormat::Jpeg);
        assert_eq!(output_format("jpeg", true), OutputFormat::Png);
        assert_eq!(output_format("webp", false), OutputFormat::WebpLossless);
        assert_eq!(output_format("gif", false), OutputFormat::Png);
        assert_eq!(output_format("png", false), OutputFormat::Png);
    }

    #[test]
    fn encoded_bytes_match_declared_format() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([1, 2, 3])));
        for (fmt, expected) in [
            (OutputFormat::Png, ImageFormat::Png),
            (OutputFormat::Jpeg, ImageFormat::Jpeg),
            (OutputFormat::WebpLossless, ImageFormat::WebP),
        ] {
            let bytes = encode(&img, fmt).unwrap();
            assert_eq!(image::guess_format(&bytes).unwrap(), expected);
        }
    }

    #[test]
    fn load_oriented_applies_exif_rotation() {
        // 4x2 JPEG with EXIF Orientation=6 (rotate 90° CW) must load as 2x4.
        let path = std::env::temp_dir().join("wmr_orientation6.jpg");
        std::fs::write(&path, jpeg_with_orientation(4, 2, 6)).unwrap();
        let img = load_oriented(path.to_str().unwrap()).unwrap();
        assert_eq!(img.dimensions(), (2, 4));
    }

    /// Minimal JPEG with an APP1/EXIF segment carrying only the Orientation tag.
    fn jpeg_with_orientation(w: u32, h: u32, orientation: u16) -> Vec<u8> {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&RgbImage::from_pixel(w, h, Rgb([50, 60, 70])))
            .unwrap();
        let mut tiff = vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1];
        tiff.extend_from_slice(&[0x01, 0x12, 0, 3, 0, 0, 0, 1]);
        tiff.extend_from_slice(&orientation.to_be_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff);
        let len = (app1.len() + 2) as u16;
        let mut out = jpeg[..2].to_vec(); // SOI
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&app1);
        out.extend_from_slice(&jpeg[2..]);
        out
    }
}
