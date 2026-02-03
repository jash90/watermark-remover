use image::GenericImageView;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WatermarkRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemovalOptions {
    #[serde(default = "default_lossless")]
    pub lossless: bool,
}

fn default_lossless() -> bool {
    false
}

impl Default for RemovalOptions {
    fn default() -> Self {
        Self {
            lossless: default_lossless(),
        }
    }
}

pub fn get_image_dimensions(image_path: &str) -> Result<(u32, u32), String> {
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to load image: {}", e))?;

    Ok(img.dimensions())
}
