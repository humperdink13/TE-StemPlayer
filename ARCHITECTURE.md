# Architecture — TE-StemPlayer Studio

## System Overview

```
┌─────────────────────────────────────────────────────┐
│                   UI Layer (React)                   │
│  ┌────────────┐ ┌────────────┐ ┌─────────────────┐  │
│  │  Device     │ │  Stem      │ │  Firmware       │  │
│  │  Manager    │ │  Creator   │ │  Flasher        │  │
│  └──────┬──────┘ └──────┬─────┘ └───────┬─────────┘  │
├─────────┼───────────────┼───────────────┼────────────┤
│               Backend Services (Rust/Tauri)          │
│  ┌──────┴──────┐ ┌──────┴─────┐ ┌───────┴─────────┐  │
│  │  USB/CDC    │ │  Audio     │ │  YouTube        │  │
│  │  Driver     │ │  Processor │ │  Downloader     │  │
│  └─────────────┘ └────────────┘ └─────────────────┘  │
├──────────────────────────────────────────────────────┤
│               Native / External Processes            │
│  libusb · Demucs (Python) · yt-dlp · FFmpeg          │
└──────────────────────────────────────────────────────┘
```

## Design Decisions

### Why Tauri over Electron?
- **Binary size**: ~5MB vs ~150MB+ for Electron
- **Memory**: Uses system webview, not bundled Chromium
- **Rust backend**: Direct access to `rusb` for USB communication without Node.js native modules
- **Security**: IPC-based command model with fine-grained permissions
- **Performance**: Rust handles audio sector packing at native speed

### Why Python Sidecar for Demucs?
- Demucs requires PyTorch — too heavy for Rust FFI
- Sidecar model: Tauri spawns Python process, communicates via stdout/stdin JSON
- GPU acceleration (CUDA/MPS) is only available through PyTorch
- Process isolation: if Demucs crashes, the app survives

### Why Raw Sector Access (not filesystem)?
- The SP-1 eMMC has **no filesystem** — it uses raw sector-based storage
- We implement our own sector read/write over USB vendor control transfers
- Album metadata is stored in a custom binary format in sector 0

## Module Architecture

### 1. USB Module (`src-tauri/src/usb/`)

```
┌─────────────────────────────────────────┐
│              USB Module                  │
├─────────────────────────────────────────┤
│  device.rs                              │
│  ├── find_stem_player() → Option<Dev>   │
│  ├── DeviceHandle (open/close)          │
│  └── poll_connection() (hotplug)        │
├─────────────────────────────────────────┤
│  protocol.rs                            │
│  ├── CdcAcmChannel (serial I/O)        │
│  ├── vendor_read(bRequest=0x01, ...)    │
│  └── vendor_write(bRequest=0x02, ...)   │
├─────────────────────────────────────────┤
│  emmc.rs                                │
│  ├── read_sector(offset) → [u8; 8192]  │
│  ├── write_sector(offset, data)         │
│  ├── read_album_metadata() → Album     │
│  └── write_album_metadata(Album)        │
└─────────────────────────────────────────┘
```

**Device Detection**: Poll USB bus for VID `0x2367` / PID `0x1701`. On connection, claim CDC ACM interface and prepare vendor transfer endpoints.

**Protocol**: Two transfer types:
- **CDC ACM**: Serial communication for device status/commands
- **Vendor Control Transfers**: `bRequest=0x01` for reads, `bRequest=0x02` for writes. Used for direct eMMC sector access.

### 2. Audio Module (`src-tauri/src/audio/`)

```
┌─────────────────────────────────────────┐
│             Audio Module                │
├─────────────────────────────────────────┤
│  sector.rs                              │
│  ├── StemFrame { channels: [i32; 8] }   │
│  ├── Sector { frames: [Frame; 340],     │
│  │            timing: u32, leds: u32 }  │
│  ├── pack_stems(4x WAV) → Vec<Sector>  │
│  └── unpack_sectors(Vec<Sector>) → WAVs │
├─────────────────────────────────────────┤
│  album.rs                               │
│  ├── Album { songs: Vec<Song>, ... }    │
│  ├── Song { title, artist, sectors }    │
│  ├── parse_album(sector_data) → Album   │
│  └── serialize_album(Album) → Vec<u8>   │
└─────────────────────────────────────────┘
```

