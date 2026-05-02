const SECTOR_SIZE: usize = 8192;
const FRAMES_PER_SECTOR: usize = 340;
const CHANNELS: usize = 8; // 4 stereo stems
const BYTES_PER_FRAME: usize = CHANNELS * 3; // 24-bit = 3 bytes per sample, but interleaved format uses 6 bytes per stereo pair
const STEM_FRAME_SIZE: usize = 6; // L_MB, L_MSB, R_MSB, L_LSB, R_LSB, R_MB per stereo pair

/// Pack 4 stereo stem audio buffers into sector format
/// Each stem is a Vec of interleaved L/R 24-bit samples
/// Frame byte order per stereo pair: L_MB, L_MSB, R_MSB, L_LSB, R_LSB, R_MB
pub fn pack_stems_to_sectors(
    stems: &[Vec<i32>; 4], // 4 stems, each with interleaved L/R 24-bit samples
    total_frames: usize,
) -> Vec<Vec<u8>> {
    let num_sectors = (total_frames + FRAMES_PER_SECTOR - 1) / FRAMES_PER_SECTOR;
    let mut sectors = Vec::with_capacity(num_sectors);

    for sector_idx in 0..num_sectors {
        let mut sector = vec![0u8; SECTOR_SIZE];
        let start_frame = sector_idx * FRAMES_PER_SECTOR;

        for frame in 0..FRAMES_PER_SECTOR {
            let global_frame = start_frame + frame;
            if global_frame >= total_frames {
                break;
            }

            let byte_offset = frame * STEM_FRAME_SIZE * 4; // 4 stems

            for stem in 0..4 {
                let sample_idx = global_frame * 2; // L and R
                let (left, right) = if sample_idx + 1 < stems[stem].len() {
                    (stems[stem][sample_idx], stems[stem][sample_idx + 1])
                } else {
                    (0, 0)
                };

                // Extract bytes from 24-bit samples
                let l_lsb = (left & 0xFF) as u8;
                let l_mb = ((left >> 8) & 0xFF) as u8;
                let l_msb = ((left >> 16) & 0xFF) as u8;
                let r_lsb = (right & 0xFF) as u8;
                let r_mb = ((right >> 8) & 0xFF) as u8;
                let r_msb = ((right >> 16) & 0xFF) as u8;

                // Frame byte order: L_MB, L_MSB, R_MSB, L_LSB, R_LSB, R_MB
                let off = byte_offset + stem * STEM_FRAME_SIZE;
                if off + 5 < 2040 {
                    sector[off] = l_mb;
                    sector[off + 1] = l_msb;
                    sector[off + 2] = r_msb;
                    sector[off + 3] = l_lsb;
                    sector[off + 4] = r_lsb;
                    sector[off + 5] = r_mb;
                }
            }
        }

        // Timing data at bytes 2040-2043
        let sector_time_ms = (sector_idx as u32) * 7; // ~7.083ms per sector
        sector[2040] = (sector_time_ms & 0xFF) as u8;
        sector[2041] = ((sector_time_ms >> 8) & 0xFF) as u8;
        sector[2042] = ((sector_time_ms >> 16) & 0xFF) as u8;
        sector[2043] = ((sector_time_ms >> 24) & 0xFF) as u8;

        // LED data at bytes 2044-2047 (default off)
        sector[2044] = 0;
        sector[2045] = 0;
        sector[2046] = 0;
        sector[2047] = 0;

        sectors.push(sector);
    }

    sectors
}
