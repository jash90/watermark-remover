fn main() {
    // OpenCV linking for macOS
    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-search=/opt/homebrew/opt/opencv/lib");
        println!("cargo:rustc-link-lib=opencv_core");
        println!("cargo:rustc-link-lib=opencv_imgproc");
        println!("cargo:rustc-link-lib=opencv_imgcodecs");
        println!("cargo:rustc-link-lib=opencv_photo");
    }

    tauri_build::build()
}
