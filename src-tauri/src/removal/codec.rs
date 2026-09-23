use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ImageEncoder, ImageFormat, ImageReader};
use jpeg_encoder::{ColorType, QuantizationTableType, SamplingFactor};
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

/// Quality used when the source is not a JPEG (or its tables can't be reused).
const FALLBACK_JPEG_QUALITY: u8 = 90;

/// JPEG natural-order index of the k-th zigzag coefficient (ITU T.81 Figure A.6).
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// Compression settings of a source JPEG. Re-encoding with the same tables and
/// chroma subsampling keeps the output the same size and quality as the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpegProfile {
    /// Luma quantization table, natural (row-major) order.
    pub luma: [u16; 64],
    /// Chroma quantization table, natural (row-major) order.
    pub chroma: [u16; 64],
    /// Luma (horizontal, vertical) sampling factors, e.g. (2, 2) for 4:2:0.
    pub sampling: (u8, u8),
}

/// Reads quantization tables and subsampling from JPEG bytes; `None` for non-JPEGs.
pub fn read_jpeg_profile(bytes: &[u8]) -> Option<JpegProfile> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut tables: [Option<[u16; 64]>; 4] = [None; 4];
    let mut components: Vec<(u8, u8, usize)> = Vec::new(); // (h, v, table id)
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let segment = bytes.get(i + 4..i + 2 + len)?;
        match marker {
            0xDB => {
                let mut j = 0;
                while j < segment.len() {
                    let (precision, id) = (segment[j] >> 4, (segment[j] & 0x0F) as usize);
                    let width = if precision == 0 { 1 } else { 2 };
                    let values = segment.get(j + 1..j + 1 + 64 * width)?;
                    let mut table = [0u16; 64];
                    for (k, &natural) in ZIGZAG.iter().enumerate() {
                        table[natural] = if width == 1 {
                            values[k] as u16
                        } else {
                            u16::from_be_bytes([values[2 * k], values[2 * k + 1]])
                        };
                    }
                    *tables.get_mut(id)? = Some(table);
                    j += 1 + 64 * width;
                }
            }
            0xC0..=0xC2 => {
                let count = *segment.get(5)? as usize;
                for c in 0..count {
                    let sampling = *segment.get(7 + 3 * c)?;
                    let table_id = *segment.get(8 + 3 * c)? as usize;
                    components.push((sampling >> 4, sampling & 0x0F, table_id));
                }
            }
            0xDA => break,
            _ => {}
        }
        i += 2 + len;
    }
    let (h, v, luma_id) = *components.first()?;
    let chroma_id = components.get(1).map_or(luma_id, |c| c.2);
    let luma = (*tables.get(luma_id)?)?;
    let chroma = (*tables.get(chroma_id)?)?;
    // The encoder writes 8-bit tables only.
    if luma.iter().chain(chroma.iter()).any(|&q| q == 0 || q > 255) {
        return None;
    }
    Some(JpegProfile { luma, chroma, sampling: (h, v) })
}

/// Encodes `img`. For JPEG, `jpeg` carries the source's compression settings so
/// an unchanged photo doesn't grow; without it, quality 90 with 4:2:0 is used.
pub fn encode(img: &DynamicImage, format: OutputFormat, jpeg: Option<&JpegProfile>) -> Result<Vec<u8>, String> {
    match format {
        OutputFormat::Jpeg => encode_jpeg(img, jpeg),
        OutputFormat::Png => {
            let mut buf = Vec::new();
            PngEncoder::new_with_quality(&mut buf, CompressionType::Best, FilterType::Adaptive)
                .write_image(img.as_bytes(), img.width(), img.height(), img.color().into())
                .map_err(|e| format!("Failed to encode PNG: {}", e))?;
            Ok(buf)
        }
        OutputFormat::WebpLossless => {
            let mut buf = Cursor::new(Vec::new());
            DynamicImage::ImageRgba8(img.to_rgba8())
                .write_to(&mut buf, ImageFormat::WebP)
                .map_err(|e| format!("Failed to encode WebP: {}", e))?;
            Ok(buf.into_inner())
        }
    }
}

fn encode_jpeg(img: &DynamicImage, profile: Option<&JpegProfile>) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
    let (w, h) = (
        u16::try_from(rgb.width()).map_err(|_| "Image is too wide for JPEG".to_string())?,
        u16::try_from(rgb.height()).map_err(|_| "Image is too tall for JPEG".to_string())?,
    );
    let mut buf = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut buf, FALLBACK_JPEG_QUALITY);
    let sampling = profile
        .and_then(|p| SamplingFactor::from_factors(p.sampling.0, p.sampling.1))
        .unwrap_or(SamplingFactor::F_2_2);
    encoder.set_sampling_factor(sampling);
    if let Some(p) = profile {
        encoder.set_quantization_tables(
            QuantizationTableType::Custom(Box::new(p.luma)),
            QuantizationTableType::Custom(Box::new(p.chroma)),
        );
    }
    encoder
        .encode(rgb.as_raw(), w, h, ColorType::Rgb)
        .map_err(|e| format!("Failed to encode JPEG: {}", e))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::jpeg::JpegEncoder;
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
            let bytes = encode(&img, fmt, None).unwrap();
            assert_eq!(image::guess_format(&bytes).unwrap(), expected);
        }
    }

    fn textured(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            let n = ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) % 40) as u8;
            Rgb([(x / 3) as u8 + n, (y / 3) as u8 + n, 120 + n])
        })
    }

    /// A 4:2:0 JPEG at quality 80, like a typical camera or web photo.
    fn source_jpeg(img: &RgbImage) -> Vec<u8> {
        let mut out = Vec::new();
        let mut enc = jpeg_encoder::Encoder::new(&mut out, 80);
        enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
        enc.encode(img.as_raw(), img.width() as u16, img.height() as u16, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        out
    }

    #[test]
    fn reads_quantization_tables_and_sampling_from_source() {
        let src = source_jpeg(&textured(64, 48));
        let profile = read_jpeg_profile(&src).expect("profile");
        assert_eq!(profile.sampling, (2, 2));
        assert!(profile.luma.iter().all(|&v| v > 0));
        assert_ne!(profile.luma, profile.chroma);
    }

    #[test]
    fn reencoding_with_source_profile_keeps_tables_and_size() {
        let img = textured(400, 300);
        let src = source_jpeg(&img);
        let profile = read_jpeg_profile(&src).unwrap();
        let decoded = image::load_from_memory(&src).unwrap();
        let out = encode(&decoded, OutputFormat::Jpeg, Some(&profile)).unwrap();
        assert_eq!(read_jpeg_profile(&out).unwrap(), profile);
        assert!(
            out.len() <= src.len() * 105 / 100,
            "re-encoded {} bytes vs source {} bytes",
            out.len(),
            src.len()
        );
    }

    #[test]
    fn jpeg_without_source_profile_uses_420_subsampling() {
        let img = DynamicImage::ImageRgb8(textured(64, 48));
        let out = encode(&img, OutputFormat::Jpeg, None).unwrap();
        assert_eq!(read_jpeg_profile(&out).unwrap().sampling, (2, 2));
    }

    #[test]
    fn non_jpeg_bytes_have_no_profile() {
        let img = DynamicImage::ImageRgb8(textured(8, 8));
        assert!(read_jpeg_profile(&encode(&img, OutputFormat::Png, None).unwrap()).is_none());
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