**Sector Format** (8192 bytes):
- Bytes 0–2039: 340 audio frames, 6 bytes each
- Frame byte order: `L_MB, L_MSB, R_MSB, L_LSB, R_LSB, R_MB`
- Bytes 2040–2043: Timing data (u32 big-endian)
- Bytes 2044–2047: LED color data (u32, RGB + mode)
- Remaining bytes: Padding/reserved

**Stem Interleaving**: 4 stereo stems = 8 channels. Each frame contains one sample per channel at 48kHz/24-bit.

### 3. Sidecar Module (`sidecar/`)

```
┌─────────────────────────────────────────────┐
│          Python Sidecar (Demucs)            │
├─────────────────────────────────────────────┤
│  stem_separator.py                          │
│  ├── Reads JSON commands from stdin         │
│  ├── Commands:                              │
│  │   ├── separate(input, output_dir, model) │
│  │   ├── download(youtube_url, output_path) │
│  │   └── status()                           │
│  ├── Streams progress as JSON to stdout     │
│  └── Outputs: vocals.wav, drums.wav,        │
│       bass.wav, other.wav (48kHz/24-bit)    │
└─────────────────────────────────────────────┘
```

**Communication Protocol** (stdin/stdout JSON):
```json
// Request (Tauri → Python)
{"cmd": "separate", "input": "/path/to/song.wav", "output_dir": "/tmp/stems/", "model": "htdemucs"}

// Progress (Python → Tauri)
{"type": "progress", "stage": "separating", "percent": 45}

// Result (Python → Tauri)
{"type": "result", "stems": ["vocals.wav", "drums.wav", "bass.wav", "other.wav"]}
```

### 4. Frontend Architecture

```
┌─────────────────────────────────────────────┐
│              React Application              │
├─────────────────────────────────────────────┤
│  Stores (Zustand)                           │
│  ├── deviceStore: connection, status, songs │
│  ├── stemStore: separation jobs, previews   │
│  └── albumStore: album editing state        │
├─────────────────────────────────────────────┤
│  Tauri Command Bridge (invoke)              │
│  ├── usb_connect() / usb_disconnect()       │
│  ├── read_album() / write_album()           │
│  ├── start_separation(source) → job_id      │
│  ├── upload_song(song_data)                 │
│  └── flash_firmware(binary_path)            │
├─────────────────────────────────────────────┤
│  Event Listeners (Tauri events)             │
│  ├── device-connected / device-disconnected │
│  ├── separation-progress / separation-done  │
│  ├── upload-progress / upload-done          │
│  └── flash-progress / flash-done            │
└─────────────────────────────────────────────┘
```

## Data Flow: YouTube → Device

```
YouTube URL
    │
    ▼  yt-dlp --extract-audio --audio-format wav
temp.wav (source sample rate)
    │
    ▼  ffmpeg -i temp.wav -ar 48000 -sample_fmt s32 -ac 2
normalized.wav (48kHz, 24-bit, stereo)
    │
    ▼  demucs -n htdemucs --two-stems=no
vocals.wav, drums.wav, bass.wav, other.wav
    │
    ▼  Rust: sector::pack_stems()
Vec<Sector> (8192-byte sectors, 8ch interleaved)
    │
    ▼  Rust: album::update_metadata()
Album header sector updated with new song entry
    │
    ▼  USB: emmc::write_sector() for each sector
Device eMMC programmed
```

## Security Considerations

- **USB**: All USB operations require explicit user confirmation for device access
- **YouTube**: Include legal disclaimer; local file import is the primary supported path
- **Firmware**: Binary size validation (≤ 0xDEFFF), SHA256 checksum verification, confirmation dialog
- **Sidecar**: Python process runs sandboxed, no network access after download phase
- **IPC**: Tauri's strict CSP and command allowlist

## Error Handling Strategy

| Scenario | Handling |
|----------|---------|
| USB disconnect mid-write | Journal last written sector, resume on reconnect |
| Demucs OOM | Catch subprocess exit, suggest closing other apps or using CPU mode |
| Corrupt album metadata | Validate magic bytes before every read; offer backup restore |
| yt-dlp failure | Retry with fallback format; clear error message with URL validation |
| Firmware flash failure | Bootloader has 5s watchdog — app warns user not to disconnect |
