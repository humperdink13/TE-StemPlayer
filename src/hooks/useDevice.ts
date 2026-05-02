import { useCallback, useEffect } from "react";
import { tauriCommands, tauriEvents } from "../utils/tauriCommands";
import { useAlbumStore } from "../stores/albumStore";
import { useDeviceStore } from "../stores/deviceStore";
import { sectorCountToBytes } from "../utils/tauriCommands";

export const useDevice = () => {
  const {
    connected,
    connecting,
    error,
    info,
    lastSeenAt,
    storage,
    setConnecting,
    setDevice,
    setConnected,
    setStorageUsage,
    setError,
    reset,
  } = useDeviceStore();
  const setAlbumsFromDevice = useAlbumStore((state) => state.setAlbumsFromDevice);

  const refreshAlbumMetadata = useCallback(async () => {
    try {
      const album = await tauriCommands.readAlbumMetadata();
      setAlbumsFromDevice(album);
      const usedBytes = album.songs.reduce(
        (total, song) => total + sectorCountToBytes(song.length_sectors),
        0,
      );
      setStorageUsage(usedBytes);
      return album;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return null;
    }
  }, [setAlbumsFromDevice, setError, setStorageUsage]);

  const connect = useCallback(async () => {
    setConnecting(true);
    setError(null);

    try {
      const device = await tauriCommands.connectDevice();
      setDevice(device);
      await refreshAlbumMetadata();
      return device;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      throw err;
    }
  }, [refreshAlbumMetadata, setConnecting, setDevice, setError]);

  const disconnect = useCallback(async () => {
    try {
      await tauriCommands.disconnectDevice();
    } finally {
      reset();
    }
  }, [reset]);

  const refreshStatus = useCallback(async () => {
    try {
      const isConnected = await tauriCommands.getDeviceStatus();
      setConnected(isConnected);
      return isConnected;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return false;
    }
  }, [setConnected, setError]);

  useEffect(() => {
    let mounted = true;

    const setup = async () => {
      const unlisteners = await Promise.all([
        tauriEvents.onDeviceConnected((device) => {
          if (!mounted) {
            return;
          }

          setDevice(device);
          void refreshAlbumMetadata();
        }),
        tauriEvents.onDeviceDisconnected(() => {
          if (mounted) {
            reset();
          }
        }),
      ]);

      return () => unlisteners.forEach((unlisten) => unlisten());
    };

    let cleanup: (() => void) | undefined;
    void setup().then((teardown) => {
      cleanup = teardown;
    });

    void refreshStatus();

    return () => {
      mounted = false;
      cleanup?.();
    };
  }, [refreshAlbumMetadata, refreshStatus, reset, setDevice]);

  return {
    connected,
    connecting,
    error,
    info,
    lastSeenAt,
    storage,
    connect,
    disconnect,
    refreshAlbumMetadata,
    refreshStatus,
  };
};
