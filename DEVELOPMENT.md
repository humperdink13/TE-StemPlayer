# Development Guide — TE-StemPlayer Studio

## Prerequisites

| Tool | Version | Required For |
|------|---------|-------------|
| Rust | ≥ 1.70 | Tauri backend |
| Node.js | ≥ 18 | React frontend |
| npm | ≥ 9 | Package management |
| Python | ≥ 3.9 | Demucs sidecar |
| FFmpeg | ≥ 5.0 | Audio conversion |
| libusb | ≥ 1.0 | USB communication |

### Platform-Specific Dependencies

**Linux (Debian/Ubuntu)**:
```bash
sudo apt install libwebkit2gtk-4.1-dev libusb-1.0-0-dev build-essential \
  curl wget file libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

**macOS**:
```bash
xcode-select --install
brew install libusb ffmpeg
```

**Windows**:
- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
- Install [FFmpeg](https://ffmpeg.org/download.html) and add to PATH
- libusb included via rusb crate

## Setup

```bash
# 1. Clone repository
git clone https://github.com/YOUR_ORG/TE-StemPlayer.git
cd TE-StemPlayer

# 2. Install frontend dependencies
npm install

# 3. Install Python sidecar dependencies
python3 -m venv .venv
source .venv/bin/activate  # or .venv\Scripts\activate on Windows
pip install -r sidecar/requirements.txt

# 4. Verify Rust toolchain
rustup update
cargo --version

# 5. Run development server
npm run tauri dev
```

## Project Layout

```
TE-StemPlayer/
├── src-tauri/           # Rust backend
│   ├── Cargo.toml       # Rust dependencies
│   ├── tauri.conf.json  # Tauri configuration
│   └── src/
│       ├── main.rs      # Entry point
│       ├── lib.rs       # Command registration
│       ├── usb/         # USB device module
│       ├── audio/       # Sector packing module
│       └── firmware/    # Firmware flash module
├── src/                 # React frontend
│   ├── main.tsx         # React entry
│   ├── App.tsx          # Root component
│   ├── components/      # UI components
│   ├── hooks/           # Custom hooks
│   ├── stores/          # Zustand stores
│   └── utils/           # Helpers
├── sidecar/             # Python Demucs sidecar
├── docs/                # Technical documentation
└── package.json         # Node dependencies & scripts
```

## Available Scripts

| Command | Description |
|---------|-------------|
| `npm run tauri dev` | Start dev server with hot reload |
| `npm run tauri build` | Build production binary |
| `npm run dev` | Start Vite dev server only (no Tauri) |
| `npm run build` | Build frontend only |
| `npm run lint` | Run ESLint |
| `npm run test` | Run Vitest |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Run Rust tests |
| `python sidecar/stem_separator.py --test` | Test sidecar |

## Architecture Overview

See [ARCHITECTURE.md](./ARCHITECTURE.md) for full details.

- **Frontend ↔ Backend**: Tauri IPC via `invoke()` commands and event listeners
- **Backend ↔ USB**: `rusb` crate for libusb bindings
- **Backend ↔ Sidecar**: Spawned Python subprocess, JSON over stdin/stdout

## Coding Standards

### Rust (Backend)
- Follow `rustfmt` defaults
- Use `clippy` with `--deny warnings`
- Error handling: Use `anyhow::Result` for commands, `thiserror` for library errors
- All USB operations must be wrapped in `Result` with meaningful error messages
- Document all public functions

### TypeScript (Frontend)
- Strict mode enabled
- ESLint + Prettier
- Functional components only (no class components)
- Zustand for state (no Redux, no Context for global state)
- All Tauri commands wrapped in typed helper functions (`src/utils/tauriCommands.ts`)

### Python (Sidecar)
- Type hints required
- Black formatter
- JSON-line protocol: one JSON object per line on stdout
- Never print non-JSON to stdout (use stderr for logs)

## Testing Strategy

### Unit Tests
- **Rust**: Sector packing/unpacking, album metadata parsing, frame encoding/decoding
- **TypeScript**: Store logic, utility functions, command wrappers
- **Python**: Separation pipeline with mock audio

### Integration Tests
- USB communication with mock device (use `rusb` test fixtures)
- Full pipeline: WAV → separate → pack → verify sector format
- Tauri command round-trip tests

### Manual Testing
- Device connection/disconnection handling
- Full upload workflow with real device
- YouTube download + separation + upload end-to-end

## Git Workflow

- `main`: Production-ready, tagged releases
- `develop`: Integration branch
- `feature/*`: Feature branches (e.g., `feature/stem-creator`)
- `fix/*`: Bug fix branches

### Commit Convention
```
type(scope): description

feat(usb): add device hotplug detection
fix(audio): correct frame byte order in sector packing
docs(api): update USB protocol documentation
```

## Building for Production

```bash
# Build for current platform
npm run tauri build

# Output locations:
# Linux:   src-tauri/target/release/bundle/deb/
# macOS:   src-tauri/target/release/bundle/dmg/
# Windows: src-tauri/target/release/bundle/msi/
```

### Bundling the Sidecar
Tauri's sidecar feature bundles the Python script. Configure in `tauri.conf.json`:
```json
{
  "bundle": {
    "externalBin": ["sidecar/stem_separator"]
  }
}
```

For distribution, use PyInstaller to compile the sidecar to a standalone binary:
```bash
cd sidecar
pyinstaller --onefile stem_separator.py
```
