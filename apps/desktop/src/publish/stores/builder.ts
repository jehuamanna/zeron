import { create } from "zustand";

export type Workspace = "courses" | "distribution";

interface BuilderUiState {
  workspace: Workspace;
  showWorkspace: (workspace: Workspace) => void;
  selectedDraftId: string | null;
  selectDraft: (draftId: string | null) => void;
  publishJobIds: Record<string, string>;
  rememberPublishJob: (draftId: string, jobId: string) => void;
}

export const useBuilderUi = create<BuilderUiState>((set) => ({
  workspace: "courses",
  showWorkspace: (workspace) => set({ workspace, selectedDraftId: null }),
  selectedDraftId: null,
  selectDraft: (selectedDraftId) => set({ selectedDraftId }),
  publishJobIds: {},
  rememberPublishJob: (draftId, jobId) => set((state) => ({ publishJobIds: { ...state.publishJobIds, [draftId]: jobId } })),
}));
