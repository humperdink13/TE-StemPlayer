# TE-StemPlayer Studio

A cross-platform desktop application for managing, programming, and creating content for the **Teenage Engineering SP-1 Stem Player**.

## Features

- 🎵 **Stem Creator** — Split any MP3/WAV/FLAC or YouTube stream into 4 stems (vocals, drums, bass, other) using AI-powered source separation
- 📱 **Device Manager** — Connect to your Stem Player over USB, view status, storage, and firmware info
- 💾 **Album Manager** — Organize songs, edit metadata, and upload to device
- ⚡ **Firmware Flasher** — Update device firmware safely

## Tech Stack

| Component | Technology |
|-----------|-----------|
| App Shell | [Tauri 2.0](https://tauri.app/) (Rust backend) |
| UI | React 18 + TypeScript + Tailwind CSS |
| USB | `rusb` crate (libusb wrapper) via Tauri commands |
| Audio Separation | [Demucs v4 / HTDemucs](https://github.com/facebookresearch/demucs) (Python sidecar) |
| YouTube Download | [yt-dlp](https://github.com/yt-dlp/yt-dlp) (bundled binary) |
| Audio Processing | [FFmpeg](https://ffmpeg.org/) (bundled) + `symphonia` (Rust crate) |
| Waveform Rendering | [wavesurfer.js](https://wavesurfer.xyz/) |
| State Management | [Zustand](https://zustand-demo.pmnd.rs/) |

## Prerequisites

- **Rust** ≥ 1.70
- **Node.js** ≥ 18
- **Python** ≥ 3.9 (for Demucs sidecar)
- **FFmpeg** (installed and on PATH)
- **System dependencies**: libusb-1.0-dev (Linux), libwebkit2gtk-4.1-dev (Linux)

## Quick Start

```bash
# Clone
git clone https://github.com/YOUR_ORG/TE-StemPlayer.git
cd TE-StemPlayer

# Install frontend dependencies
npm install

# Install Python sidecar dependencies
pip install -r sidecar/requirements.txt

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Project Structure

```
TE-StemPlayer/
├── README.md                  # This file
├── ARCHITECTURE.md            # System architecture & design decisions
├── DEVELOPMENT.md             # Development guide & contribution rules
├── docs/
│   ├── USB_PROTOCOL.md        # SP-1 USB communication protocol
│   ├── SECTOR_FORMAT.md       # eMMC sector & audio data format
│   ├── STEM_CREATION.md       # Audio separation pipeline
│   └── HARDWARE.md            # SP-1 hardware reference
├── src-tauri/                 # Rust backend (Tauri)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # Tauri entry point
│       ├── lib.rs             # Command registration
│       ├── usb/               # USB device communication
│       │   ├── mod.rs
│       │   ├── device.rs      # Device detection & connection
│       │   ├── protocol.rs    # CDC ACM & vendor transfers
│       │   └── emmc.rs        # eMMC read/write operations
│       ├── audio/             # Audio processing
│       │   ├── mod.rs
│       │   ├── sector.rs      # Sector packing/unpacking
│       │   └── album.rs       # Album metadata handling
│       └── firmware/          # Firmware operations
│           ├── mod.rs
│           └── flasher.rs     # Firmware upload logic
├── src/                       # React frontend
│   ├── main.tsx               # React entry point
│   ├── App.tsx                # Root component with routing
│   ├── components/
│   │   ├── DeviceManager/
│   │   │   ├── DeviceManager.tsx
│   │   │   ├── DeviceStatus.tsx
│   │   │   └── SongList.tsx
│   │   ├── StemCreator/
│   │   │   ├── StemCreator.tsx
│   │   │   ├── SourceInput.tsx
│   │   │   ├── SeparationProgress.tsx
│   │   │   └── StemPreview.tsx
│   │   ├── AlbumManager/
│   │   │   ├── AlbumManager.tsx
│   │   │   ├── SongEditor.tsx
│   │   │   └── UploadQueue.tsx
│   │   └── FirmwareFlasher/
│   │       ├── FirmwareFlasher.tsx
│   │       └── FlashProgress.tsx
│   ├── hooks/
│   │   ├── useDevice.ts
│   │   ├── useStemCreator.ts
│   │   └── useAlbum.ts
│   ├── stores/
│   │   ├── deviceStore.ts
│   │   ├── stemStore.ts
│   │   └── albumStore.ts
│   └── utils/
│       ├── tauriCommands.ts
│       └── audioHelpers.ts
├── sidecar/                   # Python sidecar for Demucs
│   ├── requirements.txt
│   └── stem_separator.py
└── package.json
```

## Documentation

- [Architecture](./ARCHITECTURE.md) — System design, data flow, module interaction
- [Development Guide](./DEVELOPMENT.md) — Setup, coding standards, testing
- [USB Protocol](./docs/USB_PROTOCOL.md) — How to communicate with the SP-1
- [Sector Format](./docs/SECTOR_FORMAT.md) — eMMC data layout and audio encoding
- [Stem Creation](./docs/STEM_CREATION.md) — Audio separation pipeline details
- [Hardware Reference](./docs/HARDWARE.md) — SP-1 hardware components and pinout

## License

MIT
