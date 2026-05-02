pub mod usb;
pub mod audio;
pub mod firmware;
pub mod backup;

use std::sync::Mutex;
use std::path::Path;
use tauri::State;
use usb::{DeviceInfo, StemPlayerDevice};

pub struct AppState {
    device: Mutex<StemPlayerDevice>,
}

#[tauri::command]
fn connect_device(state: State<AppState>) -> Result<DeviceInfo, String> {
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    device.find_and_connect()
}

#[tauri::command]
fn disconnect_device(state: State<AppState>) -> Result<(), String> {
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    device.disconnect();
    Ok(())
}

#[tauri::command]
fn get_device_status(state: State<AppState>) -> Result<bool, String> {
    let device = state.device.lock().map_err(|e| format!("{}", e))?;
    Ok(device.is_connected())
}

#[tauri::command]
fn read_album_metadata(state: State<AppState>) -> Result<String, String> {
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    let sector_data = device.read_sector(0)?;
    let album = audio::metadata::AlbumMetadata::from_sector(&sector_data)?;
    serde_json::to_string(&album).map_err(|e| format!("{}", e))
}

#[tauri::command]
fn write_album_metadata(state: State<AppState>, metadata_json: String) -> Result<(), String> {
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    let album: audio::metadata::AlbumMetadata =
        serde_json::from_str(&metadata_json).map_err(|e| format!("Bad metadata: {}", e))?;
    let sector = album.to_sector();
    device.write_sector(0, &sector)
}

#[tauri::command]
fn upload_song_sectors(
    state: State<AppState>,
    start_sector: u32,
    sectors_b64: Vec<String>,
) -> Result<(), String> {
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    for (i, b64) in sectors_b64.iter().enumerate() {
        let data = base64_decode(b64)?;
        device.write_sector(start_sector + i as u32, &data)?;
    }
    Ok(())
}

#[tauri::command]
fn read_sectors(state: State<AppState>, start: u32, count: u32) -> Result<Vec<String>, String> {
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    let mut result = Vec::with_capacity(count as usize);
    for i in 0..count {
        let data = device.read_sector(start + i)?;
        result.push(base64_encode(&data));
    }
    Ok(result)
}

#[tauri::command]
fn backup_device(state: State<AppState>, output_path: String, sector_count: u32) -> Result<String, String> {
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    let mut sectors = Vec::with_capacity(sector_count as usize);
    for i in 0..sector_count {
        sectors.push(device.read_sector(i)?);
    }
    backup::create_backup(&sectors, Path::new(&output_path))
}

#[tauri::command]
fn restore_backup(state: State<AppState>, backup_path: String) -> Result<u32, String> {
    let sectors = backup::load_backup(Path::new(&backup_path))?;
    let mut device = state.device.lock().map_err(|e| format!("{}", e))?;
    let count = sectors.len() as u32;
    for (i, sector) in sectors.iter().enumerate() {
        device.write_sector(i as u32, sector)?;
    }
    Ok(count)
}

#[tauri::command]
fn flash_firmware(state: State<AppState>, path: String) -> Result<(), String> {
    let device = state.device.lock().map_err(|e| format!("{}", e))?;
    let fw_data = firmware::FirmwareFlasher::load_firmware(std::path::Path::new(&path))?;
    firmware::FirmwareFlasher::flash(&device, &fw_data)
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 { out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char); } else { out.push('='); }
        if chunk.len() > 2 { out.push(CHARS[(triple & 0x3F) as usize] as char); } else { out.push('='); }
    }
    out
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62), b'/' => Ok(63),
            _ => Err("Invalid base64".into()),
        }
    }
    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'=' && b != b'\n' && b != b'\r').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        out.push(((a << 2) | (b >> 4)) as u8);
        if chunk.len() > 2 { let c = val(chunk[2])?; out.push((((b & 0xF) << 4) | (c >> 2)) as u8);
            if chunk.len() > 3 { let d = val(chunk[3])?; out.push((((c & 0x3) << 6) | d) as u8); }
        }
    }
    Ok(out)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            device: Mutex::new(StemPlayerDevice::new()),
        })
        .invoke_handler(tauri::generate_handler![
            connect_device,
            disconnect_device,
            get_device_status,
            read_album_metadata,
            write_album_metadata,
            upload_song_sectors,
            read_sectors,
            backup_device,
            restore_backup,
            flash_firmware,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
