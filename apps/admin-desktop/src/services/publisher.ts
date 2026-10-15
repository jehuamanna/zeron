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

export type ResourceKind = "video" | "audio" | "pdf" | "markdown" | "text" | "subtitle" | "transcript" | "image" | "source_archive" | "other";

export interface ResourceSummary {
  id: string;
  kind: ResourceKind;
  fileName: string;
  mediaType: string;
  byteLength: number | null;
  durationSeconds: number | null;
}

export interface MediaFinding {
  resourceId: string;
  field: string;
  fileName: string;
  code: string;
  severity: "warning" | "advice";
  message: string;
}

export interface DraftDetail extends DraftSummary, DraftMetadata {
  modules: Array<{ id: string; title: string; lessons: Array<{ id: string; title: string; resources: ResourceSummary[] }> }>;
}

export interface ValidationIssue {
  code: string;
  field: string;
  message: string;
}

export type StructureEdit =
  | { type: "addModule"; title: string }
  | { type: "renameModule"; moduleId: string; title: string }
  | { type: "removeModule"; moduleId: string }
  | { type: "moveModule"; moduleId: string; position: number }
  | { type: "addLesson"; moduleId: string; title: string }
  | { type: "renameLesson"; lessonId: string; title: string }
  | { type: "removeLesson"; lessonId: string }
  | { type: "moveLesson"; moduleId: string; lessonId: string; position: number }
  | { type: "removeResource"; resourceId: string }
  | { type: "moveResource"; lessonId: string; resourceId: string; position: number };

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
  editDraftStructure: (draftId: string, expectedRevision: number, edit: StructureEdit) =>
    invoke<DraftDetail>("edit_draft_structure", { draftId, expectedRevision, edit }),
  chooseAndAttachResource: (draftId: string, expectedRevision: number, lessonId: string) =>
    invoke<DraftDetail | null>("choose_and_attach_resource", { draftId, expectedRevision, lessonId }),
  inspectMedia: (draftId: string) => invoke<MediaFinding[]>("inspect_media", { draftId }),
  validateDraft: (draftId: string) => invoke<ValidationIssue[]>("validate_draft", { draftId }),
};
