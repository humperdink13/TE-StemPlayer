use sha2::{Sha256, Digest};
use std::fs;
use std::path::Path;

const SECTOR_SIZE: usize = 8192;

/// Backup file header: "STEMBAK\0" + u32 BE sector count + 32 bytes SHA-256
const HEADER_MAGIC: &[u8; 8] = b"STEMBAK\0";
const HEADER_SIZE: usize = 8 + 4 + 32; // magic + sector_count + sha256

/// Create a full eMMC backup from raw sector data
pub fn create_backup(sectors: &[Vec<u8>], output_path: &Path) -> Result<String, String> {
    let sector_count = sectors.len() as u32;

    // Compute SHA-256 of all sector data
    let mut hasher = Sha256::new();
    for sector in sectors {
        hasher.update(sector);
    }
    let hash = hasher.finalize();
    let hash_hex = hex::encode(&hash);

    // Build backup file
    let mut file_data = Vec::with_capacity(HEADER_SIZE + sectors.len() * SECTOR_SIZE);

    // Header
    file_data.extend_from_slice(HEADER_MAGIC);
    file_data.extend_from_slice(&sector_count.to_be_bytes());
    file_data.extend_from_slice(&hash);

    // Sector data
    for sector in sectors {
        file_data.extend_from_slice(sector);
    }

    fs::write(output_path, &file_data)
        .map_err(|e| format!("Failed to write backup: {}", e))?;

    Ok(hash_hex)
}

/// Load and verify a backup file, returns sector data
pub fn load_backup(backup_path: &Path) -> Result<Vec<Vec<u8>>, String> {
    let file_data = fs::read(backup_path)
        .map_err(|e| format!("Failed to read backup: {}", e))?;

    if file_data.len() < HEADER_SIZE {
        return Err("Backup file too small".into());
    }

    // Verify magic
    if &file_data[0..8] != HEADER_MAGIC {
        return Err("Not a valid stem backup file".into());
    }

    let sector_count = u32::from_be_bytes(
        file_data[8..12].try_into().map_err(|_| "Bad sector count")?
    ) as usize;

    let stored_hash: [u8; 32] = file_data[12..44]
        .try_into().map_err(|_| "Bad hash")?;

    let sector_data = &file_data[HEADER_SIZE..];
    if sector_data.len() != sector_count * SECTOR_SIZE {
        return Err(format!(
            "Backup size mismatch: expected {} sectors ({} bytes), got {} bytes",
            sector_count, sector_count * SECTOR_SIZE, sector_data.len()
        ));
    }

    // Verify SHA-256
    let mut hasher = Sha256::new();
    hasher.update(sector_data);
    let computed_hash = hasher.finalize();

    if computed_hash.as_slice() != &stored_hash {
        return Err("Backup integrity check failed: SHA-256 mismatch".into());
    }

    // Split into sectors
    let mut sectors = Vec::with_capacity(sector_count);
    for i in 0..sector_count {
        let offset = i * SECTOR_SIZE;
        sectors.push(sector_data[offset..offset + SECTOR_SIZE].to_vec());
    }

    Ok(sectors)
}
