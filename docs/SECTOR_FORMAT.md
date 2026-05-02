# Sector Format — TE Stem Player (SP-1)

## Overview

The SP-1 stores audio as raw sectors on a 4GB eMMC chip (Toshiba THGBMNG5D1LBAIL, eMMC 5.0). There is **no filesystem** — all data is stored as contiguous raw sectors.

## Sector Structure

Each sector is **8192 bytes** and contains **340 audio frames** plus metadata.

```
┌────────────────────────────────────────────────┐
│ Sector (8192 bytes)                            │
├────────────────────────────────────────────────┤
│ Bytes 0–2039      │ 340 audio frames (6 bytes) │
│ Bytes 2040–2043   │ Timing data (u32 BE)       │
│ Bytes 2044–2047   │ LED data (u32)             │
│ Bytes 2048–8191   │ Reserved / padding         │
└────────────────────────────────────────────────┘
```

**Timing**: Each sector represents **7.083 ms** of audio (340 frames at 48kHz).

## Audio Frame Format

Each frame is **6 bytes** encoding one sample across one stereo stem pair at **24-bit, 48kHz**.

```
Byte 0: L_MB   (Left middle byte)
Byte 1: L_MSB  (Left most significant byte)
Byte 2: R_MSB  (Right most significant byte)
Byte 3: L_LSB  (Left least significant byte)
Byte 4: R_LSB  (Right least significant byte)
Byte 5: R_MB   (Right middle byte)
```

### Reconstructing 24-bit Samples

```rust
fn decode_frame(frame: &[u8; 6]) -> (i32, i32) {
    // Left channel: LSB | MB | MSB
    let left = ((frame[1] as i32) << 16)   // L_MSB
             | ((frame[0] as i32) << 8)    // L_MB
             | (frame[3] as i32);          // L_LSB

    // Right channel: LSB | MB | MSB
    let right = ((frame[2] as i32) << 16)  // R_MSB
              | ((frame[5] as i32) << 8)   // R_MB
              | (frame[4] as i32);         // R_LSB

    // Sign extend from 24-bit to 32-bit
    let left = (left << 8) >> 8;
    let right = (right << 8) >> 8;

    (left, right)
}
```

### Encoding 24-bit Samples

```rust
fn encode_frame(left: i32, right: i32) -> [u8; 6] {
    [
        ((left >> 8) & 0xFF) as u8,   // L_MB
        ((left >> 16) & 0xFF) as u8,  // L_MSB
        ((right >> 16) & 0xFF) as u8, // R_MSB
        (left & 0xFF) as u8,          // L_LSB
        (right & 0xFF) as u8,         // R_LSB
        ((right >> 8) & 0xFF) as u8,  // R_MB
    ]
}
```

## Multi-Stem Interleaving

The SP-1 supports **4 stereo stems** = **8 channels** total. Stems are interleaved at the sector level:

```
Sector N+0: Stem 1 frames (vocals)     — 340 frames
Sector N+1: Stem 2 frames (drums)      — 340 frames
Sector N+2: Stem 3 frames (bass)       — 340 frames
Sector N+3: Stem 4 frames (other)      — 340 frames
Sector N+4: Stem 1 frames (next chunk) — 340 frames
...
```

Each group of 4 consecutive sectors represents one **stem block** covering the same 7.083ms window.

### Stem Packing Algorithm

```rust
fn pack_stems(
    vocals: &[Vec<(i32, i32)>],   // Per-sector frame lists
    drums: &[Vec<(i32, i32)>],
    bass: &[Vec<(i32, i32)>],
    other: &[Vec<(i32, i32)>],
) -> Vec<[u8; 8192]> {
    let mut sectors = Vec::new();
    let stems = [vocals, drums, bass, other];
    let num_blocks = vocals.len();

    for block in 0..num_blocks {
        for stem in &stems {
            let mut sector = [0u8; 8192];
            for (i, &(left, right)) in stem[block].iter().enumerate() {
                let frame = encode_frame(left, right);
                sector[i * 6..i * 6 + 6].copy_from_slice(&frame);
            }
            // Timing data at offset 2040
            let timing = (block as u32) * 340;
            sector[2040..2044].copy_from_slice(&timing.to_be_bytes());
            // LED data at offset 2044 (default: off)
            sector[2044..2048].copy_from_slice(&[0u8; 4]);
            sectors.push(sector);
        }
    }
    sectors
}
```

## Timing Data (Bytes 2040–2043)

A **u32 big-endian** value representing the cumulative frame index at the start of this sector. Used for seek operations and playback synchronization.

```
Sector 0: timing = 0
Sector 1: timing = 340
Sector 2: timing = 680
...
```

## LED Data (Bytes 2044–2047)

Controls the device's LED ring during playback.

```
Byte 2044: Red (0–255)
Byte 2045: Green (0–255)
Byte 2046: Blue (0–255)
Byte 2047: Mode (0x00=off, 0x01=solid, 0x02=pulse, 0x03=reactive)
```

## Storage Calculations

| Metric | Value |
|--------|-------|
| eMMC capacity | 4 GB (4,194,304 sectors at 8192 bytes... ~512K sectors) |
| Sector duration | 7.083 ms (340 frames at 48kHz) |
| Sectors per second | ~141 (4 stems × ~35.3 sectors/stem/second) |
| Storage per minute | ~67.5 MB (141 sectors × 8192 bytes × 60) |
| Max song duration | ~59 minutes (4GB / 67.5 MB/min) |
| Typical 4-min song | ~270 MB (~33,000 sectors) |

## Validation Checklist

Before writing sectors to device:
1. ✅ Sector size is exactly 8192 bytes
2. ✅ Frame count ≤ 340 per sector
3. ✅ Audio is 48kHz, 24-bit
4. ✅ Timing bytes increment correctly
5. ✅ Total sectors fit within available eMMC space
6. ✅ Album metadata updated with correct song offset and length
