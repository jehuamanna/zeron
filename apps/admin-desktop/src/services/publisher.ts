import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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

export type PublishStage =
  | "DRAFT"
  | "VALIDATING"
  | "PACKAGING"
  | "GENERATING_TORRENT"
  | "VERIFYING"
  | "READY_TO_PUBLISH"
  | "PUBLISHED"
  | "SEEDING"
  | "FAILED"
  | "CANCELLED"
  | "SUPERSEDED"
  | "ARCHIVED";

export interface PreparedRelease {
  attemptId: string;
  courseVersion: string;
  resourceCount: number;
  manifestSha256: string;
  infoHash: string;
  magnet: string;
  trackerCount: number;
  pieceLength: number;
  pieceCount: number;
  fileCount: number;
  totalBytes: number;
}

export interface PublishJob {
  jobId: string;
  draftId: string;
  draftRevision: number;
  sequence: number;
  stage: PublishStage;
  progress: { completedBytes: number; totalBytes: number } | null;
  prepared: PreparedRelease | null;
  failure: { code: string; message: string } | null;
  releaseId: string | null;
}

export interface Release {
  releaseId: string;
  courseId: string;
  courseVersion: string;
  title: string;
  infoHash: string;
  magnet: string;
  pieceLength: number;
  pieceCount: number;
  fileCount: number;
  totalBytes: number;
  publishedAtUnixSeconds: number;
  status: "published" | "superseded" | "archived";
  /** Whether the release files are on disk; only archived releases can lose them. */
  files: "present" | "deleting" | "deleted";
}

export interface ReleaseFile {
  /** Package-relative path as declared in the release torrent. */
  path: string;
  byteLength: number;
}

export interface ReleaseDetails {
  release: Release;
  files: ReleaseFile[];
}

export type SkipReason =
  | "hidden"
  | "symbolic_link"
  | "nested_folder"
  | "unsupported_type"
  | "unreadable"
  | "no_lessons"
  | "limit_reached"
  | "no_target_lesson";

export interface SkippedEntry {
  /** Relative to what was dropped, for example `Course/notes.txt`; never a filesystem path. */
  name: string;
  reason: SkipReason;
}

export interface DropFile {
  title: string;
  fileName: string;
  kind: ResourceKind;
}

/** A scanned drop. Paths stay in the backend; `token` refers to the scan. */
export interface DropSummary {
  token: string;
  /** Drop point in CSS pixels of the window. */
  x: number;
  y: number;
  modules: Array<{ title: string; lessons: DropFile[] }>;
  looseFiles: DropFile[];
  skipped: SkippedEntry[];
}

export interface DropOutcome {
  draft: DraftDetail;
  modulesAdded: number;
  lessonsAdded: number;
  filesAttached: number;
  skipped: SkippedEntry[];
}

export const finishedPublishStages: ReadonlySet<PublishStage> = new Set(["READY_TO_PUBLISH", "PUBLISHED", "FAILED", "CANCELLED"]);

export type NetworkProfile = "loopback" | "local_network" | "internet";

export interface SeedingStatus {
  releaseId: string;
  courseVersion: string;
  title: string;
  releaseStatus: Release["status"];
  enabled: boolean;
  state: "checking" | "seeding" | "failed" | null;
  verifiedBytes: number;
  totalBytes: number;
  uploadedBytes: number;
  uploadBytesPerSecond: number;
  connectedPeers: number;
  failure: string | null;
}

export interface Distribution {
  configuredProfile: NetworkProfile;
  activeProfile: NetworkProfile;
  listenPort: number | null;
  /** Stored fixed peer port; null means a new port on each start. */
  peerPort: number | null;
  /** Stored UPnP choice, honored under the Internet profile only. */
  portForwarding: boolean;
  activePortForwarding: boolean;
  restartRequired: boolean;
  /** Trackers written into new releases. */
  trackers: string[];
  engineError: string | null;
  releases: SeedingStatus[];
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
  /** Deletes the draft only; its releases and source files are kept. */
  deleteDraft: (draftId: string, expectedRevision: number) => invoke<void>("delete_draft", { draftId, expectedRevision }),
  updateDraftMetadata: (draftId: string, expectedRevision: number, metadata: DraftMetadata) =>
    invoke<DraftDetail>("update_draft_metadata", { draftId, expectedRevision, metadata }),
  editDraftStructure: (draftId: string, expectedRevision: number, edit: StructureEdit) =>
    invoke<DraftDetail>("edit_draft_structure", { draftId, expectedRevision, edit }),
  chooseAndAttachResource: (draftId: string, expectedRevision: number, lessonId: string) =>
    invoke<DraftDetail | null>("choose_and_attach_resource", { draftId, expectedRevision, lessonId }),
  inspectMedia: (draftId: string) => invoke<MediaFinding[]>("inspect_media", { draftId }),
  startPublishPreparation: (draftId: string) => invoke<PublishJob>("start_publish_preparation", { draftId }),
  getPublishJob: (jobId: string) => invoke<PublishJob | null>("get_publish_job", { jobId }),
  cancelPublishJob: (jobId: string) => invoke<boolean>("cancel_publish_job", { jobId }),
  onPublishJob: (handler: (job: PublishJob) => void) => listen<PublishJob>("publish-job", (event) => handler(event.payload)),
  activateRelease: (jobId: string) => invoke<Release>("activate_release", { jobId }),
  listReleases: () => invoke<Release[]>("list_releases"),
  /** Stops seeding and marks the release archived; its files stay on disk. */
  archiveRelease: (releaseId: string) => invoke<Release>("archive_release", { releaseId }),
  /** Permanently removes an archived release's files; `confirmation` must be its course version. */
  deleteReleaseFiles: (releaseId: string, confirmation: string) => invoke<Release>("delete_release_files", { releaseId, confirmation }),
  getReleaseDetails: (releaseId: string) => invoke<ReleaseDetails>("get_release_details", { releaseId }),
  /** Opens a native save dialog; resolves `false` when the publisher cancels. */
  exportReleaseTorrent: (releaseId: string) => invoke<boolean>("export_release_torrent", { releaseId }),
  getDistribution: () => invoke<Distribution>("get_distribution"),
  startSeeding: (releaseId: string) => invoke<void>("start_seeding", { releaseId }),
  stopSeeding: (releaseId: string) => invoke<void>("stop_seeding", { releaseId }),
  setNetworkProfile: (profile: NetworkProfile) => invoke<void>("set_network_profile", { profile }),
  setTrackers: (trackers: string[]) => invoke<string[]>("set_trackers", { trackers }),
  setPeerReachability: (peerPort: number | null, portForwarding: boolean) =>
    invoke<void>("set_peer_reachability", { peerPort, portForwarding }),
  onDrag: (handler: (active: boolean) => void) => listen<{ active: boolean }>("builder-drag", (event) => handler(event.payload.active)),
  onDrop: (handler: (summary: DropSummary) => void) => listen<DropSummary>("builder-drop", (event) => handler(event.payload)),
  /** Folders become new modules; loose files attach to `lessonId` when given. */
  applyDrop: (draftId: string, expectedRevision: number, token: string, lessonId: string | null) =>
    invoke<DropOutcome>("apply_drop", { draftId, expectedRevision, token, target: { lessonId } }),
  discardDrop: (token: string) => invoke<void>("discard_drop", { token }),
  validateDraft: (draftId: string) => invoke<ValidationIssue[]>("validate_draft", { draftId }),
};
