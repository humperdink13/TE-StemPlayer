import { useEffect, useRef } from "react";
import WaveSurfer from "wavesurfer.js";
import { useStemCreator } from "../../hooks/useStemCreator";

const SourceInput = () => {
  const {
    youtubeUrl,
    selectedFilePath,
    setYoutubeUrl,
    setSelectedFilePath,
    startYoutubeSeparation,
    startFileSeparation,
  } = useStemCreator();

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Stem Creator</p>
          <h2>YouTube or local audio</h2>
        </div>
        <span className="round-badge">4 stems</span>
      </div>

      <label className="field">
        <span>YouTube URL</span>
        <input
          value={youtubeUrl}
          onChange={(event) => setYoutubeUrl(event.currentTarget.value)}
          placeholder="https://youtube.com/watch?v=…"
        />
      </label>
      <button className="primary-action wide" onClick={() => void startYoutubeSeparation()}>
        Separate YouTube audio
      </button>

      <div className="divider">or</div>

      <label className="field">
        <span>Audio file path</span>
        <input
          value={selectedFilePath}
          onChange={(event) => setSelectedFilePath(event.currentTarget.value)}
          placeholder="/Users/you/Music/source.wav"
        />
      </label>
      <input
        className="file-input"
        type="file"
        accept="audio/*"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          if (file) {
            setSelectedFilePath(file.name);
          }
        }}
      />
      <button className="secondary-action wide" onClick={() => void startFileSeparation()}>
        Separate local file
      </button>
    </section>
  );
};

const SeparationProgress = () => {
  const { activeJob, jobs, error } = useStemCreator();

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Progress</p>
          <h2>{activeJob?.title ?? "No active job"}</h2>
        </div>
        <span className="round-badge">{activeJob?.progress ?? 0}%</span>
      </div>

      <div className="progress-channel">
        <div style={{ width: `${activeJob?.progress ?? 0}%` }} />
      </div>
      <p className="panel-note">{activeJob?.message ?? "Start a separation job to monitor Demucs progress."}</p>

      {error ? <p className="inline-error">{error}</p> : null}

      <div className="stem-lanes">
        {["vocals", "drums", "bass", "other"].map((stem, index) => (
          <div className="stem-lane" key={stem}>
            <span style={{ background: ["#f9a8d4", "#fde68a", "#a7f3d0", "#93c5fd"][index] }} />
            <strong>{stem}</strong>
            <small>{activeJob?.stems[index] ?? "pending"}</small>
          </div>
        ))}
      </div>

      <div className="job-list">
        {jobs.slice(0, 4).map((job) => (
          <article key={job.id}>
            <span>{job.stage}</span>
            <strong>{job.title}</strong>
          </article>
        ))}
      </div>
    </section>
  );
};

const StemPreview = () => {
  const { activeJob } = useStemCreator();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const waveSurferRef = useRef<WaveSurfer | null>(null);

  useEffect(() => {
    if (!containerRef.current) {
      return;
    }

    waveSurferRef.current?.destroy();
    waveSurferRef.current = WaveSurfer.create({
      container: containerRef.current,
      height: 120,
      waveColor: "rgba(248,250,252,0.32)",
      progressColor: "#f8fafc",
      cursorColor: "#facc15",
      barWidth: 3,
      barGap: 2,
      barRadius: 999,
    });

    if (activeJob?.previewUrl) {
      void waveSurferRef.current.load(activeJob.previewUrl);
    }

    return () => {
      waveSurferRef.current?.destroy();
      waveSurferRef.current = null;
    };
  }, [activeJob?.previewUrl]);

  return (
    <section className="panel waveform-panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Waveform preview</p>
          <h2>Separated audio</h2>
        </div>
        <button
          className="circular-control"
          onClick={() => void waveSurferRef.current?.playPause()}
          disabled={!activeJob?.previewUrl}
          aria-label="Play or pause waveform preview"
        >
          ▶
        </button>
      </div>
      <div className="waveform-shell" ref={containerRef} />
      {!activeJob?.previewUrl ? (
        <p className="panel-note">Preview will render here when the backend emits a stem preview URL.</p>
      ) : null}
    </section>
  );
};

export const StemCreator = () => (
  <div className="module-grid">
    <SourceInput />
    <SeparationProgress />
    <StemPreview />
  </div>
);
