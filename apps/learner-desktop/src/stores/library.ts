import { create } from "zustand";

export type TorrentImportSource = { kind: "file" } | { kind: "magnet"; uri: string };

interface LibraryUiState {
  selectedCourseId: string | null;
  selectCourse: (courseId: string | null) => void;
  torrentImport: { jobId: string; source: TorrentImportSource } | null;
  trackTorrentImport: (jobId: string, source: TorrentImportSource) => void;
  dismissTorrentImport: () => void;
  networkOpen: boolean;
  showNetwork: (open: boolean) => void;
}

export const useLibraryUi = create<LibraryUiState>((set) => ({
  selectedCourseId: null,
  selectCourse: (selectedCourseId) => set({ selectedCourseId }),
  torrentImport: null,
  trackTorrentImport: (jobId, source) => set({ torrentImport: { jobId, source } }),
  dismissTorrentImport: () => set({ torrentImport: null }),
  networkOpen: false,
  showNetwork: (networkOpen) => set({ networkOpen }),
}));
