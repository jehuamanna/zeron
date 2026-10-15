import { create } from "zustand";

interface BuilderUiState {
  selectedDraftId: string | null;
  selectDraft: (draftId: string | null) => void;
  publishJobIds: Record<string, string>;
  rememberPublishJob: (draftId: string, jobId: string) => void;
}

export const useBuilderUi = create<BuilderUiState>((set) => ({
  selectedDraftId: null,
  selectDraft: (selectedDraftId) => set({ selectedDraftId }),
  publishJobIds: {},
  rememberPublishJob: (draftId, jobId) => set((state) => ({ publishJobIds: { ...state.publishJobIds, [draftId]: jobId } })),
}));
