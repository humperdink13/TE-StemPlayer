import { create } from "zustand";
import type { SeparationProgressEvent, StemSource } from "../utils/tauriCommands";

export type StemJob = {
  id: string;
  source: StemSource;
  title: string;
  createdAt: string;
  progress: number;
  stage: SeparationProgressEvent["stage"];
  message: string;
  stems: string[];
  previewUrl?: string;
};

type StemState = {
  youtubeUrl: string;
  selectedFilePath: string;
  activeJobId: string | null;
  jobs: StemJob[];
  error: string | null;
  setYoutubeUrl: (url: string) => void;
  setSelectedFilePath: (path: string) => void;
  createJob: (source: StemSource, id?: string) => string;
  updateJobProgress: (event: SeparationProgressEvent) => void;
  setError: (error: string | null) => void;
  getActiveJob: () => StemJob | undefined;
};

const createId = () => crypto.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;

const titleFromSource = (source: StemSource) => {
  if (source.type === "youtube") {
    return source.value.replace(/^https?:\/\//, "").slice(0, 48) || "YouTube stem job";
  }

  return source.value.split(/[\\/]/).pop() || "Local audio stem job";
};

export const useStemStore = create<StemState>((set, get) => ({
  youtubeUrl: "",
  selectedFilePath: "",
  activeJobId: null,
  jobs: [],
  error: null,

  setYoutubeUrl: (youtubeUrl) => set({ youtubeUrl }),

  setSelectedFilePath: (selectedFilePath) => set({ selectedFilePath }),

  createJob: (source, id = createId()) => {
    const now = new Date().toISOString();

    set((state) => ({
      jobs: [
        {
          id,
          source,
          title: titleFromSource(source),
          createdAt: now,
          progress: 0,
          stage: "queued",
          message: "Waiting for the separation worker",
          stems: [],
        },
        ...state.jobs,
      ],
      activeJobId: id,
      error: null,
    }));

    return id;
  },

  updateJobProgress: (event) =>
    set((state) => ({
      jobs: state.jobs.map((job) =>
        job.id === event.jobId
          ? {
              ...job,
              progress: Math.max(0, Math.min(100, event.percent)),
              stage: event.stage,
              message: event.message ?? job.message,
              stems: event.stems ?? job.stems,
              previewUrl: event.previewUrl ?? job.previewUrl,
            }
          : job,
      ),
      activeJobId: event.jobId,
      error: event.stage === "error" ? event.message ?? "Stem separation failed" : state.error,
    })),

  setError: (error) => set({ error }),

  getActiveJob: () => {
    const state = get();
    return state.jobs.find((job) => job.id === state.activeJobId) ?? state.jobs[0];
  },
}));
