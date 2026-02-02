mod commands;
mod gemini_client;
mod image_processor;
mod video_processor;

use commands::{
    // Image commands
    cleanup_temp_files,
    get_image_info,
    load_image_base64,
    remove_watermark,
    save_processed_image,
    // Video commands
    cancel_video_processing,
    extract_video_frame,
    get_video_info,
    get_video_progress,
    process_video,
    // Gemini cloud commands
    get_gemini_api_key,
    list_gemini_models,
    remove_watermark_cloud,
    set_gemini_api_key,
    test_gemini_connection,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            // Image commands
            remove_watermark,
            get_image_info,
            load_image_base64,
            save_processed_image,
            cleanup_temp_files,
            // Video commands
            get_video_info,
            extract_video_frame,
            process_video,
            get_video_progress,
            cancel_video_processing,
            // Gemini cloud commands
            remove_watermark_cloud,
            set_gemini_api_key,
            get_gemini_api_key,
            test_gemini_connection,
            list_gemini_models,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
