# TE-StemPlayer Project Context

## Project Overview
Full-stack Tauri desktop app for the Teenage Engineering Stem Player (SP-1). Separates songs into stems using AI (Demucs), manages albums, and flashes custom firmware via USB.

**GitHub**: https://github.com/humperdink13/TE-StemPlayer

## Tech Stack
- **App Shell**: Tauri 2.0 (Rust backend)
- **UI**: React + TypeScript + Tailwind CSS (dark theme)
- **USB**: rusb + serialport crates (CDC ACM serial protocol)
- **Audio Separation**: Demucs v4 / HTDemucs (Python sidecar)
- **YouTube Download**: yt-dlp (Python sidecar)
- **Audio Processing**: ffmpeg + symphonia
- **Waveform**: wavesurfer.js
- **State**: Zustand

## Hardware Details

### Device Identity
- **Teenage Engineering SP-1 Stem Player** (designed by Kano Computing + Kanye West)
- USB VID: 0x2367, PID: 0x1701
- Serial: F4725FFE7DC7
- Circular beige device, 7cm diameter
- Full-Speed USB 12Mbps, bMaxPacketSize0=64

### Key Hardware Components
- **MCU**: Nordic nRF52840 - ARM Cortex-M4 @ 64MHz, 1MB flash, 256KB RAM
- **Speaker Amp**: TI TAS2505 (I2C controlled)
- **Headphone Codec/Amp**: Cirrus Logic CS42L42 (I2C controlled, provides I2S LRCLK)
- **Storage**: Toshiba/Kioxia THGBMNG5D1LBAIL - 4GB eMMC 5.0 (1-bit mode: DAT0, CMD, CLK only)
- **Bluetooth**: Infineon CYBT-353027-02 (BLE Audio module, UART controlled, receives audio via I2S)
- **Battery Charger**: TI BQ24232 (USB-friendly Li-ion charger)
- **Audio Clock**: 3.072 MHz oscillator

### Audio Format
- 24-bit, 48kHz, 8-channel (4 stereo stems)
- I2S bus connects MCU → speaker amp, headphone amp, BT module
- No filesystem on eMMC - raw sector-based storage
- 8192-byte app sectors (composed of 16x 512-byte eMMC blocks)
- 340 audio frames per sector (7.083ms per sector)
- Interleaved stem frame byte order: L_MB, L_MSB, R_MSB, L_LSB, R_LSB, R_MB
- Sectors contain timing data (bytes 2040-2043) and LED data (bytes 2044-2047)

### Album Metadata Format
- First sector: magic bytes 'ALBUM_PRESENT', album length, song count, title
- Song entries: start offset, length (in sectors), artist name, song title (64 chars each, null-terminated, X-padded)
- Album ends with 'ALBUM_PRESENT' magic bytes

## USB Communication History & Findings

### Stock Firmware (Factory)
- Device has **0 endpoints** and STALLs all vendor/class control transfers
- Only standard USB requests work (GET_DESCRIPTOR, GET_STRING)
- The reverse-engineering repos (leabs/yzy-stemplayer-reverse) were fuzzing scripts that got the same pipe errors - they never actually succeeded
- Google Chrome (solderless.engineering) attaches AppleUSBHostDeviceUserClient which blocks other apps

### Custom CDC ACM Firmware (What We Built)
- Full USB CDC ACM implementation on nRF52840
- Serial port appears as /dev/cu.usbmodemF4725FFE7DC71
- Accessible without sudo on macOS
- Protocol: COBS+CRC8 framing over serial
- App protocol frame: [0x51, seq, cmd, payload_len, payload..., crc8]
- Commands:
  - 0x52 (R) = Status → responds 0x53 with version, emmc_ok
  - 0x53 (S) = Read sector
  - 0x57 (W) = Write sector
  - 0x49 (I) = Info
  - 0x42 (B) = Reboot to bootloader (added in latest firmware, not yet flashed)

### Bootloader Protocol (solderless.engineering)
- Web Serial at 115200 baud, COBS+CRC8 framing
- Simple frame: [cmd, ..., crc8] (Dallas/Maxim CRC8, poly 0x8C)
- Commands:
  - R (0x52) = Status
  - F (0x46) = Format/erase
  - E (0x45) = Write chunk (240-byte chunks within 4096-byte pages)
  - H (0x48) = Commit/finalize
- Flash region: 0x20000-0xFF000, 4096-byte pages
- **SP Validation**: Bootloader checks `(SP & 0x2FFE0000) == 0x20000000`. SP must be 0x2001FFF0 (not 0x20040000)
- Same VID/PID and endpoint layout as app firmware
- Bootloader jumps to app very quickly - hard to catch

### Flashing History
- Successfully flashed custom firmware via libusb (not pyserial)
- macOS serial driver fails with "Device not configured" but libusb bulk transfers work with sudo
- Must detach kernel driver with sudo, set line coding (115200 8N1), set DTR+RTS
- Flash sequence: Status(0x52) → Format(0x46) → WriteChunks(0x45) → Commit(0x48) → Finalize(0x48)
- Firmware binary: ~/Downloads/stem-player-firmware.bin (39KB, 10 pages, 172 chunks)
- Flash scripts:
  - `/tmp/flash_bootloader.py` - correct bootloader protocol
  - `/tmp/flash_firmware_libusb.py` - was using app protocol (wrong), needs update

## Firmware Details

