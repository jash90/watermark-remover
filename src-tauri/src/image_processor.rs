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
    #[serde(default)]
    pub lossless: bool,
    #[serde(default = "default_refine_mask")]
    pub refine_mask: bool,
}

fn default_refine_mask() -> bool {
    true
}

impl Default for RemovalOptions {
    fn default() -> Self {
        Self { lossless: false, refine_mask: default_refine_mask() }
    }
}

pub fn get_image_dimensions(image_path: &str) -> Result<(u32, u32), String> {
    Ok(crate::removal::codec::load_oriented(image_path)?.dimensions())
}
