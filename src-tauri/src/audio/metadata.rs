const SECTOR_SIZE: usize = 8192;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SongEntry {
    pub start_sector: u32,
    pub length_sectors: u32,
    pub artist: String,
    pub title: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlbumMetadata {
    pub title: String,
    pub songs: Vec<SongEntry>,
}

const MAGIC: &[u8] = b"ALBUM_PRESENT";

impl AlbumMetadata {
    /// Create the album metadata sector
    pub fn to_sector(&self) -> Vec<u8> {
        let mut sector = vec![0u8; SECTOR_SIZE];
        let mut offset = 0;

        // Magic bytes
        sector[offset..offset + MAGIC.len()].copy_from_slice(MAGIC);
        offset += MAGIC.len();

        // Album length placeholder (total sectors)
        let total_sectors: u32 = self.songs.iter().map(|s| s.length_sectors).sum();
        sector[offset..offset + 4].copy_from_slice(&total_sectors.to_le_bytes());
        offset += 4;

        // Song count
        let song_count = self.songs.len() as u32;
        sector[offset..offset + 4].copy_from_slice(&song_count.to_le_bytes());
        offset += 4;

        // Album title (64 bytes, null-terminated, X-padded)
        let title_bytes = pad_string(&self.title, 64);
        sector[offset..offset + 64].copy_from_slice(&title_bytes);
        offset += 64;

        // Song entries
        for song in &self.songs {
            // Start offset (4 bytes)
            sector[offset..offset + 4].copy_from_slice(&song.start_sector.to_le_bytes());
            offset += 4;

            // Length in sectors (4 bytes)
            sector[offset..offset + 4].copy_from_slice(&song.length_sectors.to_le_bytes());
            offset += 4;

            // Artist (64 bytes)
            let artist_bytes = pad_string(&song.artist, 64);
            sector[offset..offset + 64].copy_from_slice(&artist_bytes);
            offset += 64;

            // Title (64 bytes)
            let title_bytes = pad_string(&song.title, 64);
            sector[offset..offset + 64].copy_from_slice(&title_bytes);
            offset += 64;
        }

        // End magic
        if offset + MAGIC.len() <= SECTOR_SIZE {
            sector[offset..offset + MAGIC.len()].copy_from_slice(MAGIC);
        }

        sector
    }

    /// Parse album metadata from a sector
    pub fn from_sector(data: &[u8]) -> Result<Self, String> {
        if data.len() < SECTOR_SIZE {
            return Err("Invalid sector size".into());
        }

        // Check magic
        if &data[0..MAGIC.len()] != MAGIC {
            return Err("Not an album sector".into());
        }

        let mut offset = MAGIC.len();
        let _total_sectors = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let song_count = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;

        let title = read_padded_string(&data[offset..offset + 64]);
        offset += 64;

        let mut songs = Vec::with_capacity(song_count);
        for _ in 0..song_count {
            let start_sector = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            offset += 4;
            let length_sectors = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            offset += 4;
            let artist = read_padded_string(&data[offset..offset + 64]);
            offset += 64;
            let song_title = read_padded_string(&data[offset..offset + 64]);
            offset += 64;

            songs.push(SongEntry {
                start_sector,
                length_sectors,
                artist,
                title: song_title,
            });
        }

        Ok(AlbumMetadata { title, songs })
    }
}

fn pad_string(s: &str, len: usize) -> Vec<u8> {
    let mut bytes = s.as_bytes().to_vec();
    bytes.truncate(len - 1); // Leave room for null terminator
    bytes.push(0); // Null terminator
    while bytes.len() < len {
        bytes.push(b'X'); // X-padding
    }
    bytes
}

fn read_padded_string(data: &[u8]) -> String {
    let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).to_string()
}
