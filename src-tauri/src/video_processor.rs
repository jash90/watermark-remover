use opencv::{
    core::{Mat, MatTraitConst, Size, CV_8UC1, BORDER_CONSTANT, Scalar},
    imgcodecs,
    imgproc,
    photo,
    prelude::*,
    videoio::{self, VideoCapture, VideoWriter, CAP_ANY},
};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::image_processor::{RemovalOptions, WatermarkRegion};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoInfo {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub frame_count: i32,
    pub duration_secs: f64,
    pub codec: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoProgress {
    pub current_frame: u32,
    pub total_frames: u32,
    pub percent: f32,
    pub estimated_remaining_secs: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoProcessResult {
    pub output_path: String,
    pub frames_processed: u32,
    pub duration_secs: f64,
}

/// Global cancellation flag for video processing
static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);
static CURRENT_FRAME: AtomicU32 = AtomicU32::new(0);
static TOTAL_FRAMES: AtomicU32 = AtomicU32::new(0);

/// Request cancellation of current video processing
pub fn request_cancel() {
    CANCEL_FLAG.store(true, Ordering::SeqCst);
}

/// Reset cancellation flag
fn reset_cancel() {
    CANCEL_FLAG.store(false, Ordering::SeqCst);
}

/// Check if cancellation was requested
fn is_cancelled() -> bool {
    CANCEL_FLAG.load(Ordering::SeqCst)
}

/// Get current processing progress
pub fn get_progress() -> VideoProgress {
    let current = CURRENT_FRAME.load(Ordering::SeqCst);
    let total = TOTAL_FRAMES.load(Ordering::SeqCst);
    let percent = if total > 0 {
        (current as f32 / total as f32) * 100.0
    } else {
        0.0
    };
    VideoProgress {
        current_frame: current,
        total_frames: total,
        percent,
        estimated_remaining_secs: None,
    }
}

/// Get video information
pub fn get_video_info(video_path: &str) -> Result<VideoInfo, String> {
    let mut cap = VideoCapture::from_file(video_path, CAP_ANY)
        .map_err(|e| format!("Failed to open video: {}", e))?;

    if !cap.is_opened().map_err(|e| format!("Failed to check video: {}", e))? {
        return Err("Failed to open video file".to_string());
    }

    let width = cap.get(videoio::CAP_PROP_FRAME_WIDTH)
        .map_err(|e| format!("Failed to get width: {}", e))? as i32;
    let height = cap.get(videoio::CAP_PROP_FRAME_HEIGHT)
        .map_err(|e| format!("Failed to get height: {}", e))? as i32;
    let fps = cap.get(videoio::CAP_PROP_FPS)
        .map_err(|e| format!("Failed to get fps: {}", e))?;
    let frame_count = cap.get(videoio::CAP_PROP_FRAME_COUNT)
        .map_err(|e| format!("Failed to get frame count: {}", e))? as i32;
    let fourcc = cap.get(videoio::CAP_PROP_FOURCC)
        .map_err(|e| format!("Failed to get codec: {}", e))? as i32;

    // Convert fourcc to string
    let codec = fourcc_to_string(fourcc);
    let duration_secs = if fps > 0.0 { frame_count as f64 / fps } else { 0.0 };

    Ok(VideoInfo {
        width,
        height,
        fps,
        frame_count,
        duration_secs,
        codec,
        path: video_path.to_string(),
    })
}

/// Extract a single frame from video
pub fn extract_frame(video_path: &str, frame_number: i32) -> Result<Mat, String> {
    #[allow(unused_mut)]
    let mut cap = VideoCapture::from_file(video_path, CAP_ANY)
        .map_err(|e| format!("Failed to open video: {}", e))?;

    if !cap.is_opened().map_err(|e| format!("Failed to check video: {}", e))? {
        return Err("Failed to open video file".to_string());
    }

    cap.set(videoio::CAP_PROP_POS_FRAMES, frame_number as f64)
        .map_err(|e| format!("Failed to seek to frame: {}", e))?;

    let mut frame = Mat::default();
    cap.read(&mut frame)
        .map_err(|e| format!("Failed to read frame: {}", e))?;

    if frame.empty() {
        return Err("Failed to read frame: empty frame".to_string());
    }

    Ok(frame)
}

/// Extract first frame and save as image for preview
pub fn extract_first_frame(video_path: &str, output_path: &str) -> Result<(), String> {
    let frame = extract_frame(video_path, 0)?;

    let params = opencv::core::Vector::<i32>::from_slice(&[imgcodecs::IMWRITE_PNG_COMPRESSION, 6]);
    imgcodecs::imwrite(output_path, &frame, &params)
        .map_err(|e| format!("Failed to save frame: {}", e))?;

    Ok(())
}

/// Process video by removing watermark from each frame
pub fn process_video(
    input_path: &str,
    output_path: &str,
    region: &WatermarkRegion,
    options: &RemovalOptions,
) -> Result<VideoProcessResult, String> {
    reset_cancel();
    let start_time = std::time::Instant::now();

    // Get video info
    let info = get_video_info(input_path)?;
    TOTAL_FRAMES.store(info.frame_count as u32, Ordering::SeqCst);
    CURRENT_FRAME.store(0, Ordering::SeqCst);

    // Validate region bounds
    if region.x < 0 || region.y < 0 || region.width <= 0 || region.height <= 0 {
        return Err("Invalid region dimensions".to_string());
    }

    if region.x + region.width > info.width || region.y + region.height > info.height {
        return Err(format!(
            "Region exceeds video bounds. Video: {}x{}, Region: ({}, {}) + {}x{}",
            info.width, info.height, region.x, region.y, region.width, region.height
        ));
    }

    // Extract audio from source video (if exists)
    let temp_dir = std::env::temp_dir().join("watermark-remover");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

    let audio_path = temp_dir.join("audio_temp.aac");
    let video_without_audio_path = temp_dir.join("video_no_audio.mp4");
    let has_audio = extract_audio(input_path, audio_path.to_string_lossy().as_ref());

    // Open input video
    let mut cap = VideoCapture::from_file(input_path, CAP_ANY)
        .map_err(|e| format!("Failed to open video: {}", e))?;

    if !cap.is_opened().map_err(|e| format!("Failed to check video: {}", e))? {
        return Err("Failed to open video file".to_string());
    }

    // Determine output codec
    let fourcc = VideoWriter::fourcc('a', 'v', 'c', '1')
        .map_err(|e| format!("Failed to create fourcc: {}", e))?;

    // Create video writer
    let output_for_writer = if has_audio {
        video_without_audio_path.to_string_lossy().to_string()
    } else {
        output_path.to_string()
    };

    let mut writer = VideoWriter::new(
        &output_for_writer,
        fourcc,
        info.fps,
        Size::new(info.width, info.height),
        true,
    )
    .map_err(|e| format!("Failed to create video writer: {}", e))?;

    if !writer.is_opened().map_err(|e| format!("Failed to check writer: {}", e))? {
        return Err("Failed to open video writer".to_string());
    }

    // Pre-compute mask and kernel for efficiency
    let mask = create_mask(info.width, info.height, region)?;
    let dilated_mask = dilate_mask(&mask, options.dilate_pixels)?;

    // Select inpainting algorithm
    let inpaint_method = match options.algorithm.to_lowercase().as_str() {
        "navier_stokes" | "ns" => photo::INPAINT_NS,
        _ => photo::INPAINT_TELEA,
    };

    // Process frames
    let mut frame = Mat::default();
    let mut processed_frames = 0u32;

    loop {
        if is_cancelled() {
            // Cleanup on cancel
            let _ = std::fs::remove_file(&output_for_writer);
            if has_audio {
                let _ = std::fs::remove_file(&audio_path);
            }
            return Err("Video processing cancelled".to_string());
        }

        let success = cap.read(&mut frame)
            .map_err(|e| format!("Failed to read frame: {}", e))?;

        if !success || frame.empty() {
            break;
        }

        // Apply inpainting to this frame
        let mut result = Mat::default();
        photo::inpaint(
            &frame,
            &dilated_mask,
            &mut result,
            options.inpaint_radius,
            inpaint_method,
        )
        .map_err(|e| format!("Inpainting failed at frame {}: {}", processed_frames, e))?;

        // Write processed frame
        writer.write(&result)
            .map_err(|e| format!("Failed to write frame {}: {}", processed_frames, e))?;

        processed_frames += 1;
        CURRENT_FRAME.store(processed_frames, Ordering::SeqCst);
    }

    // Release resources
    drop(writer);
    drop(cap);

    // Merge audio back if it existed
    if has_audio {
        merge_audio(
            video_without_audio_path.to_string_lossy().as_ref(),
            audio_path.to_string_lossy().as_ref(),
            output_path,
        )?;
        // Cleanup temp files
        let _ = std::fs::remove_file(&video_without_audio_path);
        let _ = std::fs::remove_file(&audio_path);
    }

    let duration = start_time.elapsed().as_secs_f64();

    Ok(VideoProcessResult {
        output_path: output_path.to_string(),
        frames_processed: processed_frames,
        duration_secs: duration,
    })
}

/// Create binary mask for the watermark region
fn create_mask(width: i32, height: i32, region: &WatermarkRegion) -> Result<Mat, String> {
    let mut mask = Mat::zeros(height, width, CV_8UC1)
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

    Ok(mask)
}

/// Dilate the mask for better edge blending
fn dilate_mask(mask: &Mat, dilate_pixels: i32) -> Result<Mat, String> {
    if dilate_pixels <= 0 {
        return Ok(mask.clone());
    }

    let kernel_size = dilate_pixels * 2 + 1;
    let kernel = imgproc::get_structuring_element(
        imgproc::MORPH_ELLIPSE,
        Size::new(kernel_size, kernel_size),
        opencv::core::Point::new(-1, -1),
    )
    .map_err(|e| format!("Failed to create kernel: {}", e))?;

    let mut dilated_mask = Mat::default();
    imgproc::dilate(
        mask,
        &mut dilated_mask,
        &kernel,
        opencv::core::Point::new(-1, -1),
        1,
        BORDER_CONSTANT,
        Scalar::all(0.0),
    )
    .map_err(|e| format!("Failed to dilate mask: {}", e))?;

    Ok(dilated_mask)
}

/// Convert fourcc int to string
fn fourcc_to_string(fourcc: i32) -> String {
    let bytes = [
        (fourcc & 0xFF) as u8,
        ((fourcc >> 8) & 0xFF) as u8,
        ((fourcc >> 16) & 0xFF) as u8,
        ((fourcc >> 24) & 0xFF) as u8,
    ];
    String::from_utf8_lossy(&bytes).to_string()
}

/// Extract audio from video using FFmpeg
fn extract_audio(video_path: &str, audio_output_path: &str) -> bool {
    let result = Command::new("ffmpeg")
        .args([
            "-y",           // Overwrite output
            "-i", video_path,
            "-vn",          // No video
            "-acodec", "aac",
            "-b:a", "192k",
            audio_output_path,
        ])
        .output();

    match result {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

/// Merge video and audio using FFmpeg
fn merge_audio(video_path: &str, audio_path: &str, output_path: &str) -> Result<(), String> {
    let result = Command::new("ffmpeg")
        .args([
            "-y",           // Overwrite output
            "-i", video_path,
            "-i", audio_path,
            "-c:v", "copy",
            "-c:a", "aac",
            "-strict", "experimental",
            output_path,
        ])
        .output()
        .map_err(|e| format!("Failed to run FFmpeg: {}", e))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("FFmpeg merge failed: {}", stderr));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fourcc_to_string() {
        // Test common codecs
        let h264 = 0x34363248; // H264
        assert!(!fourcc_to_string(h264).is_empty());
    }
}
