import { useCallback, useEffect, useState } from "react";
import { tauriCommands, tauriEvents, type FlashProgressEvent } from "../utils/tauriCommands";

export const useFirmware = () => {
  const [firmwarePath, setFirmwarePath] = useState("");
  const [progress, setProgress] = useState<FlashProgressEvent | null>(null);
  const [flashing, setFlashing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const flashFirmware = useCallback(async () => {
    const path = firmwarePath.trim();

    if (!path) {
      setError("Choose a firmware binary before flashing.");
      return;
    }

    setFlashing(true);
    setError(null);
    setProgress({
      stage: "validating",
      percent: 1,
      message: "Validating firmware image",
    });

    try {
      await tauriCommands.flashFirmware(path);
      setProgress({
        stage: "complete",
        percent: 100,
        message: "Firmware flash complete",
      });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      setProgress({
        stage: "error",
        percent: 0,
        message,
      });
    } finally {
      setFlashing(false);
    }
  }, [firmwarePath]);

  useEffect(() => {
    let mounted = true;

    const setup = async () => {
      const unlisteners = await Promise.all([
        tauriEvents.onFlashProgress((event) => {
          if (mounted) {
            setProgress(event);
          }
        }),
        tauriEvents.onFlashDone(() => {
          if (mounted) {
            setProgress({
              stage: "complete",
              percent: 100,
              message: "Firmware flash complete",
            });
            setFlashing(false);
          }
        }),
      ]);

      return () => unlisteners.forEach((unlisten) => unlisten());
    };

    let cleanup: (() => void) | undefined;
    void setup().then((teardown) => {
      cleanup = teardown;
    });

    return () => {
      mounted = false;
      cleanup?.();
    };
  }, []);

  return {
    firmwarePath,
    setFirmwarePath,
    progress,
    flashing,
    error,
    flashFirmware,
  };
};
