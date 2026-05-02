use std::time::Duration;

const STEM_PLAYER_VID: u16 = 0x2367;
const STEM_PLAYER_PID: u16 = 0x1701;
const SECTOR_SIZE: usize = 8192;

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
                let serial = handle.read_serial_number_string_ascii(&desc).ok();
                self.handle = Some(handle);
                return Ok(DeviceInfo {
                    connected: true,
                    serial,
                    firmware_version: None,
                });
            }
        }
        Err("Stem Player not found".into())
    }

    pub fn disconnect(&mut self) { self.handle = None; }
    pub fn is_connected(&self) -> bool { self.handle.is_some() }

    pub fn read_sectors(&self, start_sector: u32, count: u32) -> Result<Vec<u8>, String> {
        let handle = self.handle.as_ref().ok_or("Not connected")?;
        let mut buffer = vec![0u8; SECTOR_SIZE * count as usize];
        for i in 0..count {
            let sector = start_sector + i;
            let offset = (i as usize) * SECTOR_SIZE;
            let mut sector_buf = [0u8; SECTOR_SIZE];
            handle.read_control(
                rusb::request_type(rusb::Direction::In, rusb::RequestType::Vendor, rusb::Recipient::Device),
                0x01, (sector >> 16) as u16, (sector & 0xFFFF) as u16,
                &mut sector_buf, Duration::from_secs(5),
            ).map_err(|e| format!("Read sector {} failed: {}", sector, e))?;
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
            handle.write_control(
                rusb::request_type(rusb::Direction::Out, rusb::RequestType::Vendor, rusb::Recipient::Device),
                0x02, (sector >> 16) as u16, (sector & 0xFFFF) as u16,
                &data[offset..offset + SECTOR_SIZE], Duration::from_secs(5),
            ).map_err(|e| format!("Write sector {} failed: {}", sector, e))?;
        }
        Ok(())
    }
}
