use std::time::Duration;

const STEM_PLAYER_VID: u16 = 0x2367;
const STEM_PLAYER_PID: u16 = 0x1701;
const SECTOR_SIZE: usize = 8192;
const MAX_RETRIES: u32 = 3;

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

                // Detach kernel driver if active on interface 0 (CDC control)
                if let Ok(true) = handle.kernel_driver_active(0) {
                    let _ = handle.detach_kernel_driver(0);
                }
                // Also check interface 1 (CDC data)
                if let Ok(true) = handle.kernel_driver_active(1) {
                    let _ = handle.detach_kernel_driver(1);
                }

                // Note: We do NOT claim an interface for vendor control transfers.
                // Vendor requests go to the device (Recipient::Device), not a specific interface.
                // Claiming the CDC ACM interface can interfere with vendor control transfers.

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

    pub fn is_connected(&self) -> bool { self.handle.is_some() }

    pub fn read_sectors(&self, start_sector: u32, count: u32) -> Result<Vec<u8>, String> {
        let handle = self.handle.as_ref().ok_or("Not connected")?;
        let mut buffer = vec![0u8; SECTOR_SIZE * count as usize];

        for i in 0..count {
            let sector = start_sector + i;
            let offset = (i as usize) * SECTOR_SIZE;
            let mut sector_buf = [0u8; SECTOR_SIZE];

            // Use raw request type 0xC0: Device-to-Host, Vendor, Device recipient
            let mut last_err = None;
            for retry in 0..MAX_RETRIES {
                match handle.read_control(
                    0xC0, // Direction: In, Type: Vendor, Recipient: Device
                    0x01, // bRequest: Read
                    (sector >> 16) as u16,   // wValue: high 16 bits of sector offset
                    (sector & 0xFFFF) as u16, // wIndex: low 16 bits of sector offset
                    &mut sector_buf,
                    Duration::from_secs(5),
                ) {
                    Ok(bytes_read) => {
                        if bytes_read != SECTOR_SIZE {
                            return Err(format!(
                                "Read sector {}: expected {} bytes, got {}",
                                sector, SECTOR_SIZE, bytes_read
                            ));
                        }
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(format!("Read sector {} failed (attempt {}): {}", sector, retry + 1, e));
                        // Small delay before retry
                        std::thread::sleep(Duration::from_millis(100 * (retry as u64 + 1)));
                    }
                }
            }

            if let Some(err) = last_err {
                return Err(format!(
                    "{}. Make sure the Stem Player is connected and not in use by another application.",
                    err
                ));
            }

            buffer[offset..offset + SECTOR_SIZE].copy_from_slice(&sector_buf);
        }
        Ok(buffer)
    }

    pub fn write_sectors(&self, start_sector: u32, data: &[u8]) -> Result<(), String> {
        let handle = self.handle.as_ref().ok_or("Not connected")?;
        if data.len() % SECTOR_SIZE != 0 {
            return Err("Data must be aligned to sector size".into());
        }
        let count = data.len() / SECTOR_SIZE;

        for i in 0..count {
            let sector = start_sector + i as u32;
            let offset = i * SECTOR_SIZE;

            let mut last_err = None;
            for retry in 0..MAX_RETRIES {
                // Use raw request type 0x40: Host-to-Device, Vendor, Device recipient
                match handle.write_control(
                    0x40, // Direction: Out, Type: Vendor, Recipient: Device
                    0x02, // bRequest: Write
                    (sector >> 16) as u16,
                    (sector & 0xFFFF) as u16,
                    &data[offset..offset + SECTOR_SIZE],
                    Duration::from_secs(5),
                ) {
                    Ok(_) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(format!("Write sector {} failed (attempt {}): {}", sector, retry + 1, e));
                        std::thread::sleep(Duration::from_millis(100 * (retry as u64 + 1)));
                    }
                }
            }

            if let Some(err) = last_err {
                return Err(err);
            }
        }
        Ok(())
    }
}
