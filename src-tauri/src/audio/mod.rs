//! Audio sector packing for TE-StemPlayer SP-1.
//!
//! Implements the raw sector format: 8192 bytes per sector,
//! 340 audio frames at 24-bit/48kHz stereo, plus timing and LED data.
//! Supports 4-stem interleaving (vocals, drums, bass, other).

pub mod metadata;

/// Number of audio frames per sector.
pub const FRAMES_PER_SECTOR: usize = 340;

/// Size of an audio frame in bytes.
pub const FRAME_SIZE: usize = 6;

/// Total sector size in bytes.
pub const SECTOR_SIZE: usize = 8192;

/// Offset of timing data within sector (bytes 2040-2043).
pub const TIMING_OFFSET: usize = 2040;

/// Offset of LED data within sector (bytes 2044-2047).
pub const LED_OFFSET: usize = 2044;

/// Number of audio data bytes per sector (340 * 6).
pub const AUDIO_DATA_SIZE: usize = FRAMES_PER_SECTOR * FRAME_SIZE; // 2040

/// Number of stems in a full set.
pub const STEM_COUNT: usize = 4;

/// Stem labels in interleave order.
pub const STEM_ORDER: [&str; 4] = ["vocals", "drums", "bass", "other"];


/// Encode a stereo sample pair into a 6-byte frame.
///
/// Frame byte order (from SECTOR_FORMAT.md):
///   byte 0: L_MB  (left middle byte)
///   byte 1: L_MSB (left most significant byte)
///   byte 2: R_MSB (right most significant byte)
///   byte 3: L_LSB (left least significant byte)
///   byte 4: R_LSB (right least significant byte)
///   byte 5: R_MB  (right middle byte)
///
/// Samples are 24-bit signed integers (-8_388_608..8_388_607).
#[inline]
pub fn encode_frame(left: i32, right: i32) -> [u8; 6] {
    // Clamp to 24-bit signed range
    let left = left.clamp(-8_388_608, 8_388_607);
    let right = right.clamp(-8_388_608, 8_388_607);

    [
        ((left >> 8) & 0xFF) as u8,   // L_MB
        ((left >> 16) & 0xFF) as u8,  // L_MSB
        ((right >> 16) & 0xFF) as u8, // R_MSB
        (left & 0xFF) as u8,          // L_LSB
        (right & 0xFF) as u8,         // R_LSB
        ((right >> 8) & 0xFF) as u8,  // R_MB
    ]
}


/// Decode a 6-byte frame into (left, right) 32-bit signed samples.
/// Result is sign-extended from 24-bit.
#[inline]
pub fn decode_frame(frame: &[u8; 6]) -> (i32, i32) {
    let left = ((frame[1] as i32) << 16)   // L_MSB
             | ((frame[0] as i32) << 8)    // L_MB
             | (frame[3] as i32);          // L_LSB

    let right = ((frame[2] as i32) << 16)  // R_MSB
              | ((frame[5] as i32) << 8)   // R_MB
              | (frame[4] as i32);         // R_LSB

    // Sign-extend from 24-bit
    let left = (left << 8) >> 8;
    let right = (right << 8) >> 8;

    (left, right)
}


/// A single 8192-byte sector containing up to 340 audio frames.
#[derive(Debug, Clone)]
pub struct Sector {
    pub data: [u8; SECTOR_SIZE],
    /// Actual number of frames stored (≤ 340).
    pub frame_count: usize,
}


impl Sector {
    /// Create a new zero-filled sector.
    pub fn new() -> Self {
        Self {
            data: [0u8; SECTOR_SIZE],
            frame_count: 0,
        }
    }

    /// Create a sector from frames for a single stem.
    ///
    /// `frames`: slice of (left, right) sample pairs.
    /// `timing`: cumulative frame index (u32 big-endian).
    /// `led`: 4-byte LED control word (R, G, B, mode); default [0,0,0,0] = off.
    ///
    /// # Panics
    /// Panics if more than 340 frames are provided.
    pub fn from_frames(
        frames: &[(i32, i32)],
        timing: u32,
        led: [u8; 4],
    ) -> Self {
        assert!(frames.len() <= FRAMES_PER_SECTOR, "Max 340 frames per sector");

        let mut sector = Self::new();
        sector.frame_count = frames.len();

        for (i, &(left, right)) in frames.iter().enumerate() {
            let frame_bytes = encode_frame(left, right);
            let offset = i * FRAME_SIZE;
            sector.data[offset..offset + FRAME_SIZE].copy_from_slice(&frame_bytes);
        }

        // Timing data: big-endian u32 at offset 2040
        sector.data[TIMING_OFFSET..TIMING_OFFSET + 4]
            .copy_from_slice(&timing.to_be_bytes());

        // LED data at offset 2044
        sector.data[LED_OFFSET..LED_OFFSET + 4].copy_from_slice(&led);

        sector
    }

    /// Read timing value (big-endian u32).
    pub fn timing(&self) -> u32 {
        u32::from_be_bytes([
            self.data[TIMING_OFFSET],
            self.data[TIMING_OFFSET + 1],
            self.data[TIMING_OFFSET + 2],
            self.data[TIMING_OFFSET + 3],
        ])
    }

    /// Read LED control word.
    pub fn led(&self) -> [u8; 4] {
        [
            self.data[LED_OFFSET],
            self.data[LED_OFFSET + 1],
            self.data[LED_OFFSET + 2],
            self.data[LED_OFFSET + 3],
        ]
    }

