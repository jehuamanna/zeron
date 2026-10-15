import { create } from "zustand";

interface BuilderUiState {
  selectedDraftId: string | null;
  selectDraft: (draftId: string | null) => void;
}

export const useBuilderUi = create<BuilderUiState>((set) => ({
  selectedDraftId: null,
  selectDraft: (selectedDraftId) => set({ selectedDraftId }),
}));