### Source
- `/private/tmp/TE-StemPlayer/firmware/main.c` - main firmware source
- Built with nRF5 SDK 17.1.0, ARM GCC 13.2
- Linker script: `firmware/armgcc/stemplayer_gcc_nrf52.ld`

### eMMC Driver (Bit-Bang, 1-bit mode)
- GPIO bit-banged: CLK, CMD, DAT0 pins
- **Known bugs in OLD firmware (currently running on device)**:
  - Wrong CRC7 calculation
  - Wrong response parsing for CMD1 (R3) and CMD2 (R2) - uses R1 parser for all
  - SECTOR_SIZE 8192 passed to CMD16 (eMMC max is 512)
  - No proper inter-command delays
- **Fixes in NEW firmware (built but not yet flashed)**:
  - Proper CRC7 (bit-by-bit with poly 0x09)
  - Separate emmc_recv_r3() for CMD1 (48-bit, no CRC) and emmc_recv_r2() for CMD2/CMD9 (136-bit)
  - CMD16 set to 512 bytes
  - Read/write loops 16x 512-byte blocks per 8192-byte app sector
  - Reboot-to-bootloader command (0x42) via GPREGRET + NVIC_SystemReset

### Current Device State
- Running OLD firmware v1 with emmc_ok=0 (eMMC not initializing)
- New firmware compiled (39KB .bin) with all eMMC fixes
- **BLOCKED**: Cannot flash new firmware because:
  - Old firmware doesn't have reboot-to-bootloader command (0x42)
  - Bootloader jumps to app too fast to catch via USB
  - libusb bulk writes to bootloader get pipe error
  - pyserial gets "Device not configured" on macOS
  - Need user to physically enter bootloader: touch top slider while plugging USB-C

## App Architecture

### Rust Backend (src-tauri/)
- `src/main.rs` - Tauri app entry
- `src/lib.rs` - All Tauri commands
- `src/usb/mod.rs` - USB/serial communication (CDC ACM with COBS+CRC8)
- `src/audio/mod.rs` - Sector packing, format conversion
- Key commands: connect_device, read_sectors, write_sectors, start_separation, write_album_metadata, upload_song, flash_firmware
- Device capability probing on connect (tests vendor control transfer, sets needs_firmware_update flag)
- Auto firmware flash wizard triggers on connect when update needed

### React Frontend (src/)
- `src/App.tsx` - Main app with tab navigation
- `src/components/DeviceManager/` - Device connection, status display
- `src/components/StemCreator/` - YouTube URL → stem separation workflow
- `src/components/AlbumManager/` - Album/song management on device
- `src/components/FirmwareFlasher/` - Step-by-step firmware flash wizard
- `src/hooks/useDevice.ts` - Device connection hook with capability probing
- `src/stores/` - Zustand stores
- `src/utils/tauriCommands.ts` - Tauri command wrappers

### Python Sidecars (sidecar/)
- `stem_separator.py` - Demucs integration with progress events
- `downloader.py` - yt-dlp with multi-strategy fallback (legacy-server-connect, format 18, browser cookies)
- Both get /opt/homebrew/bin on PATH via env var in lib.rs spawn calls

## Build & Run
```bash
# Prerequisites
export PATH="/tmp/nodeenv/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

# Run app
cd /private/tmp/TE-StemPlayer
npm run tauri dev
# Vite on localhost:1420, Rust backend compiles and runs

# Run tests
cargo test --manifest-path src-tauri/Cargo.toml
# All 11 tests pass
```

## Key Dependencies
- libusb: /opt/homebrew/lib/libusb-1.0.dylib
- pyusb, pyserial: installed via pip3
- demucs, torch: installed
- ffmpeg: /opt/homebrew/bin/ffmpeg
- ARM GCC: arm-none-eabi-gcc 13.2
- nRF5 SDK: v17.1.0

## Git History (Key Commits)
1. Initial docs (7 files)
2. Tauri scaffold + Rust backend + sidecar
3. GPT55's React frontend
4. DeepPro's enhanced audio pipeline
5. `b5b697d` - Major bug fix (all P0/P1 bugs, 11 tests pass)
6. `c1ab479` - USB pipe error fix + downloader CLI fix
7. `bac1e79` - USB chunk fix + demucs/torch install
8. `4082264` - Full-speed USB fix (64-byte chunks) + YouTube 403 fix
9. `6419f27` - Additional integration fixes
10. `6fbcfe6` - Custom nRF52840 firmware + flash scripts
11. `6fb3870` - Device capability probing + graceful error handling
12. `7e68d72` - Auto firmware flash wizard
13. `2da4793` - Bootloader entry instructions
14. `986f73a` - Fixed firmware SP validation (0x2001FFF0)
15. `471e5a4` - Fixed flash protocol (drain stale responses)
16. `4c9b07c` - Rewrote USB module to serial CDC ACM with COBS+CRC8

## Known Issues & Next Steps
1. **eMMC not initializing** (emmc_ok=0) - Fixed in new firmware binary but can't flash it
2. **Bootloader access** - Need physical bootloader entry (touch top slider while plugging USB-C) or try solderless.engineering web flasher
3. **YouTube 403** - May persist on Python 3.9 due to SABR issue (needs Python 3.10+ for yt-dlp nightly)
4. Minor TS type warning in albumStore (non-blocking)

## Key Resources
- Dev docs: github.com/timknapen/SP-1-dev
- Reverse engineering: github.com/leabs/yzy-stemplayer-reverse
- Firmware upload: solderless.engineering
- Root password: @!nt3rn3t (for sudo)
