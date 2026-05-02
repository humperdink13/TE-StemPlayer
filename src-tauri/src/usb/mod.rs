use std::time::Duration;

const STEM_PLAYER_VID: u16 = 0x2367;
const STEM_PLAYER_PID: u16 = 0x1701;
const SECTOR_SIZE: usize = 8192;
const MAX_RETRIES: u32 = 3;
/// nRF52840 is USB 2.0 Full-Speed (12 Mbps). Full-speed control transfers
/// have a maximum data stage of 64 bytes per transaction. Using larger values
/// causes PIPE errors (STALL) because the device firmware rejects oversized
/// control transfers.
const CHUNK_SIZE: usize = 64;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceInfo {
    pub connected: bool,
    pub serial: Option<String>,
    pub firmware_version: Option<String>,
}

pub struct StemPlayerDevice {
    handle: Option<rusb::DeviceHandle<rusb::GlobalContext>>,
}

impl StemPlayerDevice {
    pub fn new() -> Self {
        Self { handle: None }
    }

    pub fn find_and_connect(&mut self) -> Result<DeviceInfo, String> {
        let devices = rusb::devices().map_err(|e| format!("USB error: {}", e))?;
        for device in devices.iter() {
            let desc = device.device_descriptor().map_err(|e| format!("{}", e))?;
            if desc.vendor_id() == STEM_PLAYER_VID && desc.product_id() == STEM_PLAYER_PID {
                let handle = device.open().map_err(|e| format!("Cannot open device: {}", e))?;

                // Detach kernel driver if active (Linux/macOS CDC ACM driver)
                for iface in 0..=1 {
                    if let Ok(true) = handle.kernel_driver_active(iface) {
                        let _ = handle.detach_kernel_driver(iface);
                    }
                }

                // Set active configuration (required before control transfers,
                // matches all working PyUSB fuzzing scripts)
                let _ = handle.set_active_configuration(1);

                // Claim interface 0 (CDC ACM data interface)
                // This is required for vendor control transfers on the Stem Player
                let _ = handle.claim_interface(0);

                let serial = handle.read_serial_number_string_ascii(&desc).ok();
                self.handle = Some(handle);
                return Ok(DeviceInfo {
                    connected: true,
                    serial,
                    firmware_version: None,
                });
            }
        }
        Err("Stem Player not found. Make sure it's plugged in via USB-C and powered on.".into())
    }

    pub fn disconnect(&mut self) {
        self.handle = None;
    }

    pub fn is_connected(&self) -> bool {
        self.handle.is_some()
    }

