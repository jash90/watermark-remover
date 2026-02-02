use opencv::{
    core::{Mat, MatTraitConst, Size, CV_8UC1, BORDER_CONSTANT, Scalar},
    imgcodecs,
    imgproc,
    photo,
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WatermarkRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemovalOptions {
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    #[serde(default = "default_dilate_pixels")]
    pub dilate_pixels: i32,
    #[serde(default = "default_inpaint_radius")]
    pub inpaint_radius: f64,
    #[serde(default = "default_lossless")]
    pub lossless: bool,
}

fn default_algorithm() -> String {
    "telea".to_string()
}

fn default_dilate_pixels() -> i32 {
    3
}

fn default_inpaint_radius() -> f64 {
    5.0
}

fn default_lossless() -> bool {
    false
}

impl Default for RemovalOptions {
    fn default() -> Self {
        Self {
            algorithm: default_algorithm(),
            dilate_pixels: default_dilate_pixels(),
            inpaint_radius: default_inpaint_radius(),
            lossless: default_lossless(),
        }
    }
}

pub fn remove_watermark(
    image_path: &str,
    region: &WatermarkRegion,
    options: &RemovalOptions,
    output_path: &str,
) -> Result<(), String> {
    // Load the image
    let img = imgcodecs::imread(image_path, imgcodecs::IMREAD_COLOR)
        .map_err(|e| format!("Failed to load image: {}", e))?;

    if img.empty() {
        return Err("Failed to load image: empty image".to_string());
    }

    let img_width = img.cols();
    let img_height = img.rows();

    // Validate region bounds
    if region.x < 0 || region.y < 0 || region.width <= 0 || region.height <= 0 {
        return Err("Invalid region dimensions".to_string());
    }

    if region.x + region.width > img_width || region.y + region.height > img_height {
        return Err(format!(
            "Region exceeds image bounds. Image: {}x{}, Region: ({}, {}) + {}x{}",
            img_width, img_height, region.x, region.y, region.width, region.height
        ));
    }

    // Create binary mask for the watermark region
    let mut mask = Mat::zeros(img_height, img_width, CV_8UC1)
        .map_err(|e| format!("Failed to create mask: {}", e))?
        .to_mat()
        .map_err(|e| format!("Failed to convert mask: {}", e))?;

    // Fill the region with white (255) in the mask
    for y in region.y..(region.y + region.height) {
        for x in region.x..(region.x + region.width) {
            *mask.at_2d_mut::<u8>(y, x)
                .map_err(|e| format!("Failed to set mask pixel: {}", e))? = 255;
        }
    }

    // Dilate the mask for better edge blending
    if options.dilate_pixels > 0 {
        let kernel_size = options.dilate_pixels * 2 + 1;
        let kernel = imgproc::get_structuring_element(
            imgproc::MORPH_ELLIPSE,
            Size::new(kernel_size, kernel_size),
            opencv::core::Point::new(-1, -1),
        )
        .map_err(|e| format!("Failed to create kernel: {}", e))?;

        let mut dilated_mask = Mat::default();
        imgproc::dilate(
            &mask,
            &mut dilated_mask,
            &kernel,
            opencv::core::Point::new(-1, -1),
            1,
            BORDER_CONSTANT,
            Scalar::all(0.0),
        )
        .map_err(|e| format!("Failed to dilate mask: {}", e))?;

        mask = dilated_mask;
    }

    // Select inpainting algorithm
    let inpaint_method = match options.algorithm.to_lowercase().as_str() {
        "navier_stokes" | "ns" => photo::INPAINT_NS,
        _ => photo::INPAINT_TELEA, // Default to Telea
    };

    // Perform inpainting
    let mut result = Mat::default();
    photo::inpaint(
        &img,
        &mask,
        &mut result,
        options.inpaint_radius,
        inpaint_method,
    )
    .map_err(|e| format!("Inpainting failed: {}", e))?;

    // Save the result
    let output_path_obj = Path::new(output_path);
    let extension = output_path_obj
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();

    let (final_output_path, params) = if options.lossless {
        // Lossless mode
        match extension.as_str() {
            "jpg" | "jpeg" => {
                // JPEG cannot be lossless - convert to PNG
                let png_path = output_path.replace(".jpg", ".png").replace(".jpeg", ".png");
                (png_path, opencv::core::Vector::<i32>::from_slice(&[
                    imgcodecs::IMWRITE_PNG_COMPRESSION, 9
                ]))
            }
            "png" => {
                (output_path.to_string(), opencv::core::Vector::<i32>::from_slice(&[
                    imgcodecs::IMWRITE_PNG_COMPRESSION, 9
                ]))
            }
            "webp" => {
                // WebP lossless: quality > 100 = lossless
                (output_path.to_string(), opencv::core::Vector::<i32>::from_slice(&[
                    imgcodecs::IMWRITE_WEBP_QUALITY, 101
                ]))
            }
            _ => (output_path.to_string(), opencv::core::Vector::<i32>::new()),
        }
    } else {
        // Lossy mode (current behavior)
        let params = match extension.as_str() {
            "jpg" | "jpeg" => {
                opencv::core::Vector::<i32>::from_slice(&[imgcodecs::IMWRITE_JPEG_QUALITY, 95])
            }
            "png" => {
                opencv::core::Vector::<i32>::from_slice(&[imgcodecs::IMWRITE_PNG_COMPRESSION, 6])
            }
            "webp" => {
                opencv::core::Vector::<i32>::from_slice(&[imgcodecs::IMWRITE_WEBP_QUALITY, 95])
            }
            _ => opencv::core::Vector::<i32>::new(),
        };
        (output_path.to_string(), params)
    };

    imgcodecs::imwrite(&final_output_path, &result, &params)
        .map_err(|e| format!("Failed to save result: {}", e))?;

    Ok(())
}

pub fn get_image_dimensions(image_path: &str) -> Result<(i32, i32), String> {
    let img = imgcodecs::imread(image_path, imgcodecs::IMREAD_COLOR)
        .map_err(|e| format!("Failed to load image: {}", e))?;

    if img.empty() {
        return Err("Failed to load image: empty image".to_string());
    }

    Ok((img.cols(), img.rows()))
}
