mod commands;
pub mod image_processor;
pub mod removal;
pub mod upscale;

use commands::{
    // Image commands
    cleanup_temp_files,
    get_image_info,
    load_image_base64,
    cancel_upscale,
    remove_watermark,
    save_processed_image,
    upscale_capabilities,
    upscale_image,
    UpscaleState,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(UpscaleState::default())
        .invoke_handler(tauri::generate_handler![
            // Image commands
            remove_watermark,
            get_image_info,
            load_image_base64,
            save_processed_image,
            cleanup_temp_files,
            // Upscaling
            upscale_image,
            cancel_upscale,
            upscale_capabilities,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
