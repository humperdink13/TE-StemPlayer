use std::io::{Read, Write};
use std::time::Duration;

const STEM_PLAYER_VID: u16 = 0x2367;
const STEM_PLAYER_PID: u16 = 0x1701;
pub const SECTOR_SIZE: usize = 8192;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceInfo {
    pub connected: bool,
    pub port_name: Option<String>,
    pub serial: Option<String>,
    pub firmware_version: Option<String>,
}

pub struct StemPlayerDevice {
    port: Option<Box<dyn serialport::SerialPort>>,
    port_name: Option<String>,
}

unsafe impl Send for StemPlayerDevice {}
unsafe impl Sync for StemPlayerDevice {}

impl StemPlayerDevice {
    pub fn new() -> Self {
        Self { port: None, port_name: None }
    }

    pub fn find_and_connect(&mut self) -> Result<DeviceInfo, String> {
        let ports = serialport::available_ports()
            .map_err(|e| format!("Failed to enumerate serial ports: {}", e))?;
        for port_info in &ports {
            if let serialport::SerialPortType::UsbPort(usb_info) = &port_info.port_type {
                if usb_info.vid == STEM_PLAYER_VID && usb_info.pid == STEM_PLAYER_PID {
                    let port = serialport::new(&port_info.port_name, 115200)
                        .timeout(Duration::from_secs(5))
                        .open()
                        .map_err(|e| format!("Cannot open {}: {}", port_info.port_name, e))?;
                    let serial = usb_info.serial_number.clone();
                    self.port_name = Some(port_info.port_name.clone());
                    self.port = Some(port);
                    return Ok(DeviceInfo {
                        connected: true,
                        port_name: self.port_name.clone(),
                        serial,
                        firmware_version: None,
                    });
                }
            }
        }
        Err("Stem Player not found. Is it connected via USB?".into())
    }

    pub fn disconnect(&mut self) { self.port = None; self.port_name = None; }
    pub fn is_connected(&self) -> bool { self.port.is_some() }

    fn send_command(&mut self, cmd: &str) -> Result<String, String> {
        let port = self.port.as_mut().ok_or("Not connected")?;
        port.write_all(format!("{}\n", cmd).as_bytes()).map_err(|e| format!("Write failed: {}", e))?;
        port.flush().map_err(|e| format!("Flush failed: {}", e))?;
        let mut response = String::new();
        let mut buf = [0u8; 1];
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if std::time::Instant::now() > deadline { return Err("Command timeout".into()); }
            match port.read(&mut buf) {
                Ok(1) => {
                    if buf[0] == b'\n' { break; }
                    if buf[0] != b'\r' { response.push(buf[0] as char); }
                }
                Ok(_) => continue,
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                Err(e) => return Err(format!("Read failed: {}", e)),
            }
        }
        Ok(response)
    }

    pub fn get_status(&mut self) -> Result<String, String> { self.send_command("STATUS") }

    pub fn read_sector(&mut self, sector: u32) -> Result<Vec<u8>, String> {
        let port = self.port.as_mut().ok_or("Not connected")?;
        port.write_all(format!("READ {}\n", sector).as_bytes()).map_err(|e| format!("Write failed: {}", e))?;
        port.flush().map_err(|e| format!("Flush failed: {}", e))?;
        let mut buffer = vec![0u8; SECTOR_SIZE];
        let mut total_read = 0;
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while total_read < SECTOR_SIZE {
            if std::time::Instant::now() > deadline {
                return Err(format!("Timeout reading sector {} ({}/{})", sector, total_read, SECTOR_SIZE));
            }
            match port.read(&mut buffer[total_read..]) {
                Ok(n) if n > 0 => total_read += n,
                Ok(_) => continue,
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
                Err(e) => return Err(format!("Read sector {} failed: {}", sector, e)),
            }
        }
        Ok(buffer)
    }

    pub fn read_sectors(&mut self, start_sector: u32, count: u32) -> Result<Vec<u8>, String> {
        let mut data = Vec::with_capacity(SECTOR_SIZE * count as usize);
        for i in 0..count { data.extend_from_slice(&self.read_sector(start_sector + i)?); }
        Ok(data)
    }

    pub fn write_sector(&mut self, sector: u32, data: &[u8]) -> Result<(), String> {
        if data.len() != SECTOR_SIZE { return Err(format!("Sector data must be {} bytes", SECTOR_SIZE)); }
        let port = self.port.as_mut().ok_or("Not connected")?;
        port.write_all(format!("WRITE {}\n", sector).as_bytes()).map_err(|e| format!("Write cmd failed: {}", e))?;
        port.flush().map_err(|e| format!("Flush failed: {}", e))?;
        std::thread::sleep(Duration::from_millis(10));
        port.write_all(data).map_err(|e| format!("Write sector {} data failed: {}", sector, e))?;
        port.flush().map_err(|e| format!("Flush failed: {}", e))?;
        Ok(())
    }

    pub fn write_sectors(&mut self, start_sector: u32, data: &[u8]) -> Result<(), String> {
        if data.len() % SECTOR_SIZE != 0 { return Err("Data must be aligned to sector size".into()); }
        let count = data.len() / SECTOR_SIZE;
        for i in 0..count {
            let offset = i * SECTOR_SIZE;
            self.write_sector(start_sector + i as u32, &data[offset..offset + SECTOR_SIZE])?;
        }
        Ok(())
    }
}
