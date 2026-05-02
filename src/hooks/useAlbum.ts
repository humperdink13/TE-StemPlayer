import { useCallback, useEffect } from "react";
import { tauriCommands, tauriEvents, type AlbumMetadata } from "../utils/tauriCommands";
import { useAlbumStore, type ManagedAlbum, type ManagedSong } from "../stores/albumStore";

const toAlbumMetadata = (album: ManagedAlbum): AlbumMetadata => ({
  id: album.id,
  title: album.title,
  songs: album.songs.map((song) => ({
    id: song.id,
    title: song.title,
    artist: song.artist,
    start_sector: song.start_sector,
    length_sectors: song.length_sectors,
    durationSeconds: song.durationSeconds,
  })),
});

export const useAlbum = () => {
  const {
    albums,
    activeAlbumId,
    uploadProgress,
    error,
    addAlbum,
    selectAlbum,
    updateAlbumTitle,
    addSong,
    removeSong,
    reorderSongs,
    setUploadProgress,
    markSongUploaded,
    setError,
    getActiveAlbum,
  } = useAlbumStore();

  const activeAlbum = getActiveAlbum();

  const syncAlbumMetadata = useCallback(
    async (album = activeAlbum) => {
      if (!album) {
        setError("Create or select an album before syncing metadata.");
        return;
      }

      try {
        await tauriCommands.writeAlbumMetadata(toAlbumMetadata(album));
        setError(null);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      }
    },
    [activeAlbum, setError],
  );

  const uploadSong = useCallback(
    async (album: ManagedAlbum, song: ManagedSong) => {
      setUploadProgress({
        albumId: album.id,
        songId: song.id,
        percent: 1,
        message: `Preparing ${song.title}`,
      });

      try {
        await tauriCommands.uploadSong(toAlbumMetadata(album), song);
        markSongUploaded(album.id, song.id);
        setUploadProgress({
          albumId: album.id,
          songId: song.id,
          percent: 100,
          message: `${song.title} uploaded`,
        });
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        setUploadProgress(null);
      }
    },
    [markSongUploaded, setError, setUploadProgress],
  );

  useEffect(() => {
    let mounted = true;

    const setup = async () => {
      const unlisten = await tauriEvents.onUploadProgress((progress) => {
        if (mounted) {
          setUploadProgress(progress);
        }
      });

      return () => unlisten();
    };

    let cleanup: (() => void) | undefined;
    void setup().then((teardown) => {
      cleanup = teardown;
    });

    return () => {
      mounted = false;
      cleanup?.();
    };
  }, [setUploadProgress]);

  return {
    albums,
    activeAlbum,
    activeAlbumId,
    uploadProgress,
    error,
    addAlbum,
    selectAlbum,
    updateAlbumTitle,
    addSong,
    removeSong,
    reorderSongs,
    syncAlbumMetadata,
    uploadSong,
  };
};
