use crate::image_processor::{self, RemovalOptions, WatermarkRegion};
use crate::removal::codec;
use crate::removal::engine::Engine;
use crate::removal::pipeline::{self, PipelineOptions};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessResult {
    pub output_path: String,
    pub base64_preview: Option<String>,
    pub original_size: u64,
    pub processed_size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub path: String,
}

/// Get temporary directory for storing processed images
fn get_temp_dir() -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir().join("watermark-remover");
    if !temp_dir.exists() {
        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp directory: {}", e))?;
    }
    Ok(temp_dir)
}

/// Simple timestamp function to avoid adding chrono dependency
fn chrono_lite_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[tauri::command]
pub async fn remove_watermark(
    image_path: String,
    region: WatermarkRegion,
    options: Option<RemovalOptions>,
) -> Result<ProcessResult, String> {
    let options = options.unwrap_or_default();
    let original_size = fs::metadata(&image_path)
        .map_err(|e| format!("Failed to get original file size: {}", e))?
        .len();

    let engine = Engine::Telea { radius: 5 };

    let original = codec::load_oriented(&image_path)?;
    let pipeline_options = PipelineOptions { refine_mask: options.refine_mask, grow: 2, feather: 3 };
    let output = pipeline::run(&original, &region, &pipeline_options, &engine).await?;

    let original_extension = PathBuf::from(&image_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_string();
    let format = codec::output_format(&original_extension, options.lossless);
    // Reuse the source JPEG's tables and subsampling so the file doesn't grow.
    let source_profile = fs::read(&image_path).ok().and_then(|b| codec::read_jpeg_profile(&b));
    let final_bytes = codec::encode(&output.image, format, source_profile.as_ref())?;
    let processed_size = final_bytes.len() as u64;

    let output_filename = format!(
        "processed_{}_{}.{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("img"),
        chrono_lite_timestamp(),
        format.extension()
    );
    let output_path = get_temp_dir()?.join(&output_filename);
    fs::write(&output_path, &final_bytes)
        .map_err(|e| format!("Failed to save processed image: {}", e))?;

    Ok(ProcessResult {
        output_path: output_path.to_string_lossy().to_string(),
        base64_preview: Some(format!("data:{};base64,{}", format.mime(), BASE64.encode(&final_bytes))),
        original_size,
        processed_size,
    })
}

#[tauri::command]
pub async fn get_image_info(image_path: String) -> Result<ImageInfo, String> {
    let (width, height) = image_processor::get_image_dimensions(&image_path)?;
    Ok(ImageInfo {
        width,
        height,
        path: image_path,
    })
}

#[tauri::command]
pub async fn load_image_base64(image_path: String) -> Result<String, String> {
    let path = PathBuf::from(&image_path);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");

    let bytes = fs::read(&image_path)
        .map_err(|e| format!("Failed to read image: {}", e))?;
    let base64_data = BASE64.encode(&bytes);

    let mime_type = match extension.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };

    Ok(format!("data:{};base64,{}", mime_type, base64_data))
}

#[tauri::command]
pub async fn save_processed_image(source_path: String, destination_path: String) -> Result<(), String> {
    fs::copy(&source_path, &destination_path)
        .map_err(|e| format!("Failed to save image: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn cleanup_temp_files() -> Result<(), String> {
    let temp_dir = get_temp_dir()?;
    if temp_dir.exists() {
        for entry in fs::read_dir(&temp_dir)
            .map_err(|e| format!("Failed to read temp dir: {}", e))?
            .flatten()
        {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}
