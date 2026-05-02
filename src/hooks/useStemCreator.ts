import { useCallback, useEffect } from "react";
import { tauriCommands, tauriEvents, type StemSource } from "../utils/tauriCommands";
import { useStemStore } from "../stores/stemStore";

export const useStemCreator = () => {
  const {
    youtubeUrl,
    selectedFilePath,
    activeJobId,
    jobs,
    error,
    setYoutubeUrl,
    setSelectedFilePath,
    createJob,
    updateJobProgress,
    setError,
    getActiveJob,
  } = useStemStore();

  const startSeparation = useCallback(
    async (source: StemSource) => {
      const optimisticJobId = createJob(source);

      try {
        const result = await tauriCommands.startSeparation(source);

        if (result.jobId !== optimisticJobId) {
          updateJobProgress({
            jobId: optimisticJobId,
            stage: "queued",
            percent: 5,
            message: `Backend accepted job ${result.jobId}`,
          });
        }

        return result.jobId;
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        updateJobProgress({
          jobId: optimisticJobId,
          stage: "error",
          percent: 0,
          message,
        });
        setError(message);
        return optimisticJobId;
      }
    },
    [createJob, setError, updateJobProgress],
  );

  const startYoutubeSeparation = useCallback(() => {
    const trimmed = youtubeUrl.trim();

    if (!trimmed) {
      setError("Paste a YouTube URL before starting separation.");
      return Promise.resolve(null);
    }

    return startSeparation({ type: "youtube", value: trimmed });
  }, [setError, startSeparation, youtubeUrl]);

  const startFileSeparation = useCallback(() => {
    const trimmed = selectedFilePath.trim();

    if (!trimmed) {
      setError("Enter or select an audio file path before starting separation.");
      return Promise.resolve(null);
    }

    return startSeparation({ type: "file", value: trimmed });
  }, [selectedFilePath, setError, startSeparation]);

  useEffect(() => {
    let mounted = true;

    const setup = async () => {
      const unlisteners = await Promise.all([
        tauriEvents.onSeparationProgress((event) => {
          if (mounted) {
            updateJobProgress(event);
          }
        }),
        tauriEvents.onSeparationDone((event) => {
          if (mounted) {
            updateJobProgress(event);
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
  }, [updateJobProgress]);

  return {
    youtubeUrl,
    selectedFilePath,
    activeJobId,
    jobs,
    activeJob: getActiveJob(),
    error,
    setYoutubeUrl,
    setSelectedFilePath,
    startSeparation,
    startYoutubeSeparation,
    startFileSeparation,
  };
};
