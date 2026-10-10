use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageEncoder, ImageReader};
use jpeg_encoder::{ColorType, QuantizationTableType, SamplingFactor};
use std::collections::HashMap;
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

/// Palette of an indexed (colour type 3) PNG, as RGBA with tRNS alpha applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PngPalette {
    pub colors: Vec<[u8; 4]>,
    pub bit_depth: u8,
}

/// How the source file was stored, so the output can be stored the same way.
#[derive(Debug, Clone, Default)]
pub struct SourceHints {
    pub jpeg: Option<JpegProfile>,
    pub png_palette: Option<PngPalette>,
    /// Colour profile (e.g. Display P3); without it colours shift in managed viewers.
    pub icc_profile: Option<Vec<u8>>,
}

pub fn read_source_hints(bytes: &[u8]) -> SourceHints {
    SourceHints {
        jpeg: read_jpeg_profile(bytes),
        png_palette: read_png_palette(bytes),
        icc_profile: read_icc_profile(bytes),
    }
}

fn read_icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.into_decoder().ok()?;
    image::ImageDecoder::icc_profile(&mut decoder).ok().flatten()
}

fn read_png_palette(bytes: &[u8]) -> Option<PngPalette> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.get(..8)? != SIGNATURE {
        return None;
    }
    let (mut colour_type, mut bit_depth) = (0u8, 8u8);
    let (mut plte, mut trns): (Option<&[u8]>, Option<&[u8]>) = (None, None);
    let mut i = 8;
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().ok()?) as usize;
        let data = bytes.get(i + 8..i + 8 + len)?;
        match &bytes[i + 4..i + 8] {
            b"IHDR" => {
                bit_depth = *data.get(8)?;
                colour_type = *data.get(9)?;
            }
            b"PLTE" => plte = Some(data),
            b"tRNS" => trns = Some(data),
            b"IDAT" | b"IEND" => break,
            _ => {}
        }
        i += 12 + len;
    }
    if colour_type != 3 {
        return None;
    }
    let colors = plte?
        .chunks_exact(3)
        .enumerate()
        .map(|(k, c)| [c[0], c[1], c[2], trns.and_then(|t| t.get(k).copied()).unwrap_or(255)])
        .collect();
    Some(PngPalette { colors, bit_depth })
}

/// Encodes `img`, storing it the way the source was stored where possible:
/// JPEG with the source's quantization tables and subsampling, PNG with the
/// source's palette. This keeps the output about the size of the input.
pub fn encode(img: &DynamicImage, format: OutputFormat, hints: &SourceHints) -> Result<Vec<u8>, String> {
    match format {
        OutputFormat::Jpeg => encode_jpeg(img, hints.jpeg.as_ref(), hints.icc_profile.as_deref()),
        OutputFormat::Png if hints.png_palette.is_some() => encode_indexed_png(
            img,
            hints.png_palette.as_ref().expect("checked above"),
            hints.icc_profile.as_deref(),
        ),
        OutputFormat::Png => {
            let mut buf = Vec::new();
            let mut encoder = PngEncoder::new_with_quality(&mut buf, CompressionType::Best, FilterType::Adaptive);
            if let Some(icc) = &hints.icc_profile {
                encoder.set_icc_profile(icc.clone()).map_err(|e| format!("Failed to embed colour profile: {}", e))?;
            }
            encoder
                .write_image(img.as_bytes(), img.width(), img.height(), img.color().into())
                .map_err(|e| format!("Failed to encode PNG: {}", e))?;
            Ok(buf)
        }
        OutputFormat::WebpLossless => {
            let mut buf = Vec::new();
            let rgba = img.to_rgba8();
            let mut encoder = WebPEncoder::new_lossless(&mut buf);
            if let Some(icc) = &hints.icc_profile {
                encoder.set_icc_profile(icc.clone()).map_err(|e| format!("Failed to embed colour profile: {}", e))?;
            }
            encoder
                .write_image(rgba.as_raw(), rgba.width(), rgba.height(), image::ExtendedColorType::Rgba8)
                .map_err(|e| format!("Failed to encode WebP: {}", e))?;
            Ok(buf)
        }
    }
}

