import { create } from "zustand";
import type { AlbumMetadata, SongEntry, UploadProgressEvent } from "../utils/tauriCommands";
import { sectorCountToBytes } from "../utils/tauriCommands";

export type ManagedSong = SongEntry & {
  id: string;
  color: string;
  uploaded?: boolean;
};

export type ManagedAlbum = AlbumMetadata & {
  id: string;
  songs: ManagedSong[];
  updatedAt: string;
};

type AlbumState = {
  albums: ManagedAlbum[];
  activeAlbumId: string | null;
  uploadProgress: UploadProgressEvent | null;
  error: string | null;
  setAlbumsFromDevice: (album: AlbumMetadata) => void;
  addAlbum: (title?: string) => string;
  selectAlbum: (id: string) => void;
  updateAlbumTitle: (id: string, title: string) => void;
  addSong: (albumId: string, song: Partial<SongEntry>) => void;
  removeSong: (albumId: string, songId: string) => void;
  reorderSongs: (albumId: string, activeId: string, overId: string) => void;
  setUploadProgress: (progress: UploadProgressEvent | null) => void;
  markSongUploaded: (albumId: string, songId: string) => void;
  setError: (error: string | null) => void;
  getActiveAlbum: () => ManagedAlbum | undefined;
};

const colors = ["#f8fafc", "#a7f3d0", "#fde68a", "#f9a8d4", "#93c5fd", "#c4b5fd"];

const createId = () => crypto.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;

const normalizeSong = (song: SongEntry, index: number): ManagedSong => ({
  id: song.id ?? createId(),
  title: song.title || `Track ${index + 1}`,
  artist: song.artist || "Unknown Artist",
  start_sector: song.start_sector ?? 1,
  length_sectors: song.length_sectors ?? 0,
  durationSeconds: song.durationSeconds,
  color: colors[index % colors.length],
  uploaded: false,
});

const normalizeAlbum = (album: AlbumMetadata, index = 0): ManagedAlbum => ({
  id: album.id ?? createId(),
  title: album.title || `Stem Player Album ${index + 1}`,
  songs: album.songs.map(normalizeSong),
  updatedAt: new Date().toISOString(),
});

export const albumStorageBytes = (album: Pick<ManagedAlbum, "songs">) =>
  album.songs.reduce((total, song) => total + sectorCountToBytes(song.length_sectors), 0);

export const useAlbumStore = create<AlbumState>((set, get) => ({
  albums: [
    normalizeAlbum({
      title: "Demo Set",
      songs: [
        {
          title: "Four Channel Sunrise",
          artist: "Teenage Engineering",
          start_sector: 1,
          length_sectors: 1280,
        },
        {
          title: "Pocket Operator Choir",
          artist: "Studio Draft",
          start_sector: 1281,
          length_sectors: 980,
        },
      ],
    }),
  ],
  activeAlbumId: null,
  uploadProgress: null,
  error: null,

  setAlbumsFromDevice: (album) => {
    const normalizedAlbum = normalizeAlbum(album);

    set({
      albums: [normalizedAlbum],
      activeAlbumId: normalizedAlbum.id,
      error: null,
    });
  },

  addAlbum: (title = "Untitled Album") => {
    const id = createId();
    const album = normalizeAlbum({ id, title, songs: [] });
    set((state) => ({
      albums: [...state.albums, album],
      activeAlbumId: id,
    }));
    return id;
  },

  selectAlbum: (id) => set({ activeAlbumId: id }),

  updateAlbumTitle: (id, title) =>
    set((state) => ({
      albums: state.albums.map((album) =>
        album.id === id ? { ...album, title, updatedAt: new Date().toISOString() } : album,
      ),
    })),

  addSong: (albumId, song) =>
    set((state) => ({
      albums: state.albums.map((album) =>
        album.id === albumId
          ? {
              ...album,
              songs: [
                ...album.songs,
                normalizeSong(
                  {
                    title: song.title ?? "New Stem Track",
                    artist: song.artist ?? "Unknown Artist",
                    start_sector: song.start_sector ?? 1,
                    length_sectors: song.length_sectors ?? 0,
                  },
                  album.songs.length,
                ),
              ],
              updatedAt: new Date().toISOString(),
            }
          : album,
      ),
    })),

  removeSong: (albumId, songId) =>
    set((state) => ({
      albums: state.albums.map((album) =>
        album.id === albumId
          ? ({
              ...album,
              songs: album.songs.filter((song): song is ManagedSong => song.id !== songId),
              updatedAt: new Date().toISOString(),
            } as ManagedAlbum)
          : album,
      ),
    })),

  reorderSongs: (albumId, activeId, overId) =>
    set((state) => ({
      albums: state.albums.map((album) => {
        if (album.id !== albumId || activeId === overId) {
          return album;
        }

        const oldIndex = album.songs.findIndex((song) => song.id === activeId);
        const newIndex = album.songs.findIndex((song) => song.id === overId);

        if (oldIndex < 0 || newIndex < 0) {
          return album;
        }

        const songs: ManagedSong[] = [...album.songs];
        const [moved] = songs.splice(oldIndex, 1);
        songs.splice(newIndex, 0, moved);

        return {
          ...album,
          songs,
          updatedAt: new Date().toISOString(),
        } as ManagedAlbum;
      }),
    })),

  setUploadProgress: (uploadProgress) => set({ uploadProgress }),

  markSongUploaded: (albumId, songId) =>
    set((state) => ({
      albums: state.albums.map((album) =>
        album.id === albumId
          ? ({
              ...album,
              songs: album.songs.map((song) =>
                song.id === songId ? { ...song, uploaded: true } as ManagedSong : song as ManagedSong,
              ),
            } as ManagedAlbum)
          : album,
      ),
    })),

  setError: (error) => set({ error }),

  getActiveAlbum: () => {
    const state = get();
    return state.albums.find((album) => album.id === state.activeAlbumId) ?? state.albums[0];
  },
}));
