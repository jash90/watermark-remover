use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpscaleModel {
    Fast,
    Quality,
}

pub struct ModelInfo {
    pub file: &'static str,
    pub sha256: &'static str,
    /// None = bundled with the app.
    pub url: Option<&'static str>,
    pub bytes: u64,
}

/// Real-ESRGAN (BSD-3-Clause) exported by scripts/export_upscalers.py.
pub fn info(m: UpscaleModel) -> ModelInfo {
    match m {
        UpscaleModel::Fast => ModelInfo {
            file: "realesr-general-x4v3.onnx",
            sha256: "f383396dbc9d58a03f76da2fadd9e870a63b22895b2a58293cdbc47528641444",
            url: None,
            bytes: 4_873_231,
        },
        UpscaleModel::Quality => ModelInfo {
            file: "RealESRGAN_x4plus.onnx",
            sha256: "d7a49b1b74a15cfbf88e59dd2389c07816a3c3d269d8247bdbbf49ebf1e932b0",
            url: Some("https://github.com/jash90/watermark-remover/releases/download/models-v1/RealESRGAN_x4plus.onnx"),
            bytes: 67_028_583,
        },
    }
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Cannot read model: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("Cannot read model: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn verify(path: &Path, sha256: &str) -> Result<(), String> {
    let actual = sha256_of(path)?;
    if actual != sha256 {
        return Err(format!("Model checksum mismatch (expected {sha256}, got {actual})"));
    }
    Ok(())
}

/// Downloads to `<dest>.part`, verifies, then renames; a bad file never reaches `dest`.
pub fn download(url: &str, dest: &Path, sha256: &str, on_bytes: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
    let part = dest.with_extension("part");
    let mut response = ureq::get(url).call().map_err(|e| format!("Download failed: {e}"))?;
    let total = response.body().content_length().unwrap_or(0);
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| format!("Cannot write model: {e}"))?;
    let (mut buf, mut done) = (vec![0u8; 1 << 16], 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("Cannot write model: {e}"))?;
        done += n as u64;
        on_bytes(done, total);
    }
    drop(file);
    if let Err(e) = verify(&part, sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, dest).map_err(|e| format!("Cannot store model: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fast_model_matches_pinned_checksum() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources/models")
            .join(info(UpscaleModel::Fast).file);
        verify(&path, info(UpscaleModel::Fast).sha256).unwrap();
    }

    /// Serves `body` once over HTTP on a random local port; returns the URL.
    fn serve_once(body: &'static [u8]) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.onnx", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = [0u8; 1024];
            let _ = stream.read(&mut req);
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        url
    }

    fn sha_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn download_verifies_and_stores_the_model() {
        let dest = std::env::temp_dir().join("wmr_dl_ok.onnx");
        let _ = std::fs::remove_file(&dest);
        let mut progress = Vec::new();
        download(&serve_once(b"model-bytes"), &dest, &sha_hex(b"model-bytes"), &mut |d, t| progress.push((d, t))).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"model-bytes");
        assert_eq!(progress.last(), Some(&(11, 11)));
    }

    #[test]
    fn tampered_download_is_rejected_and_removed() {
        let dest = std::env::temp_dir().join("wmr_dl_bad.onnx");
        let _ = std::fs::remove_file(&dest);
        let err = download(&serve_once(b"evil-bytes"), &dest, &sha_hex(b"model-bytes"), &mut |_, _| {}).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
        assert!(!dest.exists(), "bad model must not reach its final path");
        assert!(!dest.with_extension("part").exists(), "partial file must be cleaned up");
    }

    #[test]
    fn rejects_model_with_wrong_checksum() {
        let path = std::env::temp_dir().join("wmr_bad_model.onnx");
        std::fs::write(&path, b"not a model").unwrap();
        let err = verify(&path, info(UpscaleModel::Fast).sha256).unwrap_err();
        assert!(err.contains("checksum"), "{err}");
    }
}