fn encode_indexed_png(img: &DynamicImage, palette: &PngPalette, icc: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let rgba = img.to_rgba8();
    let mut lookup: HashMap<[u8; 4], u8> = HashMap::new();
    for (k, c) in palette.colors.iter().enumerate().rev() {
        lookup.insert(*c, k as u8);
    }
    let mut nearest = |p: [u8; 4]| -> u8 {
        *lookup.entry(p).or_insert_with(|| {
            let dist = |c: &[u8; 4]| (0..4).map(|i| (c[i] as i32 - p[i] as i32).pow(2)).sum::<i32>();
            palette.colors.iter().enumerate().min_by_key(|(_, c)| dist(c)).map_or(0, |(k, _)| k as u8)
        })
    };
    let depth = palette.bit_depth as u32;
    let per_byte = 8 / depth;
    let row_bytes = rgba.width().div_ceil(per_byte) as usize;
    let mut data = vec![0u8; row_bytes * rgba.height() as usize];
    for (x, y, p) in rgba.enumerate_pixels() {
        let index = nearest(p.0) as u32;
        let byte = y as usize * row_bytes + (x / per_byte) as usize;
        let shift = 8 - depth * (x % per_byte + 1);
        data[byte] |= (index << shift) as u8;
    }

    let bit_depth = match palette.bit_depth {
        1 => png::BitDepth::One,
        2 => png::BitDepth::Two,
        4 => png::BitDepth::Four,
        _ => png::BitDepth::Eight,
    };
    let mut buf = Vec::new();
    let mut info = png::Info::with_size(rgba.width(), rgba.height());
    info.icc_profile = icc.map(|p| std::borrow::Cow::Owned(p.to_vec()));
    let mut encoder = png::Encoder::with_info(&mut buf, info).map_err(|e| format!("Failed to encode PNG: {}", e))?;
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(bit_depth);
    encoder.set_compression(png::Compression::High);
    // Palette indices are not a continuous signal; delta filters only add noise.
    encoder.set_filter(png::Filter::NoFilter);
    encoder.set_palette(palette.colors.iter().flat_map(|c| [c[0], c[1], c[2]]).collect::<Vec<u8>>());
    if palette.colors.iter().any(|c| c[3] != 255) {
        encoder.set_trns(palette.colors.iter().map(|c| c[3]).collect::<Vec<u8>>());
    }
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&data))
        .map_err(|e| format!("Failed to encode PNG: {}", e))?;
    Ok(buf)
}

