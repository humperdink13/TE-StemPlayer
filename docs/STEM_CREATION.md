# Stem Creation Pipeline — TE-StemPlayer Studio

## Overview

The stem creation pipeline converts any audio source (local file or YouTube URL) into 4 separated stems formatted for the SP-1's raw sector storage.

## Pipeline Stages

```
┌──────────┐    ┌───────────┐    ┌───────────┐    ┌──────────┐    ┌──────────┐
│  Source   │───▶│ Download  │───▶│ Normalize │───▶│ Separate │───▶│  Pack    │
│  Input    │    │ (yt-dlp)  │    │ (ffmpeg)  │    │ (Demucs) │    │ (Rust)   │
└──────────┘    └───────────┘    └───────────┘    └──────────┘    └──────────┘
```

## Stage 1: Source Input

### Supported Sources
| Source | Format | Handler |
|--------|--------|---------|
| Local file | MP3, WAV, FLAC, OGG, AAC, M4A | Direct pass to Stage 3 |
| YouTube URL | Any youtube.com/youtu.be URL | yt-dlp download |
| YouTube playlist | Playlist URL | yt-dlp batch download |

### Validation
- Local files: Check file exists, verify audio codec with ffprobe
- YouTube URLs: Validate URL format, check availability via yt-dlp --dump-json

## Stage 2: Download (YouTube only)

Uses **yt-dlp** as a bundled binary or system install.

```bash
yt-dlp \
  --extract-audio \
  --audio-format wav \
  --audio-quality 0 \
  --output "/tmp/stems/%(title)s.%(ext)s" \
  --no-playlist \
  "$YOUTUBE_URL"
```

**Configuration**:
- `--audio-quality 0`: Best quality
- `--no-playlist`: Single video only (unless batch mode)
- `--cookies-from-browser`: Optional, for age-restricted content

**Error Handling**:
- Network timeout → Retry 3 times with exponential backoff
- Geo-restricted → Inform user, suggest VPN
- Age-restricted → Prompt for cookie authentication

**Legal Notice**: YouTube downloading may violate YouTube's Terms of Service. This feature is provided for personal use with content you have rights to. Local file import is the recommended primary workflow.

## Stage 3: Normalize

Convert to Demucs-compatible format using **FFmpeg**.

```bash
ffmpeg -i input.wav \
  -ar 44100 \
  -ac 2 \
  -sample_fmt s16 \
  -y normalized.wav
```

**Note**: Demucs expects 44.1kHz stereo input. We normalize to 44.1kHz for separation, then resample the output stems to 48kHz for SP-1 packing.

## Stage 4: Separate (Demucs)

### Model Selection

| Model | Quality | Speed (CPU) | Speed (GPU) | VRAM |
|-------|---------|-------------|-------------|------|
| `htdemucs` | Best | ~3 min/song | ~30s/song | ~4GB |
| `htdemucs_ft` | Best+ | ~5 min/song | ~45s/song | ~6GB |
| `mdx_extra` | Good | ~2 min/song | ~20s/song | ~3GB |

Default: `htdemucs` (best balance of quality and speed).

### Sidecar Communication

The Python sidecar (`stem_separator.py`) runs as a long-lived subprocess.

**Request** (Tauri → Python via stdin):
```json
{
  "cmd": "separate",
  "input": "/tmp/stems/normalized.wav",
  "output_dir": "/tmp/stems/output/",
  "model": "htdemucs",
  "device": "auto"
}
```

**Progress** (Python → Tauri via stdout):
```json
{"type": "progress", "stage": "loading_model", "percent": 10}
{"type": "progress", "stage": "separating", "percent": 45}
{"type": "progress", "stage": "saving", "percent": 90}
```

**Result** (Python → Tauri via stdout):
```json
{
  "type": "result",
  "success": true,
  "stems": {
    "vocals": "/tmp/stems/output/htdemucs/song/vocals.wav",
    "drums": "/tmp/stems/output/htdemucs/song/drums.wav",
    "bass": "/tmp/stems/output/htdemucs/song/bass.wav",
    "other": "/tmp/stems/output/htdemucs/song/other.wav"
  },
  "duration_secs": 245.3,
  "model": "htdemucs"
}
```

### Device Auto-Detection
```python
def get_device():
    if torch.cuda.is_available():
        return "cuda"
    elif torch.backends.mps.is_available():
        return "mps"
    return "cpu"
```

## Stage 5: Resample & Pack

After separation, stems are at 44.1kHz. We resample to 48kHz/24-bit for SP-1.

### Resample (FFmpeg)
```bash
# For each stem
ffmpeg -i vocals.wav -ar 48000 -sample_fmt s32 -ac 2 -y vocals_48k.wav
ffmpeg -i drums.wav  -ar 48000 -sample_fmt s32 -ac 2 -y drums_48k.wav
ffmpeg -i bass.wav   -ar 48000 -sample_fmt s32 -ac 2 -y bass_48k.wav
ffmpeg -i other.wav  -ar 48000 -sample_fmt s32 -ac 2 -y other_48k.wav
```

### Pack into Sectors (Rust)
See [SECTOR_FORMAT.md](./SECTOR_FORMAT.md) for full sector packing details.

```
48kHz stems → Read interleaved samples → Chunk into 340-frame groups
→ Encode each frame (6 bytes) → Fill sector buffer (8192 bytes)
→ Add timing + LED data → Interleave stems (4 sectors per block)
```

## Batch Processing

For albums or playlists:
1. Queue all sources
2. Download all (parallel, max 3 concurrent)
3. Separate sequentially (GPU memory constraint)
4. Pack all, build album metadata
5. Upload as single album

## Temp File Management

```
/tmp/te-stemplayer/
├── downloads/          # yt-dlp output
├── normalized/         # FFmpeg normalized input
├── separated/          # Demucs output stems
├── resampled/          # 48kHz stems ready for packing
└── packed/             # Final sector data ready for upload
```

All temp files are cleaned up after successful upload or on app exit.

## Performance Benchmarks (Estimated)

| Stage | 4-min song (CPU) | 4-min song (GPU) |
|-------|-------------------|-------------------|
| Download | 5–15s | 5–15s |
| Normalize | <1s | <1s |
| Separate | 2–5 min | 20–45s |
| Resample | <2s | <2s |
| Pack | <1s | <1s |
| **Total** | **~3–6 min** | **~30–75s** |
