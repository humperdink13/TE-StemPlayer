
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const STEM_PLAYER_VID: u16 = 0x2367;
const STEM_PLAYER_PID: u16 = 0x1701;
const SECTOR_SIZE: usize = 8192;
const SERIAL_BAUD: u32 = 115200;
const SERIAL_TIMEOUT: Duration = Duration::from_secs(3);
const SECTOR_READ_TIMEOUT: Duration = Duration::from_secs(10);

// CRC8 lookup table (same as firmware)
const CRC8_TABLE: [u8; 256] = [
    0xea,0xd4,0x96,0xa8,0x12,0x2c,0x6e,0x50,0x7f,0x41,0x03,0x3d,0x87,0xb9,0xfb,0xc5,
    0xa5,0x9b,0xd9,0xe7,0x5d,0x63,0x21,0x1f,0x30,0x0e,0x4c,0x72,0xc8,0xf6,0xb4,0x8a,
    0x74,0x4a,0x08,0x36,0x8c,0xb2,0xf0,0xce,0xe1,0xdf,0x9d,0xa3,0x19,0x27,0x65,0x5b,
    0x3b,0x05,0x47,0x79,0xc3,0xfd,0xbf,0x81,0xae,0x90,0xd2,0xec,0x56,0x68,0x2a,0x14,
    0xb3,0x8d,0xcf,0xf1,0x4b,0x75,0x37,0x09,0x26,0x18,0x5a,0x64,0xde,0xe0,0xa2,0x9c,
    0xfc,0xc2,0x80,0xbe,0x04,0x3a,0x78,0x46,0x69,0x57,0x15,0x2b,0x91,0xaf,0xed,0xd3,
    0x2d,0x13,0x51,0x6f,0xd5,0xeb,0xa9,0x97,0xb8,0x86,0xc4,0xfa,0x40,0x7e,0x3c,0x02,
    0x62,0x5c,0x1e,0x20,0x9a,0xa4,0xe6,0xd8,0xf7,0xc9,0x8b,0xb5,0x0f,0x31,0x73,0x4d,
    0x58,0x66,0x24,0x1a,0xa0,0x9e,0xdc,0xe2,0xcd,0xf3,0xb1,0x8f,0x35,0x0b,0x49,0x77,
    0x17,0x29,0x6b,0x55,0xef,0xd1,0x93,0xad,0x82,0xbc,0xfe,0xc0,0x7a,0x44,0x06,0x38,
    0xc6,0xf8,0xba,0x84,0x3e,0x00,0x42,0x7c,0x53,0x6d,0x2f,0x11,0xab,0x95,0xd7,0xe9,
    0x89,0xb7,0xf5,0xcb,0x71,0x4f,0x0d,0x33,0x1c,0x22,0x60,0x5e,0xe4,0xda,0x98,0xa6,
    0x01,0x3f,0x7d,0x43,0xf9,0xc7,0x85,0xbb,0x94,0xaa,0xe8,0xd6,0x6c,0x52,0x10,0x2e,
    0x4e,0x70,0x32,0x0c,0xb6,0x88,0xca,0xf4,0xdb,0xe5,0xa7,0x99,0x23,0x1d,0x5f,0x61,
    0x9f,0xa1,0xe3,0xdd,0x67,0x59,0x1b,0x25,0x0a,0x34,0x76,0x48,0xf2,0xcc,0x8e,0xb0,
    0xd0,0xee,0xac,0x92,0x28,0x16,0x54,0x6a,0x45,0x7b,0x39,0x07,0xbd,0x83,0xc1,0xff,
];

fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc = CRC8_TABLE[(crc ^ b) as usize];
    }
    crc
}

fn cobs_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 254 + 2);
    let mut idx = 0;
    while idx <= data.len() {
        let mut end = idx;
        while end < data.len() && data[end] != 0 {
            end += 1;
        }
        out.push((end - idx + 1) as u8);
        out.extend_from_slice(&data[idx..end]);
        if end < data.len() {
            idx = end + 1;
        } else {
            break;
        }
    }
    out.push(0x00); // frame delimiter
    out
}

