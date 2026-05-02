pub mod usb;
pub mod audio;
pub mod firmware;

use std::sync::Mutex;
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
    let device = state.device.lock().map_err(|e| format!("{}", e))?;
    let sector_data_vec = device.read_sectors(0, 1)?;
    let sector_data: &[u8; 8192] = sector_data_vec[0..8192].try_into().map_err(|_| "Invalid sector size")?;
    let album = audio::metadata::parse_album(&sector_data).map_err(|e| format!("{}", e))?;
    serde_json::to_string(&album).map_err(|e| format!("{}", e))
}

#[tauri::command]
fn flash_firmware(state: State<AppState>, path: String) -> Result<(), String> {
    let device = state.device.lock().map_err(|e| format!("{}", e))?;
    let fw_data = firmware::FirmwareFlasher::load_firmware(std::path::Path::new(&path))?;
    firmware::FirmwareFlasher::flash(&device, &fw_data)
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
            flash_firmware,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
