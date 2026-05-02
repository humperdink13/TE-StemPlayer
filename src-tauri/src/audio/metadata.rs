//! Album metadata sector (sector 0 on the SP-1 eMMC).
//!
//! Binary format:
//!   Bytes 0-12:   Magic bytes "ALBUM_PRESENT"
//!   Byte  13:     Version (currently 1)
//!   Bytes 14-15:  Reserved (zero)
//!   Bytes 16-19:  Song count (u32 LE)
//!   Bytes 20+:    Song entries (each 136 bytes)
//!
//! Song entry (136 bytes):
//!   Bytes 0-3:    start_sector (u32 LE) — first sector of this song
//!   Bytes 4-7:    length_sectors (u32 LE) — number of sectors
//!   Bytes 8-71:   artist (64 bytes, UTF-8, null-terminated, 'X' padded)
//!   Bytes 72-135: title  (64 bytes, UTF-8, null-terminated, 'X' padded)

use std::fmt;

/// Magic bytes that identify a valid album metadata sector.
pub const MAGIC: &[u8; 13] = b"ALBUM_PRESENT";

/// Current metadata format version.
pub const VERSION: u8 = 1;

/// Size of a song entry in bytes.
pub const SONG_ENTRY_SIZE: usize = 136;

/// Maximum length of artist/title string fields (including null terminator).
pub const STRING_FIELD_SIZE: usize = 64;

/// Maximum number of songs that fit in one metadata sector.
pub const MAX_SONGS: usize = (crate::audio::SECTOR_SIZE - 20) / SONG_ENTRY_SIZE; // ~60


/// A single song entry in the album metadata.
#[derive(Debug, Clone, PartialEq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Song {
    /// Sector index where this song's data begins.
    pub start_sector: u32,
    /// Number of sectors this song occupies.
    #[serde(alias = "sector_count")]
    pub length_sectors: u32,
    /// Artist name (max 63 characters + null terminator).
    pub artist: String,
    /// Song title (max 63 characters + null terminator).
    pub title: String,
}


/// Album metadata containing all songs on the device.
#[derive(Debug, Clone, PartialEq)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Album {
    /// Album title.
    #[serde(default)]
    pub title: String,
    pub songs: Vec<Song>,
}


impl Album {
    /// Create a new empty album.
    pub fn new() -> Self {
        Self {
            title: String::new(),
            songs: Vec::new(),
        }
    }

    /// Add a song to the album.
    pub fn add_song(&mut self, song: Song) {
        self.songs.push(song);
    }
}


/// Errors that can occur during metadata parsing.
#[derive(Debug)]
pub enum MetadataError {
    InvalidMagic,
    InvalidVersion(u8),
    TooManySongs(usize),
    InvalidUtf8(std::str::Utf8Error),
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "Invalid magic bytes — expected 'ALBUM_PRESENT'"),
            Self::InvalidVersion(v) => write!(f, "Unsupported metadata version: {}", v),
            Self::TooManySongs(n) => write!(f, "Too many songs ({}), max is {}", n, MAX_SONGS),
            Self::InvalidUtf8(e) => write!(f, "Invalid UTF-8 in string field: {}", e),
        }
    }
}

impl std::error::Error for MetadataError {}


/// Pad a string to exactly 64 bytes: null-terminated, remainder filled with 'X'.
fn pad_string_field(s: &str) -> [u8; STRING_FIELD_SIZE] {
    let mut buf = [b'X'; STRING_FIELD_SIZE];
    let bytes = s.as_bytes();
    let copy_len = bytes.len().min(STRING_FIELD_SIZE - 1);
    buf[..copy_len].copy_from_slice(&bytes[..copy_len]);
    buf[copy_len] = b'\0'; // null terminate
    buf
}


/// Parse a padded string field (64 bytes, null-terminated, 'X' padded).
fn parse_string_field(buf: &[u8; STRING_FIELD_SIZE]) -> Result<String, MetadataError> {
    // Find null terminator position
    let end = buf.iter().position(|&b| b == b'\0').unwrap_or(STRING_FIELD_SIZE);
    let slice = &buf[..end];
    // Trim trailing 'X' characters that may appear before null
    let trimmed = slice.strip_suffix(&[b'X']).unwrap_or(slice);
    let trimmed = trimmed.strip_suffix(&[b'X']).unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix(&[b'X']).unwrap_or(trimmed);
    let s = std::str::from_utf8(trimmed).map_err(MetadataError::InvalidUtf8)?;
    Ok(s.to_string())
}


/// Serialize an Album into an 8192-byte metadata sector.
///
/// Format:
///   [MAGIC: 13][version: 1][reserved: 2][song_count: 4][song_entries...]
///   Remaining bytes are zero-padded.
pub fn serialize_album(album: &Album) -> [u8; crate::audio::SECTOR_SIZE] {
    let mut sector = [0u8; crate::audio::SECTOR_SIZE];

    // Magic bytes (0-12)
    sector[0..13].copy_from_slice(MAGIC);

    // Version (13)
    sector[13] = VERSION;

    // Reserved (14-15) remain zero

    // Song count u32 LE (16-19)
    let count = album.songs.len().min(MAX_SONGS) as u32;
    sector[16..20].copy_from_slice(&count.to_le_bytes());

    // Song entries starting at byte 20
    for (i, song) in album.songs.iter().take(MAX_SONGS).enumerate() {
        let offset = 20 + i * SONG_ENTRY_SIZE;

        // start_sector u32 LE (bytes 0-3 of entry)
        sector[offset..offset + 4].copy_from_slice(&song.start_sector.to_le_bytes());
        // length_sectors u32 LE (bytes 4-7 of entry)
        sector[offset + 4..offset + 8].copy_from_slice(&song.length_sectors.to_le_bytes());

        // artist (bytes 8-71 of entry)
        let artist_bytes = pad_string_field(&song.artist);
        sector[offset + 8..offset + 8 + STRING_FIELD_SIZE].copy_from_slice(&artist_bytes);

        // title (bytes 72-135 of entry)
        let title_bytes = pad_string_field(&song.title);
        sector[offset + 8 + STRING_FIELD_SIZE..offset + 8 + STRING_FIELD_SIZE * 2]
            .copy_from_slice(&title_bytes);
    }

    sector
}


