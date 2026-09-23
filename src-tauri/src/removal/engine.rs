use super::telea;
use image::{GrayImage, RgbImage};

pub enum Engine {
    Telea { radius: u32 },
    #[cfg(test)]
    Fill([u8; 3]),
}

impl Engine {
    /// Returns an image the size of `crop` with the HOLE pixels of `mask` filled.
    pub async fn inpaint(&self, crop: RgbImage, mask: GrayImage) -> Result<RgbImage, String> {
        match self {
            Engine::Telea { radius } => {
                let radius = *radius;
                tokio::task::spawn_blocking(move || telea::inpaint(&crop, &mask, radius))
                    .await
                    .map_err(|e| format!("Inpainting task failed: {}", e))?
            }
            #[cfg(test)]
            Engine::Fill(c) => Ok(RgbImage::from_fn(crop.width(), crop.height(), |x, y| {
                if mask.get_pixel(x, y)[0] == super::mask::HOLE { image::Rgb(*c) } else { *crop.get_pixel(x, y) }
            })),
        }
    }
}
