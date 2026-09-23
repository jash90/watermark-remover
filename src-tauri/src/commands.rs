use crate::image_processor::{self, RemovalOptions, WatermarkRegion};
use crate::removal::codec;
use crate::removal::engine::Engine;
use crate::removal::pipeline::{self, PipelineOptions};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use crate::upscale::{self, models::{self, UpscaleModel}, Scale};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};
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

    // Decoding, inpainting and encoding are CPU-heavy: keep them off the async runtime.
    let path = image_path.clone();
    let (final_bytes, format) = tauri::async_runtime::spawn_blocking(move || {
        let original = codec::load_oriented(&path)?;
        let pipeline_options = PipelineOptions { refine_mask: options.refine_mask, grow: 2, feather: 3 };
        let output = pipeline::run(&original, &region, &pipeline_options, &Engine::Telea { radius: 5 })?;

        let original_extension = PathBuf::from(&path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_string();
        let format = codec::output_format(&original_extension, options.lossless);
        // Store the result the way the source was stored (JPEG tables, PNG palette,
        // colour profile) so the file doesn't grow and colours don't shift.
        let hints = fs::read(&path).map(|b| codec::read_source_hints(&b)).unwrap_or_default();
        Ok::<_, String>((codec::encode(&output.image, format, &hints)?, format))
    })
    .await
    .map_err(|e| format!("Processing task failed: {}", e))??;
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

/// Shared cancel flag for the running upscale.
#[derive(Default)]
pub struct UpscaleState {
    pub cancel: Arc<AtomicBool>,
}

#[derive(Serialize)]
pub struct UpscaleCapabilities {
    pub ai: bool,
}

#[tauri::command]
pub fn upscale_capabilities() -> UpscaleCapabilities {
    UpscaleCapabilities { ai: cfg!(ai_upscale) }
}

#[tauri::command]
pub fn cancel_upscale(state: tauri::State<'_, UpscaleState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Bundled models resolve from the app resources; downloadable ones are fetched
/// once into app data and verified against their pinned SHA-256.
fn model_path(app: &tauri::AppHandle, model: UpscaleModel) -> Result<Option<PathBuf>, String> {
    if !cfg!(ai_upscale) {
        return Ok(None);
    }
    let info = models::info(model);
    let path = match info.url {
        None => app
            .path()
            .resolve(format!("models/{}", info.file), tauri::path::BaseDirectory::Resource)
            .map_err(|e| e.to_string())?,
        Some(url) => {
            let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("models");
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(info.file);
            if models::verify(&path, info.sha256).is_err() {
                let handle = app.clone();
                models::download(url, &path, info.sha256, &mut |done, total| {
                    let _ = handle.emit("model-download", serde_json::json!({ "done": done, "total": total }));
                })?;
            }
            path
        }
    };
    models::verify(&path, info.sha256)?;
    Ok(Some(path))
}

#[tauri::command]
pub async fn upscale_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, UpscaleState>,
    image_path: String,
    scale: Scale,
    model: UpscaleModel,
    lossless: bool,
) -> Result<ProcessResult, String> {
    let original_size = fs::metadata(&image_path)
        .map_err(|e| format!("Failed to read file: {}", e))?
        .len();
    let cancel = state.cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    let handle = app.clone();
    let (bytes, format, preview) = tauri::async_runtime::spawn_blocking(move || {
        let path = model_path(&handle, model)?;
        let original = codec::load_oriented(&image_path)?;
        let out = upscale::run(&original, scale, path.as_deref(), &cancel, &mut |p| {
            let _ = handle.emit("upscale-progress", p);
        })?;
        let ext = PathBuf::from(&image_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_string();
        let format = codec::output_format(&ext, lossless);
        let mut hints = fs::read(&image_path).map(|b| codec::read_source_hints(&b)).unwrap_or_default();
        hints.png_palette = None; // AI detail doesn't fit a palette
        let bytes = codec::encode(&out, format, &hints)?;
        Ok::<_, String>((bytes, format, codec::preview_data_url(&out)?))
    })
    .await
    .map_err(|e| format!("Upscaling task failed: {}", e))??;

    let output_path = get_temp_dir()?.join(format!(
        "upscaled_{}_{}.{}",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("img"),
        chrono_lite_timestamp(),
        format.extension()
    ));
    fs::write(&output_path, &bytes).map_err(|e| format!("Failed to save upscaled image: {}", e))?;
    Ok(ProcessResult {
        output_path: output_path.to_string_lossy().to_string(),
        base64_preview: Some(preview),
        original_size,
        processed_size: bytes.len() as u64,
    })
}
