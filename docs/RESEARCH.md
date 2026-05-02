# Research Findings — TE Stem Player (SP-1)

> Compiled from TE-stemplayer repo, timknapen/SP-1-dev wiki, solderless.engineering, and community resources.

## Table of Contents

1. [Device Overview](#device-overview)
2. [Storage Architecture](#storage-architecture)
3. [Corrected Sector Format](#corrected-sector-format)
4. [Album Metadata](#album-metadata)
5. [USB Communication](#usb-communication)
6. [Firmware & Bootloader](#firmware--bootloader)
7. [Plan: Getting New Music On The Device](#plan-getting-new-music-on-the-device)
8. [Open Questions & Blockers](#open-questions--blockers)
9. [Sources](#sources)

---

## Device Overview

- **MCU**: Nordic nRF52840 (ARM Cortex-M4F @ 64MHz, 1MB Flash, 256KB RAM)
- **Storage**: Toshiba THGBMNG5D1LBAIL — 4GB eMMC 5.0, **no filesystem**, raw sector access
- **Audio**: 48kHz, 24-bit, 4 stereo stems (8 channels total)
- **DACs**: TI TAS2505 (speaker), Cirrus Logic CS42L42 (headphone)
- **Bluetooth**: Infineon CYBT-353027-02 (BLE audio streaming)
- **USB**: USB-C, VID:PID `0x2367:0x1701`, CDC ACM serial interface
- **Clock**: 3.072 MHz oscillator → I2S bus (48kHz × 24-bit × 2ch)

## Storage Architecture

The eMMC has **no filesystem**. All data is raw sectors:

| Region | Description |
|--------|-------------|
| Sector 0 | Album metadata (song count, offsets, lengths) |
| Sectors 1+ | Audio data as raw stem sectors |

### Key Insight: eMMC Interface Limitation

The SP-1 uses a **1-bit eMMC interface** (DAT0 only, no DAT1-DAT7). This means:
- Slower read/write speeds than typical eMMC
- But sufficient for 48kHz audio streaming
- Raw sector addressing via CMD17 (read) and CMD24 (write)

## Corrected Sector Format

> ⚠️ **CRITICAL**: The original `SECTOR_FORMAT.md` in this repo had an incorrect interleaving model. The corrections below are based on the authoritative [SP-1-dev wiki](https://github.com/timknapen/SP-1-dev/wiki/Audio-format).

### Sector Layout (8192 bytes)

Each sector is divided into **4 blocks of 2048 bytes**:

```
┌─────────────────────────────────────────────────────────────┐
│ Sector (8192 bytes)                                         │
├─────────────────────────────────────────────────────────────┤
│ Block 0: bytes 0–2047      │ Audio frames for stems         │
│ Block 1: bytes 2048–4095   │ Audio frames for stems         │
│ Block 2: bytes 4096–6143   │ Audio frames for stems         │
│ Block 3: bytes 6144–8191   │ Audio frames for stems         │
└─────────────────────────────────────────────────────────────┘
```

### Block Internal Layout (2048 bytes each)

```
Bytes 0–2039:    340 audio frames × 6 bytes = 2040 bytes
Bytes 2040–2041: Timing/sync data (u16)
Bytes 2042–2043: Tempo data (u16)
Bytes 2044–2047: LED data (4 bytes: R, G, B, Mode)
```

### Frame Interleaving — The Critical Detail

**All 4 stems are packed into ONE sector**, not spread across 4 consecutive sectors.

Each **audio frame** is 24 bytes (not 6 bytes), containing **4 stem frames** of 6 bytes each:

```
Audio Frame (24 bytes):
  Stem 0 frame: bytes 0–5    (e.g., vocals L+R)
  Stem 1 frame: bytes 6–11   (e.g., drums L+R)
  Stem 2 frame: bytes 12–17  (e.g., bass L+R)
  Stem 3 frame: bytes 18–23  (e.g., other L+R)
```

### Block Ordering (Non-Sequential!)

Frames are distributed across blocks in a **non-sequential order**:

```
block_order[] = {0, 2, 1, 3}
```

For frame index `i`:
- `i % 4 == 0` → Block 0 (bytes 0–2047)
- `i % 4 == 1` → Block 2 (bytes 4096–6143)
- `i % 4 == 2` → Block 1 (bytes 2048–4095)
- `i % 4 == 3` → Block 3 (bytes 6144–8191)

Each block holds 340 frames × 6 bytes = 2040 bytes of audio data, so each sector holds **340 × 4 = 1360 total stem frames** (340 per stem).

### Stem Frame Format (6 bytes)

Each stem frame encodes one stereo sample at 24-bit depth:

```
Byte 0: L_MB   (Left middle byte)
Byte 1: L_MSB  (Left most significant byte)
Byte 2: R_MSB  (Right most significant byte)
Byte 3: L_LSB  (Left least significant byte)
Byte 4: R_LSB  (Right least significant byte)
Byte 5: R_MB   (Right middle byte)
```

### Sample Reconstruction

```rust
fn decode_stem_frame(frame: &[u8; 6]) -> (i32, i32) {
    // Left channel
    let left = ((frame[1] as i32) << 24)   // L_MSB → bits 31-24
             | ((frame[0] as i32) << 16)   // L_MB  → bits 23-16
             | ((frame[3] as i32) << 8);   // L_LSB → bits 15-8
    let left = left >> 8; // Sign-extend and shift to 24-bit

    // Right channel
    let right = ((frame[2] as i32) << 24)  // R_MSB → bits 31-24
              | ((frame[5] as i32) << 16)  // R_MB  → bits 23-16
              | ((frame[4] as i32) << 8);  // R_LSB → bits 15-8
    let right = right >> 8;

    (left, right)
}
```

> **Note**: The shift-left-by-8-then-right-by-8 pattern achieves sign extension from 24-bit to 32-bit.

## Album Metadata

Stored at **sector 0** of the eMMC.

| Offset | Size | Description |
|--------|------|-------------|
| 0 | 1 byte | Song count (0–255) |
| 1+ | Variable | Per-song entries |

### Per-Song Entry

| Field | Size | Description |
|-------|------|-------------|
| Start sector | 4 bytes (u32 BE) | First sector of audio data |
| Sector count | 4 bytes (u32 BE) | Number of sectors for this song |
| Song name | Variable | Null-terminated string |

> **Note**: Song count is a single byte (not u32 as previously assumed). Maximum 255 songs.

## USB Communication

### Connection

- **Interface**: CDC ACM (virtual serial port)
- **VID:PID**: `0x2367:0x1701`
- **Platform paths**: `/dev/ttyACM0` (Linux), auto-detected on macOS
- **Baud**: 115200

### Protocol

The device uses a command/response protocol over CDC ACM:

1. **Device detection**: Enumerate USB → filter by VID:PID
2. **Open serial connection**: Standard serial port settings
3. **Send commands**: Text-based commands for status, read, write
4. **Sector I/O**: Read/write raw 8192-byte sectors

### Key Commands (from reverse engineering)

| Command | Description |
|---------|-------------|
| `STATUS` | Query device state |
| `READ <sector>` | Read a raw sector |
| `WRITE <sector>` | Write a raw sector |

## Firmware & Bootloader

### Memory Map

```
0x00000–0x1FFFF: Bootloader (128KB)
0x20000–0xFEFFF: Application firmware (max ~0xDEFFF = 896KB)
```

### Bootloader Entry

The bootloader initializes watchdog, clocks, PWM, and SAADC on boot. There is **no hard reset** — the firmware must use `SYSTEM_OFF` to return to the bootloader.

### Firmware Update Methods

1. **Web-based**: [solderless.engineering](https://solderless.engineering) — Uses WebUSB to flash firmware via browser
2. **Physical bootloader entry**: Requires hardware manipulation (specific button/pin sequence)
3. **DFU over USB**: Nordic DFU protocol (if bootloader supports it)

### Current Blocker: `emmc_ok=0`

The device context file notes `emmc_ok=0`, meaning the eMMC initialization is failing in the current firmware state. This must be resolved before any audio data can be written.

**Possible causes**:
- Firmware bug in eMMC initialization sequence
- eMMC CMD/DAT timing issues on the 1-bit interface
- Need to flash working firmware first

## Plan: Getting New Music On The Device

### Phase 1: Device Backup & Connection

> **No firmware changes required.** The stock firmware exposes USB CDC ACM serial access to the eMMC. As long as the device plays its default music, the eMMC is working.

1. **Connect device via USB-C** → detect CDC ACM serial port (VID:PID `0x2367:0x1701`)
2. **Full device backup**: Read ALL sectors from the eMMC and save to a binary backup file (`.stembackup`)
   - Read sector 0 (album metadata) to determine total sector count
   - Sequentially read every sector and write to the backup file
   - Store a SHA-256 checksum for integrity verification
3. **Verify backup**: Compare checksums and spot-check random sectors
4. **Restore capability**: The app will include a "Restore from Backup" function that writes the entire backup file back to the device sector-by-sector, restoring it to factory state

### Phase 2: Stem Preparation Pipeline

1. **Source audio**: Start with a WAV/FLAC/MP3 file
2. **Stem separation**: Use a model like Demucs, Spleeter, or HTDemucs to split into 4 stems:
   - Vocals, Drums, Bass, Other
3. **Resample**: Ensure all stems are 48kHz, 24-bit, stereo
4. **Encode**: Convert each stem's samples into 6-byte frame format
5. **Interleave**: Pack 4 stem frames into 24-byte audio frames
6. **Sectorize**: Distribute frames across 4 blocks per sector using the block ordering `{0, 2, 1, 3}`
7. **Add metadata**: Timing/sync, tempo, LED data per block
8. **Build album**: Update sector 0 metadata with song offsets and lengths

### Phase 3: Upload via USB

1. Connect device via USB-C
2. Open CDC ACM serial connection
3. Read sector 0 to get current album metadata
4. Write new audio sectors sequentially
5. Update sector 0 with new album metadata
6. Verify written data by reading back sectors

### Phase 4: Tauri Desktop App

The TE-StemPlayer repo already has a Tauri + React + TypeScript application scaffold:

1. **Frontend (React/TypeScript)**:
   - Drag-and-drop audio file input
   - Stem separation progress UI
   - Song library management
   - Device connection status
   - Upload progress

2. **Backend (Rust/Tauri)**:
   - USB device detection and serial communication
   - Sector read/write operations
   - Audio encoding/decoding
   - Stem separation (via sidecar or native)
   - Album metadata management

3. **Sidecar**:
   - Python-based stem separation using Demucs/HTDemucs
   - Outputs 4 stem WAV files

## Open Questions & Blockers

| # | Question/Blocker | Status |
|---|-----------------|--------|
| 1 | Exact USB protocol commands for sector read/write | 🟡 Needs verification |
| 2 | Album metadata format — exact byte layout beyond song count | 🟡 Partially known |
| 3 | Total sector count — how to determine eMMC used/total size | 🟡 Needed for full backup |
| 4 | LED mode values — are there more than off/solid/pulse/reactive? | 🟢 Low priority |
| 5 | Tempo data format in bytes 2042–2043 | 🟡 Unknown encoding |
| 6 | Backup file format — include header with device info + checksum? | 🟡 Design needed |

## Sources

1. **TE-StemPlayer repo**: This repository — Tauri app, architecture docs, USB protocol
2. **SP-1-dev wiki**: [github.com/timknapen/SP-1-dev](https://github.com/timknapen/SP-1-dev) — Hardware details, pin mappings, audio format, bootloader info
3. **solderless.engineering**: [solderless.engineering](https://solderless.engineering) — Web-based firmware flasher
4. **yzy-stemplayer-reverse**: [github.com/leabs/yzy-stemplayer-reverse](https://github.com/leabs/yzy-stemplayer-reverse) — USB reverse engineering
5. **stemplayer_pins.h**: Pin mapping header from SP-1-dev repo
6. **Community forums**: Various Reddit/Discord threads on stem player modding
