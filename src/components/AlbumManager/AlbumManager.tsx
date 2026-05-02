import {
  closestCenter,
  DndContext,
  type DragEndEvent,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { useMemo } from "react";
import { useAlbum } from "../../hooks/useAlbum";
import { albumStorageBytes, type ManagedAlbum, type ManagedSong } from "../../stores/albumStore";
import { formatBytes } from "../../utils/tauriCommands";

const SortableSong = ({
  album,
  song,
  index,
  onUpload,
  onRemove,
}: {
  album: ManagedAlbum;
  song: ManagedSong;
  index: number;
  onUpload: (album: ManagedAlbum, song: ManagedSong) => void;
  onRemove: (albumId: string, songId: string) => void;
}) => {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: song.id,
  });

  const style = {
    transform: transform
      ? `translate3d(${Math.round(transform.x)}px, ${Math.round(transform.y)}px, 0) scaleX(${transform.scaleX}) scaleY(${transform.scaleY})`
      : undefined,
    transition,
  };

  return (
    <article
      className={`sortable-song ${isDragging ? "sortable-song--dragging" : ""}`}
      ref={setNodeRef}
      style={style}
    >
      <button className="drag-handle" {...attributes} {...listeners} aria-label={`Drag ${song.title}`}>
        ⋮⋮
      </button>
      <div className="track-index" style={{ borderColor: song.color }}>
        {String(index + 1).padStart(2, "0")}
      </div>
      <div className="song-main">
        <strong>{song.title}</strong>
        <span>{song.artist}</span>
      </div>
      <div className="song-row__meta">
        <span>{formatBytes(song.length_sectors * 8192)}</span>
        <span>{song.uploaded ? "uploaded" : "local"}</span>
      </div>
      <button className="secondary-action compact" onClick={() => onUpload(album, song)}>
        Upload
      </button>
      <button className="ghost-action compact" onClick={() => onRemove(album.id, song.id)}>
        Remove
      </button>
    </article>
  );
};

const AlbumSidebar = () => {
  const { albums, activeAlbum, addAlbum, selectAlbum } = useAlbum();

  return (
    <section className="panel album-sidebar">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Library</p>
          <h2>Albums</h2>
        </div>
        <button className="circular-control" onClick={() => addAlbum()}>
          +
        </button>
      </div>

      <div className="album-list">
        {albums.map((album) => (
          <button
            className={`album-card ${album.id === activeAlbum?.id ? "album-card--active" : ""}`}
            key={album.id}
            onClick={() => selectAlbum(album.id)}
          >
            <span>{album.songs.length} songs</span>
            <strong>{album.title}</strong>
            <small>{formatBytes(albumStorageBytes(album))}</small>
          </button>
        ))}
      </div>
    </section>
  );
};

const SongEditor = () => {
  const {
    activeAlbum,
    updateAlbumTitle,
    addSong,
    removeSong,
    reorderSongs,
    uploadSong,
    syncAlbumMetadata,
    uploadProgress,
    error,
  } = useAlbum();
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }));

  const storageBytes = useMemo(
    () => (activeAlbum ? albumStorageBytes(activeAlbum) : 0),
    [activeAlbum],
  );

  if (!activeAlbum) {
    return <section className="panel empty-state">Create an album to begin arranging songs.</section>;
  }

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;

    if (over && active.id !== over.id) {
      reorderSongs(activeAlbum.id, String(active.id), String(over.id));
    }
  };

  return (
    <section className="panel album-editor">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Album Manager</p>
          <input
            className="title-input"
            value={activeAlbum.title}
            onChange={(event) => updateAlbumTitle(activeAlbum.id, event.currentTarget.value)}
            aria-label="Album title"
          />
        </div>
        <div className="stat-stack">
          <strong>{formatBytes(storageBytes)}</strong>
          <span>raw audio</span>
        </div>
      </div>

      <div className="button-row">
        <button
          className="secondary-action"
          onClick={() =>
            addSong(activeAlbum.id, {
              title: `Stem Track ${activeAlbum.songs.length + 1}`,
              artist: "New Artist",
              start_sector: 1 + activeAlbum.songs.reduce((sum, song) => sum + song.length_sectors, 0),
              length_sectors: 900,
            })
          }
        >
          Add placeholder song
        </button>
        <button className="primary-action" onClick={() => void syncAlbumMetadata(activeAlbum)}>
          Write metadata
        </button>
      </div>

      {error ? <p className="inline-error">{error}</p> : null}

      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
        <SortableContext items={activeAlbum.songs.map((song) => song.id)} strategy={verticalListSortingStrategy}>
          <div className="sortable-list">
            {activeAlbum.songs.map((song, index) => (
              <SortableSong
                album={activeAlbum}
                index={index}
                key={song.id}
                onRemove={removeSong}
                onUpload={(album, selectedSong) => void uploadSong(album, selectedSong)}
                song={song}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>

      {!activeAlbum.songs.length ? (
        <div className="empty-state">Drop separated stems here once the backend exposes import output.</div>
      ) : null}

      {uploadProgress ? (
        <div className="upload-queue">
          <div>
            <strong>Upload queue</strong>
            <span>{uploadProgress.message ?? "Writing sectors to eMMC"}</span>
          </div>
          <div className="progress-channel">
            <div style={{ width: `${uploadProgress.percent}%` }} />
          </div>
        </div>
      ) : null}
    </section>
  );
};

export const AlbumManager = () => (
  <div className="module-grid album-grid">
    <AlbumSidebar />
    <SongEditor />
  </div>
);
