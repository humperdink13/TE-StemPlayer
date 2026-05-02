const SECTOR_SIZE: usize = 8192;
const BLOCK_SIZE: usize = 2048;
const FRAMES_PER_BLOCK: usize = 340;
const STEM_FRAME_SIZE: usize = 6;

/// Block ordering: frames are distributed non-sequentially across blocks
const BLOCK_ORDER: [usize; 4] = [0, 2, 1, 3];

/// Encode a single stereo sample pair (24-bit) into the 6-byte stem frame format
/// Format: L_MB, L_MSB, R_MSB, L_LSB, R_LSB, R_MB
pub fn encode_stem_frame(left: i32, right: i32) -> [u8; 6] {
    let l_lsb = (left & 0xFF) as u8;
    let l_mb = ((left >> 8) & 0xFF) as u8;
    let l_msb = ((left >> 16) & 0xFF) as u8;
    let r_lsb = (right & 0xFF) as u8;
    let r_mb = ((right >> 8) & 0xFF) as u8;
    let r_msb = ((right >> 16) & 0xFF) as u8;
    [l_mb, l_msb, r_msb, l_lsb, r_lsb, r_mb]
}

/// Decode a 6-byte stem frame back to stereo 24-bit samples
pub fn decode_stem_frame(frame: &[u8; 6]) -> (i32, i32) {
    let left = ((frame[1] as i32) << 24)
             | ((frame[0] as i32) << 16)
             | ((frame[3] as i32) << 8);
    let left = left >> 8; // sign-extend to 24-bit

    let right = ((frame[2] as i32) << 24)
              | ((frame[5] as i32) << 16)
              | ((frame[4] as i32) << 8);
    let right = right >> 8;

    (left, right)
}

/// Pack 4 stereo stem audio buffers into sectors with correct block interleaving.
///
/// Each stem is a Vec of interleaved L/R 24-bit samples.
/// Returns a Vec of 8192-byte sectors ready to write to the device.
pub fn pack_stems_to_sectors(
    stems: &[Vec<i32>; 4],
    total_frames: usize,
) -> Vec<Vec<u8>> {
    // Each sector holds FRAMES_PER_BLOCK * 4 = 1360 frames (340 per block)
    let frames_per_sector = FRAMES_PER_BLOCK * 4;
    let num_sectors = (total_frames + frames_per_sector - 1) / frames_per_sector;
    let mut sectors = Vec::with_capacity(num_sectors);

    for sector_idx in 0..num_sectors {
        let mut sector = vec![0u8; SECTOR_SIZE];
        let sector_start_frame = sector_idx * frames_per_sector;

        // Track how many frames written to each block
        let mut block_frame_count = [0usize; 4];

        for frame_idx in 0..frames_per_sector {
            let global_frame = sector_start_frame + frame_idx;
            if global_frame >= total_frames {
                break;
            }

            // Determine which block this frame goes to using interleave order
            let block_select = frame_idx % 4;
            let block_idx = BLOCK_ORDER[block_select];
            let frame_in_block = block_frame_count[block_idx];
            block_frame_count[block_idx] += 1;

            if frame_in_block >= FRAMES_PER_BLOCK {
                continue;
            }

            // Calculate byte offset within the sector
            let block_offset = block_idx * BLOCK_SIZE;
            // Each frame position holds all 4 stems: 4 * 6 = 24 bytes
            let frame_offset = frame_in_block * STEM_FRAME_SIZE * 4;

            for stem in 0..4 {
                let sample_idx = global_frame * 2; // L and R interleaved
                let (left, right) = if sample_idx + 1 < stems[stem].len() {
                    (stems[stem][sample_idx], stems[stem][sample_idx + 1])
                } else {
                    (0, 0)
                };

                let encoded = encode_stem_frame(left, right);
                let off = block_offset + frame_offset + stem * STEM_FRAME_SIZE;
                if off + 5 < block_offset + 2040 {
                    sector[off..off + 6].copy_from_slice(&encoded);
                }
            }
        }

        // Write timing/sync, tempo, and LED data for each block
        for block_idx in 0..4 {
            let block_offset = block_idx * BLOCK_SIZE;
            let block_time_ms = (sector_idx as u32) * 28 + (block_idx as u32) * 7;

            // Timing data at bytes 2040-2041 (u16 LE)
            sector[block_offset + 2040] = (block_time_ms & 0xFF) as u8;
            sector[block_offset + 2041] = ((block_time_ms >> 8) & 0xFF) as u8;

            // Tempo at bytes 2042-2043 (u16, default 0)
            sector[block_offset + 2042] = 0;
            sector[block_offset + 2043] = 0;

            // LED data at bytes 2044-2047 (R, G, B, Mode)
            sector[block_offset + 2044] = 0;
            sector[block_offset + 2045] = 0;
            sector[block_offset + 2046] = 0;
            sector[block_offset + 2047] = 0;
        }

        sectors.push(sector);
    }

    sectors
}

/// Unpack sectors back to 4 stem audio buffers (for verification/playback)
pub fn unpack_sectors_to_stems(
    sectors: &[Vec<u8>],
) -> [Vec<i32>; 4] {
    let mut stems: [Vec<i32>; 4] = [vec![], vec![], vec![], vec![]];
    let frames_per_sector = FRAMES_PER_BLOCK * 4;

    for sector in sectors {
        if sector.len() != SECTOR_SIZE {
            continue;
        }

        let mut block_frame_idx = [0usize; 4];

        for frame_idx in 0..frames_per_sector {
            let block_select = frame_idx % 4;
            let block_idx = BLOCK_ORDER[block_select];
            let frame_in_block = block_frame_idx[block_idx];
            block_frame_idx[block_idx] += 1;

            if frame_in_block >= FRAMES_PER_BLOCK {
                continue;
            }

            let block_offset = block_idx * BLOCK_SIZE;
            let frame_offset = frame_in_block * STEM_FRAME_SIZE * 4;

            for stem in 0..4 {
                let off = block_offset + frame_offset + stem * STEM_FRAME_SIZE;
                if off + 5 < block_offset + 2040 {
                    let frame_bytes: [u8; 6] = [
                        sector[off], sector[off + 1], sector[off + 2],
                        sector[off + 3], sector[off + 4], sector[off + 5],
                    ];
                    let (left, right) = decode_stem_frame(&frame_bytes);
                    stems[stem].push(left);
                    stems[stem].push(right);
                } else {
                    stems[stem].push(0);
                    stems[stem].push(0);
                }
            }
        }
    }

    stems
}
