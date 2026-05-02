const SECTOR_SIZE: usize = 8192;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SongEntry {
    pub start_sector: u32,
    pub sector_count: u32,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlbumMetadata {
    pub songs: Vec<SongEntry>,
}

impl AlbumMetadata {
    /// Build sector 0 metadata from album info.
    /// Format: 1 byte song count, then per-song: u32 BE start sector, u32 BE sector count, null-terminated name
    pub fn to_sector(&self) -> Vec<u8> {
        let mut sector = vec![0u8; SECTOR_SIZE];
        let mut offset = 0;

        // Song count (1 byte, max 255)
        let count = self.songs.len().min(255) as u8;
        sector[offset] = count;
        offset += 1;

        for song in &self.songs {
            if offset + 8 >= SECTOR_SIZE { break; }

            // Start sector (u32 BE)
            sector[offset..offset + 4].copy_from_slice(&song.start_sector.to_be_bytes());
            offset += 4;

            // Sector count (u32 BE)
            sector[offset..offset + 4].copy_from_slice(&song.sector_count.to_be_bytes());
            offset += 4;

            // Song name (null-terminated)
            let name_bytes = song.name.as_bytes();
            let max_name = (SECTOR_SIZE - offset - 1).min(name_bytes.len());
            sector[offset..offset + max_name].copy_from_slice(&name_bytes[..max_name]);
            offset += max_name;
            sector[offset] = 0; // null terminator
            offset += 1;
        }

        sector
    }

    /// Parse album metadata from sector 0
    pub fn from_sector(data: &[u8]) -> Result<Self, String> {
        if data.len() < SECTOR_SIZE {
            return Err("Invalid sector size".into());
        }

        let song_count = data[0] as usize;
        let mut offset = 1;
        let mut songs = Vec::with_capacity(song_count);

        for _ in 0..song_count {
            if offset + 8 > SECTOR_SIZE {
                return Err("Metadata overflow".into());
            }

            let start_sector = u32::from_be_bytes(
                data[offset..offset + 4].try_into().map_err(|_| "Bad start sector")?
            );
            offset += 4;

            let sector_count = u32::from_be_bytes(
                data[offset..offset + 4].try_into().map_err(|_| "Bad sector count")?
            );
            offset += 4;

            // Read null-terminated name
            let name_start = offset;
            while offset < SECTOR_SIZE && data[offset] != 0 {
                offset += 1;
            }
            let name = String::from_utf8_lossy(&data[name_start..offset]).to_string();
            if offset < SECTOR_SIZE { offset += 1; } // skip null

            songs.push(SongEntry {
                start_sector,
                sector_count,
                name,
            });
        }

        Ok(AlbumMetadata { songs })
    }
}
