# Sector Format — TE Stem Player (SP-1)

> ⚠️ **Corrected**: This document has been updated based on findings from the
> [SP-1-dev wiki](https://github.com/timknapen/SP-1-dev/wiki/Audio-format).
> See [RESEARCH.md](./RESEARCH.md) for full research notes.

## Overview

The SP-1 stores audio as raw sectors on a 4GB eMMC chip (Toshiba THGBMNG5D1LBAIL, eMMC 5.0). There is **no filesystem** — all data is stored as contiguous raw sectors.

## Sector Structure

Each sector is **8192 bytes** divided into **4 blocks of 2048 bytes**. All 4 stems are packed into a single sector (not spread across consecutive sectors).

```
┌─────────────────────────────────────────────────────────────┐
│ Sector (8192 bytes)                                         │
├─────────────────────────────────────────────────────────────┤
│ Block 0: bytes 0–2047                                       │
│ Block 1: bytes 2048–4095                                    │
│ Block 2: bytes 4096–6143                                    │
│ Block 3: bytes 6144–8191                                    │
└─────────────────────────────────────────────────────────────┘
```

### Block Layout (2048 bytes each)

```
Bytes 0–2039:    340 stem frames × 6 bytes = 2040 bytes
Bytes 2040–2041: Timing/sync data (u16)
Bytes 2042–2043: Tempo data (u16)
Bytes 2044–2047: LED data (R, G, B, Mode)
```

## Frame Interleaving

Each **audio frame** is 24 bytes, containing one sample for all 4 stems:

```
Audio Frame (24 bytes):
  Bytes  0–5:  Stem 0 (vocals)  — 6-byte stereo sample
  Bytes  6–11: Stem 1 (drums)   — 6-byte stereo sample
  Bytes 12–17: Stem 2 (bass)    — 6-byte stereo sample
  Bytes 18–23: Stem 3 (other)   — 6-byte stereo sample
```

### Block Ordering (Non-Sequential!)

Frames are distributed across blocks in a **non-sequential** order:

```
block_order[] = {0, 2, 1, 3}
```

For audio frame index `i`:
- `i % 4 == 0` → Block 0 (bytes 0–2047)
- `i % 4 == 1` → Block 2 (bytes 4096–6143)
- `i % 4 == 2` → Block 1 (bytes 2048–4095)
- `i % 4 == 3` → Block 3 (bytes 6144–8191)

Each block holds **340 stem frames** per stem, so each sector represents **340 audio samples** across all 4 stems.

**Sector duration**: 340 frames at 48kHz = **7.083 ms**

## Stem Frame Format (6 bytes)

Each stem frame encodes one stereo sample at 24-bit depth:

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
fn decode_stem_frame(frame: &[u8; 6]) -> (i32, i32) {
    // Left channel: sign-extend via shift-up then shift-down
    let left = ((frame[1] as i32) << 24)   // L_MSB → bits 31-24
             | ((frame[0] as i32) << 16)   // L_MB  → bits 23-16
             | ((frame[3] as i32) << 8);   // L_LSB → bits 15-8
    let left = left >> 8; // Sign-extend to 24-bit

    // Right channel
    let right = ((frame[2] as i32) << 24)  // R_MSB → bits 31-24
              | ((frame[5] as i32) << 16)  // R_MB  → bits 23-16
              | ((frame[4] as i32) << 8);  // R_LSB → bits 15-8
    let right = right >> 8;

    (left, right)
}
```

### Encoding 24-bit Samples

```rust
fn encode_stem_frame(left: i32, right: i32) -> [u8; 6] {
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

## Correct Stem Packing Algorithm

```rust
const BLOCK_ORDER: [usize; 4] = [0, 2, 1, 3];
const FRAMES_PER_BLOCK: usize = 340;
const BLOCK_SIZE: usize = 2048;

/// Pack interleaved stem samples into raw sectors.
///
/// Input: 4 stems, each a Vec of stereo (i32, i32) samples at 48kHz 24-bit.
/// Output: Vec of 8192-byte sectors ready to write to eMMC.
fn pack_stems(
    stems: [&[(i32, i32)]; 4],  // [vocals, drums, bass, other]
) -> Vec<[u8; 8192]> {
    let num_samples = stems[0].len();
    let num_sectors = (num_samples + FRAMES_PER_BLOCK - 1) / FRAMES_PER_BLOCK;
    let mut sectors = Vec::with_capacity(num_sectors);

    for sector_idx in 0..num_sectors {
        let mut sector = [0u8; 8192];
        let base_sample = sector_idx * FRAMES_PER_BLOCK;

        for frame_in_block in 0..FRAMES_PER_BLOCK {
            let sample_idx = base_sample + frame_in_block;
            if sample_idx >= num_samples {
                break;
            }

            // Determine which block this frame goes into
            let block_idx = BLOCK_ORDER[frame_in_block % 4];
            let pos_in_block = (frame_in_block / 4) * 24; // 24 bytes per audio frame
            let byte_offset = block_idx * BLOCK_SIZE + pos_in_block;

            // Pack all 4 stems into one 24-byte audio frame
            for (stem_i, stem) in stems.iter().enumerate() {
                let (left, right) = if sample_idx < stem.len() {
                    stem[sample_idx]
                } else {
                    (0, 0)
                };
                let frame = encode_stem_frame(left, right);
                let offset = byte_offset + stem_i * 6;
                sector[offset..offset + 6].copy_from_slice(&frame);
            }
        }

        // Timing data for each block
        for block in 0..4 {
            let offset = block * BLOCK_SIZE;
            let timing = ((sector_idx * FRAMES_PER_BLOCK) as u16).to_be_bytes();
            sector[offset + 2040] = timing[0];
            sector[offset + 2041] = timing[1];
            // Tempo: 0 for now
            sector[offset + 2042] = 0;
            sector[offset + 2043] = 0;
            // LED: off
            sector[offset + 2044..offset + 2048].copy_from_slice(&[0, 0, 0, 0]);
        }

        sectors.push(sector);
    }

    sectors
}
```

## Timing Data (Bytes 2040–2041 per block)

A **u16 big-endian** value for timing/synchronization. Present in each of the 4 blocks within a sector.

## Tempo Data (Bytes 2042–2043 per block)

A **u16** value encoding tempo information. Exact format TBD.

## LED Data (Bytes 2044–2047 per block)

Controls the device's LED ring during playback:

```
Byte 2044: Red (0–255)
Byte 2045: Green (0–255)
Byte 2046: Blue (0–255)
Byte 2047: Mode (0x00=off, 0x01=solid, 0x02=pulse, 0x03=reactive)
```

## Storage Calculations

| Metric | Value |
|--------|-------|
| eMMC capacity | 4 GB (~512K sectors at 8192 bytes) |
| Sector duration | 7.083 ms (340 frames at 48kHz) |
| Sectors per second | ~141 |
| Storage per minute | ~67.5 MB |
| Max duration | ~59 minutes (4GB / 67.5 MB/min) |
| Typical 4-min song | ~270 MB (~33,000 sectors) |

## Validation Checklist

Before writing sectors to device:
1. ✅ Sector size is exactly 8192 bytes
2. ✅ Each block has ≤ 340 stem frames (2040 bytes audio data)
3. ✅ Audio is 48kHz, 24-bit
4. ✅ Block ordering follows `{0, 2, 1, 3}` interleave pattern
5. ✅ All 4 stems present in each audio frame (24 bytes)
6. ✅ Timing bytes set correctly per block
7. ✅ Total sectors fit within available eMMC space
8. ✅ Album metadata (sector 0) updated with correct song offset and length
