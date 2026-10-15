import { invoke } from "@tauri-apps/api/core";

export interface DraftSummary {
  draftId: string;
  courseId: string;
  courseVersion: string;
  title: string;
  revision: number;
  updatedAtUnixSeconds: number;
}

export interface DraftMetadata {
  courseVersion: string;
  title: string;
  description: string | null;
  publisherName: string;
  language: string;
  category: string | null;
  tags: string[];
  prerequisites: string[];
  licenseName: string | null;
}

export interface DraftDetail extends DraftSummary, DraftMetadata {
  modules: Array<{ id: string; title: string; lessons: Array<{ id: string; title: string; resourceCount: number }> }>;
}

export interface ValidationIssue {
  code: string;
  field: string;
  message: string;
}

export interface CommandError {
  code: string;
  message: string;
  field?: string;
  currentRevision?: number;
}

export function asCommandError(error: unknown): CommandError | null {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    const { code, message } = error as Record<string, unknown>;
    if (typeof code === "string" && typeof message === "string") return error as CommandError;
  }
  return null;
}

export const publisherApi = {
  listDrafts: () => invoke<DraftSummary[]>("list_drafts"),
  createDraft: () => invoke<DraftDetail>("create_draft"),
  getDraft: (draftId: string) => invoke<DraftDetail | null>("get_draft", { draftId }),
  updateDraftMetadata: (draftId: string, expectedRevision: number, metadata: DraftMetadata) =>
    invoke<DraftDetail>("update_draft_metadata", { draftId, expectedRevision, metadata }),
  validateDraft: (draftId: string) => invoke<ValidationIssue[]>("validate_draft", { draftId }),
};
