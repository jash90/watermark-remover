use crate::gemini_client::GeminiClient;
use crate::image_processor::{self, RemovalOptions, WatermarkRegion};
use crate::video_processor::{self, VideoInfo, VideoProgress, VideoProcessResult};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use opencv::prelude::{MatTraitConst, VectorToVec};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri_plugin_store::StoreExt;
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
    pub width: i32,
    pub height: i32,
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

#[tauri::command]
pub async fn remove_watermark(
    image_path: String,
    region: WatermarkRegion,
    options: Option<RemovalOptions>,
) -> Result<ProcessResult, String> {
    let options = options.unwrap_or_default();

    // Get original file size
    let original_size = fs::metadata(&image_path)
        .map_err(|e| format!("Failed to get original file size: {}", e))?
        .len();

    // Generate unique output filename
    let input_path = PathBuf::from(&image_path);
    let extension = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let output_filename = format!("processed_{}_{}.{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("img"),
        chrono_lite_timestamp(),
        extension
    );

    let temp_dir = get_temp_dir()?;
    let output_path = temp_dir.join(&output_filename);
    let output_path_str = output_path.to_string_lossy().to_string();

    // Process the image
    image_processor::remove_watermark(&image_path, &region, &options, &output_path_str)?;

    // Read the result and encode as base64 for preview
    let result_bytes = fs::read(&output_path)
        .map_err(|e| format!("Failed to read processed image: {}", e))?;
    let processed_size = result_bytes.len() as u64;
    let base64_preview = BASE64.encode(&result_bytes);

    // Determine MIME type
    let mime_type = match extension.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };

    Ok(ProcessResult {
        output_path: output_path_str,
        base64_preview: Some(format!("data:{};base64,{}", mime_type, base64_preview)),
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
        for entry in fs::read_dir(&temp_dir).map_err(|e| format!("Failed to read temp dir: {}", e))? {
            if let Ok(entry) = entry {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    Ok(())
}

/// Simple timestamp function to avoid adding chrono dependency
fn chrono_lite_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================
// Video Processing Commands
// ============================================

#[tauri::command]
pub async fn get_video_info(video_path: String) -> Result<VideoInfo, String> {
    video_processor::get_video_info(&video_path)
}

#[tauri::command]
pub async fn extract_video_frame(video_path: String, output_path: String) -> Result<String, String> {
    // Generate output path if not provided
    let output = if output_path.is_empty() {
        let temp_dir = get_temp_dir()?;
        let output_filename = format!("frame_{}_{}.png",
            Uuid::new_v4().to_string().split('-').next().unwrap_or("img"),
            chrono_lite_timestamp()
        );
        temp_dir.join(&output_filename).to_string_lossy().to_string()
    } else {
        output_path
    };

    video_processor::extract_first_frame(&video_path, &output)?;

    // Read the result and encode as base64 for preview
    let result_bytes = fs::read(&output)
        .map_err(|e| format!("Failed to read frame image: {}", e))?;
    let base64_data = BASE64.encode(&result_bytes);

    Ok(format!("data:image/png;base64,{}", base64_data))
}

#[tauri::command]
pub async fn process_video(
    video_path: String,
    region: WatermarkRegion,
    options: Option<RemovalOptions>,
) -> Result<VideoProcessResult, String> {
    let options = options.unwrap_or_default();

    // Generate unique output filename
    let output_filename = format!("processed_{}_{}.mp4",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("vid"),
        chrono_lite_timestamp()
    );

    let temp_dir = get_temp_dir()?;
    let output_path = temp_dir.join(&output_filename);
    let output_path_str = output_path.to_string_lossy().to_string();

    // Process the video (this can take a long time)
    video_processor::process_video(&video_path, &output_path_str, &region, &options)
}

#[tauri::command]
pub async fn get_video_progress() -> Result<VideoProgress, String> {
    Ok(video_processor::get_progress())
}

#[tauri::command]
pub async fn cancel_video_processing() -> Result<(), String> {
    video_processor::request_cancel();
    Ok(())
}

// ============================================
// Gemini Cloud Processing Commands
// ============================================

const STORE_FILENAME: &str = "settings.json";
const API_KEY_SETTING: &str = "gemini_api_key";

/// Re-encode image bytes with lossless compression
fn reencode_lossless(bytes: &[u8], original_ext: &str) -> Result<(Vec<u8>, String), String> {
    use opencv::{imgcodecs, core::Vector};

    // Decode image from bytes
    let buf = Vector::<u8>::from_slice(bytes);
    let img = imgcodecs::imdecode(&buf, imgcodecs::IMREAD_COLOR)
        .map_err(|e| format!("Failed to decode image for lossless re-encoding: {}", e))?;

    if img.empty() {
        return Err("Failed to decode image: empty result".to_string());
    }

    let (params, ext) = match original_ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => {
            // JPEG doesn't support lossless - convert to PNG
            (Vector::<i32>::from_slice(&[imgcodecs::IMWRITE_PNG_COMPRESSION, 9]), "png")
        }
        "webp" => {
            // WebP lossless mode (quality > 100 enables lossless)
            (Vector::<i32>::from_slice(&[imgcodecs::IMWRITE_WEBP_QUALITY, 101]), "webp")
        }
        _ => {
            // PNG and others - use PNG lossless
            (Vector::<i32>::from_slice(&[imgcodecs::IMWRITE_PNG_COMPRESSION, 9]), "png")
        }
    };

    let mut output_buf = Vector::<u8>::new();
    imgcodecs::imencode(&format!(".{}", ext), &img, &mut output_buf, &params)
        .map_err(|e| format!("Failed to encode lossless image: {}", e))?;

    Ok((output_buf.to_vec(), ext.to_string()))
}

#[tauri::command]
pub async fn remove_watermark_cloud(
    app: tauri::AppHandle,
    image_path: String,
    region: WatermarkRegion,
    options: Option<RemovalOptions>,
) -> Result<ProcessResult, String> {
    let options = options.unwrap_or_default();

    // Get original file size
    let original_size = fs::metadata(&image_path)
        .map_err(|e| format!("Failed to get original file size: {}", e))?
        .len();

    // Get API key from store
    let api_key = get_stored_api_key(&app)?;

    if api_key.is_empty() {
        return Err("Gemini API key not configured. Please set it in Settings.".to_string());
    }

    let client = GeminiClient::new(api_key);

    // Get original extension
    let input_path = PathBuf::from(&image_path);
    let original_extension = input_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");

    // Call Gemini API
    let processed_bytes = client
        .remove_watermark(&image_path, &region)
        .await?;

    // Apply lossless re-encoding if requested
    let (final_bytes, final_extension) = if options.lossless {
        reencode_lossless(&processed_bytes, original_extension)?
    } else {
        (processed_bytes, original_extension.to_string())
    };

    let processed_size = final_bytes.len() as u64;

    // Generate unique output filename with final extension
    let output_filename = format!(
        "cloud_processed_{}_{}.{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("img"),
        chrono_lite_timestamp(),
        final_extension
    );

    let temp_dir = get_temp_dir()?;
    let output_path = temp_dir.join(&output_filename);
    let output_path_str = output_path.to_string_lossy().to_string();

    // Save the result
    fs::write(&output_path, &final_bytes)
        .map_err(|e| format!("Failed to save processed image: {}", e))?;

    // Encode as base64 for preview
    let base64_preview = BASE64.encode(&final_bytes);

    // Determine MIME type based on final extension
    let mime_type = match final_extension.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    };

    Ok(ProcessResult {
        output_path: output_path_str,
        base64_preview: Some(format!("data:{};base64,{}", mime_type, base64_preview)),
        original_size,
        processed_size,
    })
}

#[tauri::command]
pub async fn set_gemini_api_key(app: tauri::AppHandle, api_key: String) -> Result<(), String> {
    let store = app
        .store(STORE_FILENAME)
        .map_err(|e| format!("Failed to access store: {}", e))?;

    store
        .set(API_KEY_SETTING, serde_json::json!(api_key));

    store
        .save()
        .map_err(|e| format!("Failed to save store: {}", e))?;

    Ok(())
}

#[tauri::command]
pub async fn get_gemini_api_key(app: tauri::AppHandle) -> Result<String, String> {
    get_stored_api_key(&app)
}

#[tauri::command]
pub async fn test_gemini_connection(app: tauri::AppHandle) -> Result<bool, String> {
    let api_key = get_stored_api_key(&app)?;

    if api_key.is_empty() {
        return Err("API key not configured".to_string());
    }

    let client = GeminiClient::new(api_key);
    client.test_connection().await
}

fn get_stored_api_key(app: &tauri::AppHandle) -> Result<String, String> {
    let store = app
        .store(STORE_FILENAME)
        .map_err(|e| format!("Failed to access store: {}", e))?;

    let api_key = store
        .get(API_KEY_SETTING)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    Ok(api_key)
}

#[tauri::command]
pub async fn list_gemini_models(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let api_key = get_stored_api_key(&app)?;

    if api_key.is_empty() {
        return Err("API key not configured".to_string());
    }

    let client = GeminiClient::new(api_key);
    client.list_models().await
}
