import { create } from "zustand";
import type { DeviceInfo } from "../utils/tauriCommands";

export type DeviceStorage = {
  totalBytes: number;
  usedBytes: number;
};

type DeviceState = {
  info: DeviceInfo | null;
  connected: boolean;
  connecting: boolean;
  error: string | null;
  storage: DeviceStorage;
  lastSeenAt: string | null;
  setConnecting: (connecting: boolean) => void;
  setDevice: (info: DeviceInfo | null) => void;
  setConnected: (connected: boolean) => void;
  setStorageUsage: (usedBytes: number, totalBytes?: number) => void;
  setError: (error: string | null) => void;
  reset: () => void;
};

const DEFAULT_TOTAL_BYTES = 8 * 1024 * 1024 * 1024;

export const useDeviceStore = create<DeviceState>((set) => ({
  info: null,
  connected: false,
  connecting: false,
  error: null,
  storage: {
    totalBytes: DEFAULT_TOTAL_BYTES,
    usedBytes: 0,
  },
  lastSeenAt: null,

  setConnecting: (connecting) => set({ connecting }),

  setDevice: (info) =>
    set({
      info,
      connected: Boolean(info?.connected),
      connecting: false,
      error: null,
      lastSeenAt: info?.connected ? new Date().toISOString() : null,
    }),

  setConnected: (connected) =>
    set((state) => ({
      connected,
      info: state.info ? { ...state.info, connected } : state.info,
      lastSeenAt: connected ? new Date().toISOString() : state.lastSeenAt,
    })),

  setStorageUsage: (usedBytes, totalBytes) =>
    set((state) => ({
      storage: {
        totalBytes: totalBytes ?? state.storage.totalBytes,
        usedBytes,
      },
    })),

  setError: (error) => set({ error, connecting: false }),

  reset: () =>
    set({
      info: null,
      connected: false,
      connecting: false,
      error: null,
      storage: {
        totalBytes: DEFAULT_TOTAL_BYTES,
        usedBytes: 0,
      },
      lastSeenAt: null,
    }),
}));