fn cobs_decode(raw: &[u8]) -> Vec<u8> {
    let data = if !raw.is_empty() && raw[raw.len() - 1] == 0 {
        &raw[..raw.len() - 1]
    } else {
        raw
    };
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < data.len() {
        let code = data[idx] as usize;
        idx += 1;
        if code == 0 {
            break;
        }
        for _ in 0..(code - 1) {
            if idx < data.len() {
                out.push(data[idx]);
                idx += 1;
            }
        }
        if code < 0xFF && idx < data.len() {
            out.append(&mut vec![0u8]); // zero byte delimiter
        }
    }
    // Remove trailing zero if we added one past the end
    if !out.is_empty() && out[out.len() - 1] == 0 {
        out.pop();
    }
    out
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceInfo {
    pub connected: bool,
    pub serial: Option<String>,
    pub firmware_version: Option<String>,
    pub data_transfer_supported: bool,
    pub needs_firmware_update: bool,
    pub status_message: Option<String>,
}

pub struct StemPlayerDevice {
    port: Option<Box<dyn serialport::SerialPort>>,
    seq: u8,
    serial_number: Option<String>,
}

impl StemPlayerDevice {
    pub fn new() -> Self {
        Self {
            port: None,
            seq: 1,
            serial_number: None,
        }
    }

    /// Find the Stem Player's CDC ACM serial port by scanning for VID:PID
    fn find_serial_port() -> Option<String> {
        if let Ok(ports) = serialport::available_ports() {
            for p in &ports {
                if let serialport::SerialPortType::UsbPort(usb) = &p.port_type {
                    if usb.vid == STEM_PLAYER_VID && usb.pid == STEM_PLAYER_PID {
                        return Some(p.port_name.clone());
                    }
                }
            }
        }
        None
    }

    /// Also try finding via rusb for the serial number, then match to serial port
    fn find_serial_from_rusb() -> Option<String> {
        if let Ok(devices) = rusb::devices() {
            for device in devices.iter() {
                if let Ok(desc) = device.device_descriptor() {
                    if desc.vendor_id() == STEM_PLAYER_VID && desc.product_id() == STEM_PLAYER_PID {
                        if let Ok(handle) = device.open() {
                            return handle.read_serial_number_string_ascii(&desc).ok();
                        }
                    }
                }
            }
        }
        None
    }

    /// Send a command and receive a response using COBS+CRC8 framing
    fn send_cmd(&mut self, cmd: u8, payload: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
        let port = self.port.as_mut().ok_or("Not connected")?;

        // Clear input buffer
        let _ = port.clear(serialport::ClearBuffer::Input);

        // Build frame: [0x51, seq, cmd, payload_len] + payload + crc8
        let mut body = vec![0x51, self.seq, cmd, payload.len() as u8];
        body.extend_from_slice(payload);
        let checksum = crc8(&body);
        body.push(checksum);

        let packet = cobs_encode(&body);
        self.seq = self.seq.wrapping_add(1);

        port.write_all(&packet).map_err(|e| format!("Serial write error: {}", e))?;
        port.flush().map_err(|e| format!("Serial flush error: {}", e))?;

        // Read response until we get a 0x00 delimiter
        let deadline = Instant::now() + timeout;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 512];

        while Instant::now() < deadline {
            match port.read(&mut tmp) {
                Ok(n) if n > 0 => {
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(idx) = buf.iter().position(|&b| b == 0x00) {
                        let frame = buf[..=idx].to_vec();
                        let decoded = cobs_decode(&frame);
                        if decoded.len() >= 5 {
                            // Verify CRC
                            let data_part = &decoded[..decoded.len() - 1];
                            let recv_crc = decoded[decoded.len() - 1];
                            if crc8(data_part) == recv_crc {
                                let plen = decoded[3] as usize;
                                let payload_data = if plen > 0 && decoded.len() >= 4 + plen {
                                    decoded[4..4 + plen].to_vec()
                                } else {
                                    Vec::new()
                                };
                                return Ok(payload_data);
                            } else {
                                return Err("CRC mismatch in response".into());
                            }
                        }
                        return Err(format!("Response too short: {} bytes", decoded.len()));
                    }
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(format!("Serial read error: {}", e)),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("Timeout waiting for device response".into())
    }

    pub fn find_and_connect(&mut self) -> Result<DeviceInfo, String> {
        // First try to find a serial port with matching VID:PID
        let port_name = Self::find_serial_port();
        let serial_number = Self::find_serial_from_rusb();

        if let Some(ref port_name) = port_name {
            // Device has CDC ACM serial port - our custom firmware is running
            let port = serialport::new(port_name, SERIAL_BAUD)
                .timeout(Duration::from_millis(100))
                .open()
                .map_err(|e| format!("Cannot open serial port {}: {}", port_name, e))?;

            self.port = Some(port);
            self.serial_number = serial_number.clone();

            // Send status command to verify communication
            match self.send_cmd(0x52, &[], SERIAL_TIMEOUT) {
                Ok(resp) => {
                    let fw_version = if resp.len() >= 4 {
                        let ver = u32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]]);
                        Some(format!("{}.{}.{}", ver >> 16, (ver >> 8) & 0xFF, ver & 0xFF))
                    } else {
                        Some("custom".to_string())
                    };

                    let emmc_ok = resp.len() >= 5 && resp[4] == 1;

                    Ok(DeviceInfo {
                        connected: true,
                        serial: serial_number,
                        firmware_version: fw_version,
                        data_transfer_supported: emmc_ok,
                        needs_firmware_update: false,
                        status_message: if emmc_ok {
                            None
                        } else {
                            Some("eMMC storage not detected. Device may need repair.".into())
                        },
                    })
                }
                Err(e) => {
                    self.port = None;
                    Err(format!("Device found on {} but communication failed: {}", port_name, e))
                }
            }
        } else {
            // No serial port found - check if device is on USB at all (stock firmware)
            let usb_present = rusb::devices()
                .map(|devs| {
                    devs.iter().any(|d| {
                        d.device_descriptor()
                            .map(|desc| {
                                desc.vendor_id() == STEM_PLAYER_VID
                                    && desc.product_id() == STEM_PLAYER_PID
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            if usb_present {
                Ok(DeviceInfo {
                    connected: true,
                    serial: serial_number,
                    firmware_version: None,
                    data_transfer_supported: false,
                    needs_firmware_update: true,
                    status_message: Some(
                        "Device found but running stock firmware without serial support. \
                         Go to the Firmware tab to flash custom firmware."
                            .into(),
                    ),
                })
            } else {
                Err("Stem Player not found. Make sure it's plugged in via USB-C and powered on."
                    .into())
            }
        }
    }

    pub fn disconnect(&mut self) {
        self.port = None;
        self.serial_number = None;
    }

    pub fn is_connected(&self) -> bool {
        self.port.is_some()
    }

    /// Read one or more 8192-byte sectors from the device eMMC.
    ///
    /// Sends read_sector command (0x53) for each sector. The firmware responds
    /// with chunked data using COBS+CRC8 framing.
    pub fn read_sectors(&mut self, start_sector: u32, count: u32) -> Result<Vec<u8>, String> {
        let mut buffer = vec![0u8; SECTOR_SIZE * count as usize];

        for i in 0..count {
            let sector = start_sector + i;
            let payload = sector.to_le_bytes();

            // Send read sector command
            let resp = self.send_cmd(0x53, &payload, SECTOR_READ_TIMEOUT)?;

            if resp.len() < SECTOR_SIZE {
                // Firmware may send data in chunks - collect them
                // For now, handle single-response case and pad
                let buf_offset = (i as usize) * SECTOR_SIZE;
                let copy_len = resp.len().min(SECTOR_SIZE);
                buffer[buf_offset..buf_offset + copy_len].copy_from_slice(&resp[..copy_len]);

                // If we got a partial response, try reading more chunks
                if copy_len < SECTOR_SIZE {
                    let mut total = copy_len;
                    let deadline = Instant::now() + SECTOR_READ_TIMEOUT;
                    while total < SECTOR_SIZE && Instant::now() < deadline {
                        // Read next chunk frame from serial
                        match self.read_next_frame(Duration::from_secs(2)) {
                            Ok(chunk) => {
                                if chunk.len() >= 4 {
                                    let offset = u16::from_le_bytes([chunk[0], chunk[1]]) as usize;
                                    let chunk_len = u16::from_le_bytes([chunk[2], chunk[3]]) as usize;
                                    let data = &chunk[4..];
                                    let copy = data.len().min(chunk_len).min(SECTOR_SIZE - offset);
                                    if offset + copy <= SECTOR_SIZE {
                                        buffer[buf_offset + offset..buf_offset + offset + copy]
                                            .copy_from_slice(&data[..copy]);
                                        total += copy;
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    if total < SECTOR_SIZE {
                        eprintln!(
                            "Warning: sector {} only got {}/{} bytes",
                            sector, total, SECTOR_SIZE
                        );
                    }
                }
            } else {
                // Full sector in single response
                let buf_offset = (i as usize) * SECTOR_SIZE;
                buffer[buf_offset..buf_offset + SECTOR_SIZE]
                    .copy_from_slice(&resp[..SECTOR_SIZE]);
            }
        }
        Ok(buffer)
    }

    /// Read the next COBS frame from the serial port
    fn read_next_frame(&mut self, timeout: Duration) -> Result<Vec<u8>, String> {
        let port = self.port.as_mut().ok_or("Not connected")?;
        let deadline = Instant::now() + timeout;
        let mut buf = Vec::new();
        let mut tmp = [0u8; 512];

        while Instant::now() < deadline {
            match port.read(&mut tmp) {
                Ok(n) if n > 0 => {
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(idx) = buf.iter().position(|&b| b == 0x00) {
                        let frame = buf[..=idx].to_vec();
                        let decoded = cobs_decode(&frame);
                        if decoded.len() >= 5 {
                            let plen = decoded[3] as usize;
                            if plen > 0 && decoded.len() >= 4 + plen {
                                return Ok(decoded[4..4 + plen].to_vec());
                            }
                        }
                        return Err("Invalid frame".into());
                    }
                }
                Ok(_) => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                Err(e) => return Err(format!("Serial read error: {}", e)),
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Err("Timeout reading frame".into())
    }

    /// Write one or more 8192-byte sectors to the device eMMC.
    ///
    /// Sends write_sector command (0x57) with sector number and data.
    /// Data is sent in chunks that fit within COBS frame limits.
    pub fn write_sectors(&mut self, start_sector: u32, data: &[u8]) -> Result<(), String> {
        if data.len() % SECTOR_SIZE != 0 {
            return Err("Data must be aligned to sector size".into());
        }
        let count = data.len() / SECTOR_SIZE;
        // Max payload per COBS frame is ~240 bytes (to stay under 254 COBS limit)
        const CHUNK_DATA_SIZE: usize = 224;

        for i in 0..count {
            let sector = start_sector + i as u32;
            let sector_data = &data[i * SECTOR_SIZE..(i + 1) * SECTOR_SIZE];

            // Send sector data in chunks
            let mut offset: usize = 0;
            while offset < SECTOR_SIZE {
                let chunk_len = CHUNK_DATA_SIZE.min(SECTOR_SIZE - offset);
                // Payload: [sector_u32_le, offset_u16_le, chunk_len_u16_le, data...]
                let mut payload = Vec::with_capacity(8 + chunk_len);
                payload.extend_from_slice(&sector.to_le_bytes());
                payload.extend_from_slice(&(offset as u16).to_le_bytes());
                payload.extend_from_slice(&(chunk_len as u16).to_le_bytes());
                payload.extend_from_slice(&sector_data[offset..offset + chunk_len]);

                let resp = self.send_cmd(0x57, &payload, SERIAL_TIMEOUT)?;
                // Check for ACK (first byte should be 0x06 or similar)
                if !resp.is_empty() && resp[0] == 0x15 {
                    return Err(format!(
                        "Write rejected at sector {} offset {}",
                        sector, offset
                    ));
                }

                offset += chunk_len;
            }
        }
        Ok(())
    }
}
