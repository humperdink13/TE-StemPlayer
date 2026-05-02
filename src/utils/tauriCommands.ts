import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DeviceInfo = {
  connected: boolean;
  serial: string | null;
  firmware_version: string | null;
  data_transfer_supported: boolean;
  needs_firmware_update: boolean;
  status_message: string | null;
};

export type SongEntry = {
  id?: string;
  start_sector: number;
  length_sectors: number;
  artist: string;
  title: string;
  durationSeconds?: number;
};

export type AlbumMetadata = {
  id?: string;
  title: string;
  songs: SongEntry[];
};

export type StemSource = {
  type: "youtube" | "file";
  value: string;
};

export type SeparationProgressEvent = {
  jobId: string;
  stage: "queued" | "downloading" | "separating" | "normalizing" | "packing" | "complete" | "error";
  percent: number;
  message?: string;
  stems?: string[];
  previewUrl?: string;
};

export type UploadProgressEvent = {
  albumId?: string;
  songId?: string;
  percent: number;
  message?: string;
};

export type FlashProgressEvent = {
  percent: number;
  stage: "validating" | "erasing" | "writing" | "verifying" | "complete" | "error" | "waiting_bootloader" | "flashing";
  message: string;
};

const parseAlbumMetadata = (raw: string | AlbumMetadata): AlbumMetadata => {
  if (typeof raw === "string") {
    return JSON.parse(raw) as AlbumMetadata;
  }

  return raw;
};

export const tauriCommands = {
  connectDevice: () => invoke<DeviceInfo>("connect_device"),

  disconnectDevice: () => invoke<void>("disconnect_device"),

  getDeviceStatus: () => invoke<boolean>("get_device_status"),

  async readAlbumMetadata() {
    const raw = await invoke<string | AlbumMetadata>("read_album_metadata");
    return parseAlbumMetadata(raw);
  },

  /**
   * Backend command planned in ARCHITECTURE.md. The hook/store layer catches
   * "command not found" and keeps a useful queued job in the UI until the Rust
   * side exposes the implementation.
   */
  startSeparation: (source: StemSource) =>
    invoke<{ jobId: string }>("start_separation", { source }),

  uploadSong: (album: AlbumMetadata, song: SongEntry) =>
    invoke<void>("upload_song", { album, song }),

  writeAlbumMetadata: (album: AlbumMetadata) =>
    invoke<void>("write_album_metadata", { album }),

  flashFirmware: (path: string) => invoke<void>("flash_firmware", { path }),
  startFirmwareFlash: () => invoke<{ status: string }>("start_firmware_flash"),
};

export const tauriEvents = {
  onDeviceConnected: (handler: (event: DeviceInfo) => void): Promise<UnlistenFn> =>
    listen<DeviceInfo>("device-connected", (event) => handler(event.payload)),

  onDeviceDisconnected: (handler: () => void): Promise<UnlistenFn> =>
    listen("device-disconnected", () => handler()),

  onSeparationProgress: (
    handler: (event: SeparationProgressEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<SeparationProgressEvent>("separation-progress", (event) =>
      handler(event.payload),
    ),

  onSeparationDone: (
    handler: (event: SeparationProgressEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<SeparationProgressEvent>("separation-done", (event) =>
      handler({ ...event.payload, stage: "complete", percent: 100 }),
    ),

  onUploadProgress: (
    handler: (event: UploadProgressEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<UploadProgressEvent>("upload-progress", (event) =>
      handler(event.payload),
    ),

  onFlashProgress: (
    handler: (event: FlashProgressEvent) => void,
  ): Promise<UnlistenFn> =>
    listen<FlashProgressEvent>("flash-progress", (event) =>
      handler(event.payload),
    ),

  onFlashDone: (handler: () => void): Promise<UnlistenFn> =>
    listen("flash-done", () => handler()),
};

export const formatBytes = (bytes: number) => {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;

  return `${value.toFixed(value >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
};

export const sectorCountToBytes = (sectors: number) => sectors * 8192;
