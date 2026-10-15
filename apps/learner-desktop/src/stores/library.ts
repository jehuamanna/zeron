import { create } from "zustand";

interface LibraryUiState {
  selectedCourseId: string | null;
  selectCourse: (courseId: string | null) => void;
}

export const useLibraryUi = create<LibraryUiState>((set) => ({
  selectedCourseId: null,
  selectCourse: (selectedCourseId) => set({ selectedCourseId }),
}));
