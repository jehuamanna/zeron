import { invoke } from "@tauri-apps/api/core";

export interface DraftSummary {
  draftId: string;
  courseId: string;
  courseVersion: string;
  title: string;
  revision: number;
  updatedAtUnixSeconds: number;
}

export interface DraftDetail extends DraftSummary {
  description: string | null;
  publisherName: string;
  language: string;
  modules: Array<{ id: string; title: string; lessons: Array<{ id: string; title: string; resourceCount: number }> }>;
}

export const publisherApi = {
  listDrafts: () => invoke<DraftSummary[]>("list_drafts"),
  createDraft: () => invoke<DraftDetail>("create_draft"),
  getDraft: (draftId: string) => invoke<DraftDetail | null>("get_draft", { draftId }),
};
