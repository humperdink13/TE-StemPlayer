import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ─── Shared Types ────────────────────────────────────────────────────────────

export interface DeviceInfo {
  connected: boolean;
  port_name: string | null;
  serial: string | null;
  firmware_version: string | null;
}

export interface SongEntry {
  id?: string;
  title: string;
  artist: string;
  start_sector: number;
  length_sectors: number;
  durationSeconds?: number;
}

export interface AlbumMetadata {
  id?: string;
  title?: string;
  songs: SongEntry[];
}

export interface UploadProgressEvent {
  albumId: string;
  songId: string;
  percent: number;
  message?: string;
}

export interface FlashProgressEvent {
  stage: string;
  percent: number;
  message?: string;
}

export type StemSource =
  | { type: "youtube"; value: string }
  | { type: "file"; value: string };

export interface SeparationProgressEvent {
  jobId: string;
  stage: "queued" | "downloading" | "separating" | "encoding" | "complete" | "error";
  percent: number;
  message?: string;
  stems?: string[];
  previewUrl?: string;
}

export interface SeparationResult {
  jobId: string;
}

// ─── Utilities ───────────────────────────────────────────────────────────────

const SECTOR_SIZE = 8192;

export function sectorCountToBytes(sectors: number): number {
  return sectors * SECTOR_SIZE;
}

export function formatBytes(bytes: number, decimals = 1): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(k)), sizes.length - 1);
  const value = bytes / Math.pow(k, i);
  return `${value.toFixed(decimals)} ${sizes[i]}`;
}

// ─── Backend Adapter ─────────────────────────────────────────────────────────
// The Rust backend uses a simpler SongEntry {start_sector, sector_count, name}.
// We translate between frontend and backend representations here.

interface BackendSongEntry {
  start_sector: number;
  sector_count: number;
  name: string;
}

interface BackendAlbumMetadata {
  songs: BackendSongEntry[];
}

function toBackendAlbum(album: AlbumMetadata): BackendAlbumMetadata {
  return {
    songs: album.songs.map((song) => ({
      start_sector: song.start_sector,
      sector_count: song.length_sectors,
      name: song.title + (song.artist ? ` - ${song.artist}` : ""),
    })),
  };
}

function fromBackendAlbum(backend: BackendAlbumMetadata): AlbumMetadata {
  return {
    songs: backend.songs.map((song) => {
      const parts = song.name.split(" - ");
      return {
        title: parts[0] || "Untitled",
        artist: parts.length > 1 ? parts.slice(1).join(" - ") : "Unknown Artist",
        start_sector: song.start_sector,
        length_sectors: song.sector_count,
      };
    }),
  };
}

// ─── Tauri Commands (namespaced) ─────────────────────────────────────────────

export const tauriCommands = {
  connectDevice: (): Promise<DeviceInfo> => invoke<DeviceInfo>("connect_device"),

  disconnectDevice: (): Promise<void> => invoke("disconnect_device"),

  getDeviceStatus: (): Promise<boolean> => invoke<boolean>("get_device_status"),

  readAlbumMetadata: async (): Promise<AlbumMetadata> => {
    const json = await invoke<string>("read_album_metadata");
    const backend: BackendAlbumMetadata = JSON.parse(json);
    return fromBackendAlbum(backend);
  },

  writeAlbumMetadata: async (metadata: AlbumMetadata): Promise<void> => {
    const backend = toBackendAlbum(metadata);
    return invoke("write_album_metadata", {
      metadataJson: JSON.stringify(backend),
    });
  },

  uploadSongSectors: (startSector: number, sectorsB64: string[]): Promise<void> =>
    invoke("upload_song_sectors", { startSector, sectorsB64 }),

  uploadSong: async (_album: AlbumMetadata, song: SongEntry): Promise<void> => {
    // Stub: In the future this will read local stem files, encode them, and
    // upload sector-by-sector. For now it writes placeholder sectors.
    const placeholder = btoa(String.fromCharCode(...new Uint8Array(SECTOR_SIZE)));
    const sectors = Array.from({ length: song.length_sectors }, () => placeholder);
    await invoke("upload_song_sectors", {
      startSector: song.start_sector,
      sectorsB64: sectors,
    });
  },

  readSectors: (start: number, count: number): Promise<string[]> =>
    invoke<string[]>("read_sectors", { start, count }),

  backupDevice: (outputPath: string, sectorCount: number): Promise<string> =>
    invoke<string>("backup_device", { outputPath, sectorCount }),

  restoreBackup: (backupPath: string): Promise<number> =>
    invoke<number>("restore_backup", { backupPath }),

  flashFirmware: (path: string): Promise<void> => invoke("flash_firmware", { path }),

  startSeparation: async (_source: StemSource): Promise<SeparationResult> => {
    // Stub: Stem separation backend (Demucs) is not yet implemented.
    // Returns a fake job ID so the UI flow works.
    return { jobId: crypto.randomUUID?.() ?? `${Date.now()}` };
  },
};

// ─── Tauri Events (namespaced) ───────────────────────────────────────────────

export const tauriEvents = {
  onDeviceConnected: (handler: (device: DeviceInfo) => void): Promise<UnlistenFn> =>
    listen<DeviceInfo>("device-connected", (event) => handler(event.payload)),

  onDeviceDisconnected: (handler: () => void): Promise<UnlistenFn> =>
    listen("device-disconnected", () => handler()),

  onUploadProgress: (handler: (progress: UploadProgressEvent) => void): Promise<UnlistenFn> =>
    listen<UploadProgressEvent>("upload-progress", (event) => handler(event.payload)),

  onFlashProgress: (handler: (progress: FlashProgressEvent) => void): Promise<UnlistenFn> =>
    listen<FlashProgressEvent>("flash-progress", (event) => handler(event.payload)),

  onFlashDone: (handler: () => void): Promise<UnlistenFn> =>
    listen("flash-done", () => handler()),

  onSeparationProgress: (handler: (event: SeparationProgressEvent) => void): Promise<UnlistenFn> =>
    listen<SeparationProgressEvent>("separation-progress", (event) => handler(event.payload)),

  onSeparationDone: (handler: (event: SeparationProgressEvent) => void): Promise<UnlistenFn> =>
    listen<SeparationProgressEvent>("separation-done", (event) => handler(event.payload)),
};

// ─── Legacy flat exports (backward compatibility) ────────────────────────────

export const connectDevice = tauriCommands.connectDevice;
export const disconnectDevice = tauriCommands.disconnectDevice;
export const getDeviceStatus = tauriCommands.getDeviceStatus;
export const readAlbumMetadata = tauriCommands.readAlbumMetadata;
export const writeAlbumMetadata = tauriCommands.writeAlbumMetadata;
export const uploadSongSectors = tauriCommands.uploadSongSectors;
export const readSectors = tauriCommands.readSectors;
export const backupDevice = tauriCommands.backupDevice;
export const restoreBackup = tauriCommands.restoreBackup;
export const flashFirmware = tauriCommands.flashFirmware;