fn encode_jpeg(img: &DynamicImage, profile: Option<&JpegProfile>, icc: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
    let (w, h) = (
        u16::try_from(rgb.width()).map_err(|_| "Image is too wide for JPEG".to_string())?,
        u16::try_from(rgb.height()).map_err(|_| "Image is too tall for JPEG".to_string())?,
    );
    let mut buf = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut buf, FALLBACK_JPEG_QUALITY);
    // The default fixed Huffman tables (T.81 K.3) make the output larger than sources
    // written with optimized tables (mozjpeg, jpegoptim, every progressive JPEG).
    encoder.set_optimized_huffman_tables(true);
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
    if let Some(icc) = icc {
        encoder.add_icc_profile(icc).map_err(|e| format!("Failed to embed colour profile: {}", e))?;
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
    use image::ImageFormat;
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
            let bytes = encode(&img, fmt, &SourceHints::default()).unwrap();
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
        let hints = SourceHints { jpeg: Some(profile.clone()), ..Default::default() };
        let out = encode(&decoded, OutputFormat::Jpeg, &hints).unwrap();
        assert_eq!(read_jpeg_profile(&out).unwrap(), profile);
        assert!(
            out.len() <= src.len() * 105 / 100,
            "re-encoded {} bytes vs source {} bytes",
            out.len(),
            src.len()
        );
    }

    /// Re-encoding must not grow sources whose Huffman tables are already optimized,
    /// baseline or progressive (both carry image-specific DHT segments).
    #[test]
    fn reencoding_optimized_or_progressive_source_does_not_grow() {
        let img = textured(400, 300);
        for progressive in [false, true] {
            let mut src = Vec::new();
            let mut enc = jpeg_encoder::Encoder::new(&mut src, 80);
            enc.set_sampling_factor(jpeg_encoder::SamplingFactor::F_2_2);
            enc.set_optimized_huffman_tables(true);
            enc.set_progressive(progressive);
            enc.encode(img.as_raw(), 400, 300, jpeg_encoder::ColorType::Rgb).unwrap();
            let hints = SourceHints { jpeg: read_jpeg_profile(&src), ..Default::default() };
            let decoded = image::load_from_memory(&src).unwrap();
            let out = encode(&decoded, OutputFormat::Jpeg, &hints).unwrap();
            assert!(
                out.len() <= src.len() * 101 / 100,
                "progressive={progressive}: re-encoded {} bytes vs source {} bytes",
                out.len(),
                src.len()
            );
        }
    }

    #[test]
    fn jpeg_without_source_profile_uses_420_subsampling() {
        let img = DynamicImage::ImageRgb8(textured(64, 48));
        let out = encode(&img, OutputFormat::Jpeg, &SourceHints::default()).unwrap();
        assert_eq!(read_jpeg_profile(&out).unwrap().sampling, (2, 2));
    }

    #[test]
    fn non_jpeg_bytes_have_no_profile() {
        let img = DynamicImage::ImageRgb8(textured(8, 8));
        assert!(read_jpeg_profile(&encode(&img, OutputFormat::Png, &SourceHints::default()).unwrap()).is_none());
    }

    /// Indexed PNG with a 16-colour palette, like many AI or web images.
    fn palette_png(w: u32, h: u32) -> Vec<u8> {
        let palette: Vec<u8> = (0..16u8).flat_map(|i| [i * 16, 255 - i * 16, i.wrapping_mul(37)]).collect();
        let data: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| ((x / 4 + y / 4) % 16) as u8)).collect();
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(palette);
        enc.write_header().unwrap().write_image_data(&data).unwrap();
        out
    }

    fn png_color_type(bytes: &[u8]) -> u8 {
        bytes[25] // IHDR colour type
    }

    #[test]
    fn reads_palette_from_indexed_png() {
        let hints = read_source_hints(&palette_png(32, 32));
        assert_eq!(hints.png_palette.expect("palette").colors.len(), 16);
        assert!(hints.jpeg.is_none());
    }

    #[test]
    fn indexed_source_is_written_back_indexed_and_lossless() {
        let src = palette_png(256, 256);
        let hints = read_source_hints(&src);
        let decoded = image::load_from_memory(&src).unwrap();
        let out = encode(&decoded, OutputFormat::Png, &hints).unwrap();
        assert_eq!(png_color_type(&out), 3, "expected an indexed PNG");
        assert_eq!(image::load_from_memory(&out).unwrap().to_rgba8(), decoded.to_rgba8());
        assert!(out.len() <= src.len() * 11 / 10, "{} bytes vs source {}", out.len(), src.len());
    }

    #[test]
    fn off_palette_pixel_maps_to_nearest_entry() {
        let src = palette_png(8, 8);
        let hints = read_source_hints(&src);
        let mut img = image::load_from_memory(&src).unwrap().to_rgba8();
        let target = *img.get_pixel(0, 0);
        img.put_pixel(1, 1, image::Rgba([target[0] + 3, target[1].saturating_sub(2), target[2], 255]));
        let out = encode(&DynamicImage::ImageRgba8(img), OutputFormat::Png, &hints).unwrap();
        assert_eq!(image::load_from_memory(&out).unwrap().to_rgba8().get_pixel(1, 1), &target);
    }

    fn fake_icc() -> Vec<u8> {
        (0..300u32).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn icc_profile_survives_jpeg_reencode() {
        let img = textured(64, 48);
        let mut src = Vec::new();
        let mut enc = jpeg_encoder::Encoder::new(&mut src, 85);
        enc.add_icc_profile(&fake_icc()).unwrap();
        enc.encode(img.as_raw(), 64, 48, jpeg_encoder::ColorType::Rgb).unwrap();
        let hints = read_source_hints(&src);
        assert_eq!(hints.icc_profile.as_deref(), Some(fake_icc().as_slice()));
        let out = encode(&image::load_from_memory(&src).unwrap(), OutputFormat::Jpeg, &hints).unwrap();
        assert_eq!(read_source_hints(&out).icc_profile, Some(fake_icc()));
    }

    #[test]
    fn icc_profile_survives_png_reencode_truecolour_and_indexed() {
        let truecolour = DynamicImage::ImageRgb8(textured(16, 16));
        let hints = SourceHints { icc_profile: Some(fake_icc()), ..Default::default() };
        let out = encode(&truecolour, OutputFormat::Png, &hints).unwrap();
        assert_eq!(read_source_hints(&out).icc_profile, Some(fake_icc()));

        let indexed_src = palette_png(16, 16);
        let mut hints = read_source_hints(&indexed_src);
        hints.icc_profile = Some(fake_icc());
        let out = encode(&image::load_from_memory(&indexed_src).unwrap(), OutputFormat::Png, &hints).unwrap();
        assert_eq!(read_source_hints(&out).icc_profile, Some(fake_icc()));
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