    /// Decode all frames from this sector.
    pub fn read_frames(&self) -> Vec<(i32, i32)> {
        let mut frames = Vec::with_capacity(self.frame_count);
        for i in 0..self.frame_count {
            let offset = i * FRAME_SIZE;
            let frame: [u8; 6] = self.data[offset..offset + FRAME_SIZE]
                .try_into()
                .unwrap();
            frames.push(decode_frame(&frame));
        }
        frames
    }
}


impl Default for Sector {
    fn default() -> Self {
        Self::new()
    }
}


/// Pack 4 stereo stems into interleaved sectors.
///
/// # Arguments
/// * `stems` - Array of 4 stems, each a slice of (L, R) sample pairs.
/// * `led_data` - Optional LED configuration per sector.
///
/// # Returns
/// Vector of sectors in interleave order:
///   Sector 0: vocals[0..340]
///   Sector 1: drums[0..340]
///   Sector 2: bass[0..340]
///   Sector 3: other[0..340]
///   Sector 4: vocals[340..680]
///   ...
///
/// The shortest stem determines total length. Partial final blocks
/// (fewer than 340 frames) are supported.
pub fn pack_stems(
    stems: &[&[(i32, i32)]; 4],
    led_data: Option<&[[u8; 4]]>,
) -> Vec<Sector> {
    let min_frames = stems.iter().map(|s| s.len()).min().unwrap_or(0);
    if min_frames == 0 {
        return Vec::new();
    }

    let num_blocks = min_frames.div_ceil(FRAMES_PER_SECTOR);
    let total_sectors = num_blocks * STEM_COUNT;
    let mut sectors = Vec::with_capacity(total_sectors);

    for block in 0..num_blocks {
        let frame_start = block * FRAMES_PER_SECTOR;
        let frame_end = ((block + 1) * FRAMES_PER_SECTOR).min(min_frames);

        // Timing: cumulative frame index at block start
        let timing = (block * FRAMES_PER_SECTOR) as u32;

        for stem_idx in 0..STEM_COUNT {
            let stem_frames = &stems[stem_idx][frame_start..frame_end];

            let led = if let Some(led_arr) = led_data {
                let sector_idx = block * STEM_COUNT + stem_idx;
                if sector_idx < led_arr.len() {
                    led_arr[sector_idx]
                } else {
                    [0u8; 4]
                }
            } else {
                [0u8; 4]
            };

            sectors.push(Sector::from_frames(stem_frames, timing, led));
        }
    }

    sectors
}


/// Convert interleaved i32 samples to (left, right) pairs.
pub fn interleaved_to_pairs(samples: &[i32]) -> Vec<(i32, i32)> {
    samples
        .chunks_exact(2)
        .map(|chunk| (chunk[0], chunk[1]))
        .collect()
}


/// Convert (left, right) pairs to interleaved i32 samples.
pub fn pairs_to_interleaved(pairs: &[(i32, i32)]) -> Vec<i32> {
    let mut out = Vec::with_capacity(pairs.len() * 2);
    for &(l, r) in pairs {
        out.push(l);
        out.push(r);
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_roundtrip() {
        let cases = [
            (0, 0),
            (1, -1),
            (8_388_607, -8_388_608),
            (0x7F_AABB, 0x40_1122),
        ];
        for (left, right) in cases {
            let frame = encode_frame(left, right);
            let (dl, dr) = decode_frame(&frame);
            assert_eq!(left, dl, "left mismatch for ({},{})", left, right);
            assert_eq!(right, dr, "right mismatch for ({},{})", left, right);
        }
    }

    #[test]
    fn test_encode_byte_order() {
        // Left = 0x7F_AABB, Right = 0x40_1122
        // Expected: L_MB=0xAA, L_MSB=0x7F, R_MSB=0x40, L_LSB=0xBB, R_LSB=0x22, R_MB=0x11
        let f = encode_frame(0x7F_AABB, 0x40_1122);
        assert_eq!([f[0], f[1], f[2], f[3], f[4], f[5]], [0xAA, 0x7F, 0x40, 0xBB, 0x22, 0x11]);
    }

    #[test]
    fn test_sector_timing_led() {
        let frames = vec![(0i32, 0i32); 340];
        let led = [0xAA, 0xBB, 0xCC, 0x01];
        let sector = Sector::from_frames(&frames, 0x12345678, led);
        assert_eq!(sector.timing(), 0x12345678);
        assert_eq!(sector.led(), led);
    }

    #[test]
    fn test_pack_stems_interleave() {
        let vocals: Vec<(i32, i32)> = vec![(1, 1); 340];
        let drums: Vec<(i32, i32)> = vec![(2, 2); 340];
        let bass: Vec<(i32, i32)> = vec![(3, 3); 340];
        let other: Vec<(i32, i32)> = vec![(4, 4); 340];
        let stems: [&[(i32, i32)]; 4] = [&vocals, &drums, &bass, &other];
        let sectors = pack_stems(&stems, None);

        assert_eq!(sectors.len(), 4);
        let (l0, r0) = decode_frame(&sectors[0].data[0..6].try_into().unwrap());
        assert_eq!((l0, r0), (1, 1)); // vocals
        let (l1, r1) = decode_frame(&sectors[1].data[0..6].try_into().unwrap());
        assert_eq!((l1, r1), (2, 2)); // drums
    }

    #[test]
    fn test_interleaved_conversion() {
        let samples = vec![1i32, 2, 3, 4];
        let pairs = interleaved_to_pairs(&samples);
        assert_eq!(pairs, vec![(1, 2), (3, 4)]);
        assert_eq!(pairs_to_interleaved(&pairs), samples);
    }
}
