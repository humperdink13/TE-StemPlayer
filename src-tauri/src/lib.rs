pub mod usb;
pub mod audio;
pub mod firmware;

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
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
    let sector_data: &[u8; 8192] = sector_data_vec[0..8192]
        .try_into()
        .map_err(|_| "Invalid sector size")?;
    let album = audio::metadata::parse_album(sector_data).map_err(|e| format!("{}", e))?;
    serde_json::to_string(&album).map_err(|e| format!("{}", e))
}

#[tauri::command]
fn write_album_metadata(state: State<AppState>, album: audio::metadata::Album) -> Result<(), String> {
    let sector = audio::metadata::serialize_album(&album);
    let device = state.device.lock().map_err(|e| format!("{}", e))?;
    device.write_sectors(0, &sector)
}

#[derive(serde::Deserialize)]
pub struct StemSource {
    #[serde(rename = "type")]
    source_type: String,
    value: String,
}

#[tauri::command]
async fn start_separation(
    app: AppHandle,
    source: StemSource,
) -> Result<serde_json::Value, String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let job_id_clone = job_id.clone();

    let _ = app.emit("separation-progress", serde_json::json!({
        "jobId": &job_id,
        "stage": "queued",
        "percent": 0,
        "message": "Job queued"
    }));

    let app_handle = app.clone();
    tokio::spawn(async move {
        let result = run_separation_pipeline(&app_handle, &job_id_clone, &source).await;
        if let Err(e) = result {
            let _ = app_handle.emit("separation-progress", serde_json::json!({
                "jobId": &job_id_clone,
                "stage": "error",
                "percent": 0,
                "message": format!("Separation failed: {}", e)
            }));
        }
    });

    Ok(serde_json::json!({ "jobId": job_id }))
}

async fn run_separation_pipeline(
    app: &AppHandle,
    job_id: &str,
    source: &StemSource,
) -> Result<(), String> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Failed to get resource dir: {}", e))?;
    let sidecar_dir = resource_dir.join("sidecar");

    let sidecar_dir = if sidecar_dir.exists() {
        sidecar_dir
    } else {
        let dev_sidecar = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("sidecar");
        if dev_sidecar.exists() {
            dev_sidecar
        } else {
            return Err("Sidecar directory not found".to_string());
        }
    };

    let output_dir = std::env::temp_dir().join("stem-app").join(job_id);
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("Failed to create output dir: {}", e))?;

    let audio_path = if source.source_type == "youtube" {
        let _ = app.emit("separation-progress", serde_json::json!({
            "jobId": job_id,
            "stage": "downloading",
            "percent": 5,
            "message": "Downloading audio from YouTube..."
        }));

        let downloader = sidecar_dir.join("downloader.py");
        let output_wav = output_dir.join("source.wav");

        // Ensure Homebrew binaries (ffmpeg, etc.) are on PATH
        let path_env = format!("/opt/homebrew/bin:/usr/local/bin:{}", std::env::var("PATH").unwrap_or_default());

        let mut child = Command::new("python3")
            .env("PATH", &path_env)
            .arg(&downloader)
            .arg("--url")
            .arg(&source.value)
            .arg("--output")
            .arg(&output_wav)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn downloader: {}", e))?;

        if let Some(stdout) = child.stdout.take() {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                    let percent = msg.get("percent").and_then(|v| v.as_f64()).unwrap_or(10.0);
                    let message = msg.get("message").and_then(|v| v.as_str()).unwrap_or("Downloading...");
                    let _ = app.emit("separation-progress", serde_json::json!({
                        "jobId": job_id,
                        "stage": "downloading",
                        "percent": percent as u32,
                        "message": message
                    }));
                }
            }
        }

        let status = child.wait().await.map_err(|e| format!("Downloader error: {}", e))?;
        if !status.success() {
            return Err("YouTube download failed".to_string());
        }

        output_dir.join("source.wav")
    } else {
        std::path::PathBuf::from(&source.value)
    };

    let _ = app.emit("separation-progress", serde_json::json!({
        "jobId": job_id,
        "stage": "separating",
        "percent": 30,
        "message": "Running Demucs stem separation..."
    }));

    let separator = sidecar_dir.join("stem_separator.py");
    let path_env = format!("/opt/homebrew/bin:/usr/local/bin:{}", std::env::var("PATH").unwrap_or_default());
    let mut child = Command::new("python3")
        .env("PATH", &path_env)
        .arg(&separator)
        .arg("--input")
        .arg(&audio_path)
        .arg("--output")
        .arg(&output_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn stem separator: {}", e))?;

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                let percent = msg.get("percent").and_then(|v| v.as_f64()).unwrap_or(50.0);
                let stage = msg.get("stage").and_then(|v| v.as_str()).unwrap_or("separating");
                let message = msg.get("message").and_then(|v| v.as_str()).unwrap_or("Separating...");
                let _ = app.emit("separation-progress", serde_json::json!({
                    "jobId": job_id,
                    "stage": stage,
                    "percent": percent as u32,
                    "message": message
                }));
            }
        }
    }

    let status = child.wait().await.map_err(|e| format!("Separator error: {}", e))?;
    if !status.success() {
        return Err("Stem separation failed".to_string());
    }

    let stems: Vec<String> = ["vocals", "drums", "bass", "other"]
        .iter()
        .map(|name| {
            output_dir
                .join(format!("{}.wav", name))
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let _ = app.emit("separation-progress", serde_json::json!({
        "jobId": job_id,
        "stage": "complete",
        "percent": 100,
        "message": "Separation complete",
        "stems": stems
    }));

    let _ = app.emit("separation-done", serde_json::json!({
        "jobId": job_id,
        "stage": "complete",
        "percent": 100,
        "stems": stems
    }));

    Ok(())
}

#[tauri::command]
fn upload_song(
    state: State<AppState>,
    _album: audio::metadata::Album,
    song: audio::metadata::Song,
) -> Result<(), String> {
    let device = state.device.lock().map_err(|e| format!("{}", e))?;
    if !device.is_connected() {
        return Err("No device connected. Please connect your Stem Player first.".to_string());
    }

    Err(format!(
        "Upload pipeline not yet complete. Song '{}' by '{}' validated. \
         To fully upload, the sector packing pipeline needs stem WAV file paths \
         which will come from the separation job output.",
        song.title, song.artist
    ))
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
            write_album_metadata,
            start_separation,
            upload_song,
            flash_firmware,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