    /// Read one or more 8192-byte sectors from the device eMMC.
    ///
    /// Each sector is read in CHUNK_SIZE-byte control transfers because the
    /// nRF52840 USB stack cannot handle a single 8192-byte control transfer.
    ///
    /// wValue/wIndex encoding (matching the device firmware, confirmed via
    /// reverse-engineering fuzzing scripts):
    ///   wValue = low 16 bits of the sector-relative byte offset
    ///   wIndex = high 16 bits (sector number for sector-addressed requests)
    pub fn read_sectors(&self, start_sector: u32, count: u32) -> Result<Vec<u8>, String> {
        let handle = self.handle.as_ref().ok_or("Not connected")?;
        let mut buffer = vec![0u8; SECTOR_SIZE * count as usize];

        for i in 0..count {
            let sector = start_sector + i;
            let buf_offset = (i as usize) * SECTOR_SIZE;

            // Read one sector in CHUNK_SIZE pieces
            let chunks_per_sector = SECTOR_SIZE / CHUNK_SIZE;
            for chunk_idx in 0..chunks_per_sector {
                let byte_offset_in_sector = chunk_idx * CHUNK_SIZE;
                // Combine sector number and byte offset into a 32-bit address.
                // The device firmware reconstructs: addr = (wIndex << 16) | wValue
                // So wValue = low 16 bits, wIndex = high 16 bits.
                let addr = (sector as u64 * SECTOR_SIZE as u64 + byte_offset_in_sector as u64) as u32;
                let w_value = (addr & 0xFFFF) as u16;
                let w_index = ((addr >> 16) & 0xFFFF) as u16;

                let dst_start = buf_offset + byte_offset_in_sector;
                let dst_end = dst_start + CHUNK_SIZE;
                let mut chunk_buf = [0u8; CHUNK_SIZE];

                let mut last_err = None;
                for retry in 0..MAX_RETRIES {
                    match handle.read_control(
                        0xC0, // Device-to-Host, Vendor, Device recipient
                        0x01, // bRequest: Read
                        w_value,
                        w_index,
                        &mut chunk_buf,
                        Duration::from_secs(5),
                    ) {
                        Ok(bytes_read) => {
                            if bytes_read != CHUNK_SIZE {
                                last_err = Some(format!(
                                    "Read sector {} chunk {}: expected {} bytes, got {}",
                                    sector, chunk_idx, CHUNK_SIZE, bytes_read
                                ));
                                std::thread::sleep(Duration::from_millis(50 * (retry as u64 + 1)));
                                continue;
                            }
                            last_err = None;
                            break;
                        }
                        Err(e) => {
                            last_err = Some(format!(
                                "Read sector {} chunk {} failed (attempt {}): {}",
                                sector, chunk_idx, retry + 1, e
                            ));
                            std::thread::sleep(Duration::from_millis(50 * (retry as u64 + 1)));
                        }
                    }
                }

                if let Some(err) = last_err {
                    return Err(format!(
                        "{}. Make sure the Stem Player is connected and not in use by another application.",
                        err
                    ));
                }

                buffer[dst_start..dst_end].copy_from_slice(&chunk_buf);
            }
        }
        Ok(buffer)
    }

    /// Write one or more 8192-byte sectors to the device eMMC.
    ///
    /// Uses the same chunked approach and wValue/wIndex encoding as read_sectors.
    pub fn write_sectors(&self, start_sector: u32, data: &[u8]) -> Result<(), String> {
        let handle = self.handle.as_ref().ok_or("Not connected")?;
        if data.len() % SECTOR_SIZE != 0 {
            return Err("Data must be aligned to sector size".into());
        }
        let count = data.len() / SECTOR_SIZE;

        for i in 0..count {
            let sector = start_sector + i as u32;
            let buf_offset = i * SECTOR_SIZE;

            let chunks_per_sector = SECTOR_SIZE / CHUNK_SIZE;
            for chunk_idx in 0..chunks_per_sector {
                let byte_offset_in_sector = chunk_idx * CHUNK_SIZE;
                let addr = (sector as u64 * SECTOR_SIZE as u64 + byte_offset_in_sector as u64) as u32;
                let w_value = (addr & 0xFFFF) as u16;
                let w_index = ((addr >> 16) & 0xFFFF) as u16;

                let src_start = buf_offset + byte_offset_in_sector;
                let src_end = src_start + CHUNK_SIZE;

                let mut last_err = None;
                for retry in 0..MAX_RETRIES {
                    match handle.write_control(
                        0x40, // Host-to-Device, Vendor, Device recipient
                        0x02, // bRequest: Write
                        w_value,
                        w_index,
                        &data[src_start..src_end],
                        Duration::from_secs(5),
                    ) {
                        Ok(bytes_written) => {
                            if bytes_written != CHUNK_SIZE {
                                last_err = Some(format!(
                                    "Write sector {} chunk {}: expected {} bytes written, got {}",
                                    sector, chunk_idx, CHUNK_SIZE, bytes_written
                                ));
                                std::thread::sleep(Duration::from_millis(50 * (retry as u64 + 1)));
                                continue;
                            }
                            last_err = None;
                            break;
                        }
                        Err(e) => {
                            last_err = Some(format!(
                                "Write sector {} chunk {} failed (attempt {}): {}",
                                sector, chunk_idx, retry + 1, e
                            ));
                            std::thread::sleep(Duration::from_millis(50 * (retry as u64 + 1)));
                        }
                    }
                }

                if let Some(err) = last_err {
                    return Err(err);
                }
            }
        }
        Ok(())
    }
}
