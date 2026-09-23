fn main() {
    println!("cargo::rustc-check-cfg=cfg(ai_upscale)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // ONNX Runtime ships no x86_64 macOS binaries (ORT 1.30, ort 2.0.0-rc.13).
    if !(os == "macos" && arch == "x86_64") {
        println!("cargo:rustc-cfg=ai_upscale");
    }
    tauri_build::build()
}
