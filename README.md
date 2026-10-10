# Watermark Remover

Desktop application for removing watermarks from images. Everything runs locally: no API key, no internet connection, and your images never leave your computer.

![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)
![License](https://img.shields.io/badge/license-MIT-green)
![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri-orange)

## Features

- **Local Removal** - Fast-marching inpainting in pure Rust; detects the watermark's own pixels inside your selection and leaves the rest untouched
- **Original Quality** - Output keeps the original resolution, transparency, and JPEG compression settings (no file-size growth)
- **Region Selection** - Select specific areas to remove with precision
- **Batch Processing** - Process multiple images at once with the same region
- **Before/After Preview** - Compare original and processed images side by side
- **Cross-Platform** - Available for Windows, macOS (Intel & Apple Silicon), and Linux
- **Lightweight** - Built with Tauri for minimal resource usage

## Screenshots

| Upload | Select Region | Preview |
|--------|---------------|---------|
| Drag & drop or browse for images | Draw rectangle around watermark | Compare before and after |

## Installation

Download the latest release for your platform:

- **Windows**: `.msi` or `.exe` installer
- **macOS**: `.dmg` (Universal - Intel & Apple Silicon)
- **Linux**: `.deb` or `.AppImage`

[Download Latest Release](https://github.com/jash90/watermark-remover/releases/latest)

## Usage

1. Launch the application
2. Drop an image or click to browse
3. Draw a rectangle around the watermark area
4. Click "Remove Watermark"
5. Preview the result and save

### Batch Processing

1. Select multiple images at once
2. Define the watermark region on the first image
3. The same region will be applied to all images
4. Process all images automatically

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) (LTS version)
- [Rust](https://rustup.rs/)
- [Tauri CLI](https://tauri.app/start/prerequisites/)

### Setup

```bash
# Clone the repository
git clone https://github.com/jash90/watermark-remover.git
cd watermark-remover

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Tech Stack

- **Frontend**: React, TypeScript, Vite
- **Backend**: Rust, Tauri
- **Image processing**: `image`, `imageproc`, `jpeg-encoder` (pure Rust, no OpenCV)
- **Styling**: CSS

## License

MIT License - see [LICENSE](LICENSE) for details.

## Author

Bartlomiej Zimny

---

If you find this project useful, please consider giving it a star!
