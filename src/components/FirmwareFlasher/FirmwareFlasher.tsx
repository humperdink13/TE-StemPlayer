import { useFirmware } from "../../hooks/useFirmware";
import type { FlashProgressEvent } from "../../utils/tauriCommands";

const FlashProgress = ({
  progress,
  flashing,
}: {
  progress: FlashProgressEvent | null;
  flashing: boolean;
}) => {
  const percent = progress?.percent ?? 0;

  return (
    <div className="flash-progress">
      <div
        className="flash-progress__ring"
        style={{
          background: `conic-gradient(#facc15 ${percent * 3.6}deg, rgba(255,255,255,0.08) 0deg)`,
        }}
      >
        <div>
          <strong>{percent}%</strong>
          <span>{progress?.stage ?? (flashing ? "working" : "idle")}</span>
        </div>
      </div>
      <p>{progress?.message ?? "Select a firmware binary to prepare DFU flashing."}</p>
    </div>
  );
};

export const FirmwareFlasher = () => {
  const { firmwarePath, setFirmwarePath, progress, flashing, error, flashFirmware } = useFirmware();

  return (
    <div className="module-grid">
      <section className="panel firmware-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Firmware Flasher</p>
            <h2>Update Stem Player firmware</h2>
          </div>
          <span className="round-badge round-badge--warning">DFU</span>
        </div>

        <div className="warning-card">
          <strong>Do not disconnect during flashing.</strong>
          <span>
            Firmware images are validated by the Rust backend before transfer. The current backend
            reports DFU as not yet implemented, so this screen is ready for progress events once the
            flasher command is completed.
          </span>
        </div>

        <label className="field">
          <span>Firmware binary path</span>
          <input
            value={firmwarePath}
            onChange={(event) => setFirmwarePath(event.currentTarget.value)}
            placeholder="/Users/you/Downloads/stem-player-fw.bin"
          />
        </label>

        <input
          className="file-input"
          type="file"
          accept=".bin,.hex,.uf2,application/octet-stream"
          onChange={(event) => {
            const file = event.currentTarget.files?.[0];
            if (file) {
              setFirmwarePath(file.name);
            }
          }}
        />

        {error ? <p className="inline-error">{error}</p> : null}

        <button className="primary-action wide" onClick={() => void flashFirmware()} disabled={flashing}>
          {flashing ? "Flashing…" : "Flash firmware"}
        </button>
      </section>

      <section className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Flash progress</p>
            <h2>Bootloader transfer</h2>
          </div>
        </div>
        <FlashProgress flashing={flashing} progress={progress} />
      </section>
    </div>
  );
};