/// Parse an 8192-byte sector into an Album.
///
/// Returns `MetadataError::InvalidMagic` if magic bytes don't match.
/// Returns `MetadataError::InvalidVersion` if the format version is unsupported.
pub fn parse_album(sector: &[u8; crate::audio::SECTOR_SIZE]) -> Result<Album, MetadataError> {
    // Validate magic bytes
    if &sector[0..13] != MAGIC {
        return Err(MetadataError::InvalidMagic);
    }

    // Check version
    let version = sector[13];
    if version != VERSION {
        return Err(MetadataError::InvalidVersion(version));
    }

    // Read song count
    let count = u32::from_le_bytes([
        sector[16], sector[17], sector[18], sector[19],
    ]) as usize;

    if count > MAX_SONGS {
        return Err(MetadataError::TooManySongs(count));
    }

    let mut songs = Vec::with_capacity(count);

    for i in 0..count {
        let offset = 20 + i * SONG_ENTRY_SIZE;
        if offset + SONG_ENTRY_SIZE > crate::audio::SECTOR_SIZE {
            break;
        }

        let start_sector = u32::from_le_bytes([
            sector[offset], sector[offset + 1],
            sector[offset + 2], sector[offset + 3],
        ]);

        let length_sectors = u32::from_le_bytes([
            sector[offset + 4], sector[offset + 5],
            sector[offset + 6], sector[offset + 7],
        ]);

        let artist_bytes: [u8; STRING_FIELD_SIZE] = sector[
            offset + 8..offset + 8 + STRING_FIELD_SIZE
        ].try_into().unwrap();

        let title_bytes: [u8; STRING_FIELD_SIZE] = sector[
            offset + 8 + STRING_FIELD_SIZE..offset + 8 + STRING_FIELD_SIZE * 2
        ].try_into().unwrap();

        songs.push(Song {
            start_sector,
            length_sectors,
            artist: parse_string_field(&artist_bytes)?,
            title: parse_string_field(&title_bytes)?,
        });
    }

    Ok(Album {
        title: String::new(), // Title not stored in binary format yet
        songs,
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_and_parse_string_field() {
        let padded = pad_string_field("The Beatles");
        assert_eq!(&padded[..11], b"The Beatles");
        assert_eq!(padded[11], b'\0');
        assert_eq!(padded[12], b'X');
        assert_eq!(padded[63], b'X');

        let parsed = parse_string_field(&padded).unwrap();
        assert_eq!(parsed, "The Beatles");
    }

    #[test]
    fn test_string_field_truncation() {
        let long_name = "A".repeat(70);
        let padded = pad_string_field(&long_name);
        assert_eq!(&padded[..63], b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        assert_eq!(padded[63], b'\0');

        let parsed = parse_string_field(&padded).unwrap();
        assert_eq!(parsed.len(), 63);
    }

    #[test]
    fn test_serialize_parse_roundtrip() {
        let mut album = Album {
            title: "Test Album".to_string(),
            songs: Vec::new(),
        };
        album.add_song(Song {
            start_sector: 1,
            length_sectors: 1000,
            artist: "Daft Punk".into(),
            title: "Get Lucky".into(),
        });
        album.add_song(Song {
            start_sector: 1001,
            length_sectors: 800,
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
        });

        let sector = serialize_album(&album);
        let parsed = parse_album(&sector).unwrap();

        assert_eq!(parsed.songs.len(), 2);
        assert_eq!(parsed.songs[0].start_sector, 1);
        assert_eq!(parsed.songs[0].length_sectors, 1000);
        assert_eq!(parsed.songs[0].artist, "Daft Punk");
        assert_eq!(parsed.songs[0].title, "Get Lucky");
        assert_eq!(parsed.songs[1].start_sector, 1001);
        assert_eq!(parsed.songs[1].artist, "Queen");
        assert_eq!(parsed.songs[1].title, "Bohemian Rhapsody");
    }

    #[test]
    fn test_invalid_magic() {
        let sector = [0u8; crate::audio::SECTOR_SIZE];
        let result = parse_album(&sector);
        assert!(matches!(result, Err(MetadataError::InvalidMagic)));
    }

    #[test]
    fn test_empty_album() {
        let album = Album::new();
        let sector = serialize_album(&album);
        let parsed = parse_album(&sector).unwrap();
        assert!(parsed.songs.is_empty());
    }

    #[test]
    fn test_version_check() {
        let mut sector = [0u8; crate::audio::SECTOR_SIZE];
        sector[0..13].copy_from_slice(MAGIC);
        sector[13] = 99;
        let result = parse_album(&sector);
        assert!(matches!(result, Err(MetadataError::InvalidVersion(99))));
    }
}
