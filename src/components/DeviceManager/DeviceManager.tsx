import { useMemo } from "react";
import { useAlbumStore, albumStorageBytes } from "../../stores/albumStore";
import { formatBytes } from "../../utils/tauriCommands";
import { useDevice } from "../../hooks/useDevice";

const StorageRing = ({ usedBytes, totalBytes }: { usedBytes: number; totalBytes: number }) => {
  const percent = totalBytes > 0 ? Math.min(100, Math.round((usedBytes / totalBytes) * 100)) : 0;

  return (
    <div
      className="storage-ring"
      style={{
        background: `conic-gradient(#f8fafc ${percent * 3.6}deg, rgba(255,255,255,0.08) 0deg)`,
      }}
      aria-label={`${percent}% storage used`}
    >
      <div className="storage-ring__inner">
        <span>{percent}%</span>
        <small>eMMC used</small>
      </div>
    </div>
  );
};

const DeviceStatus = () => {
  const {
    connected,
    connecting,
    error,
    info,
    lastSeenAt,
    storage,
    connect,
    disconnect,
    refreshAlbumMetadata,
  } = useDevice();

  return (
    <section className="panel device-status-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">USB status</p>
          <h2>Stem Player</h2>
        </div>
        <span className={`status-pill ${connected ? "status-pill--online" : ""}`}>
          <span />
          {connected ? "Connected" : "Offline"}
        </span>
      </div>

      <div className="device-hero">
        <StorageRing usedBytes={storage.usedBytes} totalBytes={storage.totalBytes} />
        <div className="device-meta">
          <div>
            <span>Serial</span>
            <strong>{info?.serial ?? "Waiting for device"}</strong>
          </div>
          <div>
            <span>Firmware</span>
            <strong>{info?.firmware_version ?? "Unknown"}</strong>
          </div>
          <div>
            <span>Last seen</span>
            <strong>{lastSeenAt ? new Date(lastSeenAt).toLocaleString() : "Never"}</strong>
          </div>
          <div>
            <span>Storage</span>
            <strong>
              {formatBytes(storage.usedBytes)} / {formatBytes(storage.totalBytes)}
            </strong>
          </div>
        </div>
      </div>

      {error ? <p className="inline-error">{error}</p> : null}

      <div className="button-row">
        <button className="primary-action" onClick={connect} disabled={connecting || connected}>
          {connecting ? "Scanning…" : "Connect"}
        </button>
        <button className="secondary-action" onClick={disconnect} disabled={!connected}>
          Disconnect
        </button>
        <button className="secondary-action" onClick={() => void refreshAlbumMetadata()} disabled={!connected}>
          Read eMMC album
        </button>
      </div>
    </section>
  );
};

const SongList = () => {
  const albums = useAlbumStore((state) => state.albums);
  const activeAlbumId = useAlbumStore((state) => state.activeAlbumId);
  const album = albums.find((item) => item.id === activeAlbumId) ?? albums[0];

  const stats = useMemo(() => {
    if (!album) {
      return { songCount: 0, bytes: 0 };
    }

    return {
      songCount: album.songs.length,
      bytes: albumStorageBytes(album),
    };
  }, [album]);

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">eMMC contents</p>
          <h2>{album?.title ?? "No album loaded"}</h2>
        </div>
        <div className="stat-stack">
          <strong>{stats.songCount}</strong>
          <span>songs</span>
        </div>
      </div>

      <div className="song-table">
        {album?.songs.length ? (
          album.songs.map((song, index) => (
            <article className="song-row" key={song.id}>
              <div className="track-index">{String(index + 1).padStart(2, "0")}</div>
              <div>
                <strong>{song.title}</strong>
                <span>{song.artist}</span>
              </div>
              <div className="song-row__meta">
                <span>{song.length_sectors.toLocaleString()} sectors</span>
                <span>{formatBytes(song.length_sectors * 8192)}</span>
              </div>
            </article>
          ))
        ) : (
          <div className="empty-state">Connect a Stem Player or create an album to see songs.</div>
        )}
      </div>

      <p className="panel-note">Loaded audio occupies approximately {formatBytes(stats.bytes)} of raw sector data.</p>
    </section>
  );
};

export const DeviceManager = () => (
  <div className="module-grid">
    <DeviceStatus />
    <SongList />
  </div>
);
