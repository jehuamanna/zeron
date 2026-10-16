import { create } from "zustand";

export type TorrentImportSource = { kind: "file" } | { kind: "magnet"; uri: string };

interface LibraryUiState {
  selectedCourseId: string | null;
  selectCourse: (courseId: string | null) => void;
  /** The library notebook is showing instead of the course list; leaving a course returns to it. */
  notebookOpen: boolean;
  showNotebook: (open: boolean) => void;
  torrentImport: { jobId: string; source: TorrentImportSource } | null;
  trackTorrentImport: (jobId: string, source: TorrentImportSource) => void;
  dismissTorrentImport: () => void;
}

export const useLibraryUi = create<LibraryUiState>((set) => ({
  selectedCourseId: null,
  selectCourse: (selectedCourseId) => set({ selectedCourseId }),
  notebookOpen: false,
  showNotebook: (notebookOpen) => set({ notebookOpen }),
  torrentImport: null,
  trackTorrentImport: (jobId, source) => set({ torrentImport: { jobId, source } }),
  dismissTorrentImport: () => set({ torrentImport: null }),
}));
