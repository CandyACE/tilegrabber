<div align="center">

# TileGrabber (御图)

**Map Tile Batch Downloader & Publisher**

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)]()
[![Tauri](https://img.shields.io/badge/Tauri-2-blue.svg)](https://tauri.app)
[![Vue](https://img.shields.io/badge/Vue-3-green.svg)](https://vuejs.org)

![Screenshot](./screenshot/1.jpg)

[中文](README.md) · English

</div>

---

## Overview

TileGrabber is an open-source desktop application for downloading and managing map tiles, built with [Tauri 2](https://tauri.app) + [Vue 3](https://vuejs.org). It runs natively on Windows, macOS, and Linux.

Draw an area on the interactive map, choose zoom levels, and TileGrabber downloads all corresponding tiles from any supported source — then export or serve them locally in multiple formats.

**Common use cases:**

- Pre-distributing map data for offline environments
- Archiving online map data as local files (MBTiles, GeoTIFF, etc.)
- Serving downloaded tiles as a local TMS/WMTS service for other applications

---

## Features

### Data Sources
- **LRC / LRA files** — Parse region files exported by tools such as Oruxmaps; auto-detects the tile URL and bounding area
- **WMTS services** — Paste a GetCapabilities XML URL; TileGrabber parses available layers and tile extents automatically
- **TMS URL templates** — Enter a `{z}/{x}/{y}` template URL and previews tiles immediately
- **Web capture** — Enter any map website URL; TileGrabber sniffs tile requests from the page and lets you pick the target layer visually

### Download Engine
- Multi-threaded concurrent downloading with configurable concurrency
- Resume interrupted downloads — no duplicate requests after a network failure
- Intelligent rate limiting with randomised delays to mimic natural browsing and reduce ban risk
- User-Agent rotation
- Real-time progress display, including a compact floating progress window

### Task Management
- Sidebar task list with one-click switching between tasks — view download bounds and tile coverage on the map
- Pause, resume, cancel, or delete tasks at any time
- Import and export tasks as `.tgr` files (SQLite-based binary format, zero-copy fast transfer)

### Export Formats
| Format | Description |
|--------|-------------|
| Directory | Tiles stored as `z/x/y.png` folder hierarchy |
| **MBTiles** | Single-file SQLite database; compatible with QGIS, MapTiler, etc. |
| **GeoTIFF / BigTIFF** | Georeferenced raster image; supports files larger than 4 GB |

### Publishing
- Built-in HTTP server to publish local tiles as **TMS** or **WMTS** endpoints for other applications
- Polygon boundary clipping support

### Other
- Automatic update checks with one-click download and install
- Multi-language UI (Chinese / English)
- Built-in help documentation and FAQ

---

## Download & Install

Go to the [Releases](../../releases/latest) page and download the package for your platform:

| Platform | File | Notes |
|----------|------|-------|
| Windows 10/11 | `*_x64-setup.exe` | NSIS installer, double-click to run |
| macOS (Apple Silicon) | `*_aarch64.dmg` | M-series chips |
| macOS (Intel) | `*_x64.dmg` | x86_64 |
| Linux | `*.AppImage` | No install needed; run `chmod +x` first |
| Linux | `*_amd64.deb` | Debian / Ubuntu |

> **macOS users**: If Gatekeeper blocks the app on first launch, go to **System Settings → Privacy & Security** and click **Open Anyway**, or run:
> ```bash
> xattr -d com.apple.quarantine /Applications/TileGrabber.app
> ```

---

## Build from Source

### Prerequisites

- [Node.js](https://nodejs.org) 18+
- [Rust](https://rustup.rs) stable toolchain
- [System dependencies (Linux only)](#linux-dependencies)

### Steps

```bash
# Clone the repository
git clone https://github.com/your-org/tilegrabber.git
cd tilegrabber

# Install frontend dependencies
npm install

# Start in development mode
npm run tauri:dev

# Build a release package
npm run tauri:build
```

### Linux Dependencies

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf
```

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Frontend framework | Vue 3 + TypeScript |
| Map rendering | MapLibre GL JS |
| Area drawing | Terra Draw |
| UI components | Reka UI + Tailwind CSS v4 |
| Desktop shell | Tauri 2 |
| Backend language | Rust |
| Database | SQLite (rusqlite, bundled) |
| HTTP client | reqwest (rustls-tls) |
| Image processing | image / tiff crates |
| Concurrency | Tokio + Rayon |

---

## Project Structure

```
├── src/                  # Vue frontend source
│   ├── components/
│   │   ├── map/          # Map components (drawing, progress layer, tile preview, etc.)
│   │   ├── sidebar/      # Sidebar panels (tasks, download config, export, publish, etc.)
│   │   └── wizard/       # New task wizard
│   ├── composables/      # Vue composables
│   └── locales/          # i18n locale files
├── src-tauri/            # Rust backend source
│   └── src/
│       ├── commands/     # Tauri commands (tasks, download, export, publish, updater, etc.)
│       ├── download/     # Download engine (multi-thread, throttle, resume)
│       ├── export/       # Export modules (directory, MBTiles, GeoTIFF)
│       ├── parser/       # Parsers (LRC/LRA, WMTS, web capture)
│       └── server/       # Built-in TMS/WMTS HTTP server
└── .github/workflows/    # CI/CD pipelines
```

---

## Contributing

Issues and pull requests are welcome! Before submitting a PR, please ensure:

1. `npm run build` completes without errors
2. `cargo check` passes in `src-tauri/`
3. Code style is consistent with the existing codebase

---

## License

This project is licensed under the [MIT License](LICENSE).
