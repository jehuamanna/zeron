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
  /** Whether the package carries the publisher's signature. */
  signed: boolean;
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

export interface ReleaseSignature {
  /** The publisher's identity, the root key learners pin. */
  publisherId: string;
  fingerprint: string;
  signingKey: string;
  signedAtUnixSeconds: number;
}

export interface ReleaseDetails {
  release: Release;
  files: ReleaseFile[];
  /** `null` for an unsigned release. */
  signature: ReleaseSignature | null;
}

export interface PublisherIdentity {
  status: "none" | "ready" | "key_missing" | "keychain_unavailable";
  publisherId: string | null;
  signingKey: string | null;
  fingerprint: string | null;
  createdAtUnixSeconds: number | null;
  rotations: number;
  signReleases: boolean;
  /** Why the keychain or key cannot be used, when it cannot. */
  problem: string | null;
}

export type SkipReason =
  | "hidden"
  | "symbolic_link"
  | "nested_folder"
  | "unsupported_type"
  | "unreadable"
  | "no_lessons"
  | "limit_reached";

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
  /** The module loose files become when no lesson is chosen. */
  looseModuleTitle: string;
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

export interface TrackerList {
  url: string;
  trackerCount: number;
  fetchedAtUnixSeconds: number | null;
}

export interface TrackerListRefresh {
  url: string;
  trackerCount: number | null;
  error: string | null;
}

/** Release seeding and trackers. The network profile, port, and speed limits are the app's Network settings. */
export interface Distribution {
  /** Trackers added by hand, written into new releases first. */
  trackers: string[];
  /** Followed tracker lists with their cached downloads. */
  trackerLists: TrackerList[];
  /** How many trackers a release published now would carry. */
  effectiveTrackerCount: number;
  /** Why releases cannot be seeded in this session, if they cannot. */
  engineError: string | null;
  releases: SeedingStatus[];
}

export type ActivityKind =
  | "release_published"
  | "release_made_current"
  | "release_archived"
  | "release_files_deleted"
  | "seeding_started"
  | "seeding_stopped"
  | "seeding_failed"
  | "tracker_lists_refreshed"
  | "tracker_list_failed";

/** One distribution event. `detail` had paths, addresses, and link secrets removed when it was recorded. */
export interface ActivityEvent {
  eventId: number;
  atUnixSeconds: number;
  releaseId: string | null;
  courseVersion: string | null;
  kind: ActivityKind;
  detail: string;
}

export interface NetworkConfiguration {
  profile: "loopback" | "local_network" | "internet";
  peerPort: number | null;
  portForwarding: boolean;
  downloadLimitKib: number | null;
  uploadLimitKib: number | null;
}

/** A validated configuration file awaiting review; `token` refers to it. */
export interface ConfigurationPreview {
  token: string;
  trackers: string[];
  trackerLists: string[];
  network: NetworkConfiguration | null;
}

export interface AppliedConfiguration {
  trackers: number;
  trackerLists: number;
  networkApplied: boolean;
  /** The network profile or port changed; they take effect when Zeron restarts. */
  restartRequired: boolean;
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
  /** Rolls back a READY_TO_PUBLISH job: its staged files are removed and it becomes CANCELLED. */
  discardPublishJob: (jobId: string) => invoke<PublishJob>("discard_publish_job", { jobId }),
  onPublishJob: (handler: (job: PublishJob) => void) => listen<PublishJob>("publish-job", (event) => handler(event.payload)),
  activateRelease: (jobId: string) => invoke<Release>("activate_release", { jobId }),
  listReleases: () => invoke<Release[]>("list_releases"),
  /** Stops seeding and marks the release archived; its files stay on disk. */
  archiveRelease: (releaseId: string) => invoke<Release>("archive_release", { releaseId }),
  /** Permanently removes an archived release's files; `confirmation` must be its course version. */
  deleteReleaseFiles: (releaseId: string, confirmation: string) => invoke<Release>("delete_release_files", { releaseId, confirmation }),
  /** Makes a superseded release the course's current release again; the current one becomes superseded. */
  makeReleaseCurrent: (releaseId: string) => invoke<Release>("make_release_current", { releaseId }),
  /** Starts a draft for the next version from a release, keeping its module, lesson, and resource IDs. */
  createDraftFromRelease: (releaseId: string) => invoke<DraftDetail>("create_draft_from_release", { releaseId }),
  getReleaseDetails: (releaseId: string) => invoke<ReleaseDetails>("get_release_details", { releaseId }),
  /** Opens a native save dialog; resolves `false` when the publisher cancels. */
  exportReleaseTorrent: (releaseId: string) => invoke<boolean>("export_release_torrent", { releaseId }),
  getDistribution: () => invoke<Distribution>("get_distribution"),
  /** Newest first; all releases when `releaseId` is null. */
  getDistributionActivity: (releaseId: string | null) => invoke<ActivityEvent[]>("get_distribution_activity", { releaseId }),
  /** Opens a native save dialog for the redacted log; resolves `false` when cancelled. */
  exportDistributionActivity: () => invoke<boolean>("export_distribution_activity"),
  /** Opens a native save dialog; resolves `false` when cancelled. */
  exportPublisherConfiguration: (includeNetwork: boolean) => invoke<boolean>("export_publisher_configuration", { includeNetwork }),
  /** Opens a native picker and validates the file; resolves `null` when cancelled. Nothing changes until applied. */
  choosePublisherConfiguration: () => invoke<ConfigurationPreview | null>("choose_publisher_configuration"),
  applyPublisherConfiguration: (token: string, applyNetwork: boolean) => invoke<AppliedConfiguration>("apply_publisher_configuration", { token, applyNetwork }),
  discardPublisherConfiguration: (token: string) => invoke<void>("discard_publisher_configuration", { token }),
  startSeeding: (releaseId: string) => invoke<void>("start_seeding", { releaseId }),
  stopSeeding: (releaseId: string) => invoke<void>("stop_seeding", { releaseId }),
  setTrackers: (trackers: string[]) => invoke<string[]>("set_trackers", { trackers }),
  setTrackerLists: (urls: string[]) => invoke<string[]>("set_tracker_lists", { urls }),
  refreshTrackerLists: () => invoke<TrackerListRefresh[]>("refresh_tracker_lists"),
  onDrag: (handler: (active: boolean) => void) => listen<{ active: boolean }>("builder-drag", (event) => handler(event.payload.active)),
  onDrop: (handler: (summary: DropSummary) => void) => listen<DropSummary>("builder-drop", (event) => handler(event.payload)),
  /**
   * Opens a native picker for folders or files and scans them like a drop.
   * Resolves `null` when the publisher cancels.
   */
  chooseLessonSources: (kind: "folders" | "files") => invoke<DropSummary | null>("choose_lesson_sources", { kind }),
  /** Folders become new modules; loose files attach to `lessonId`, or without one become a new module. */
  applyDrop: (draftId: string, expectedRevision: number, token: string, lessonId: string | null) =>
    invoke<DropOutcome>("apply_drop", { draftId, expectedRevision, token, target: { lessonId } }),
  discardDrop: (token: string) => invoke<void>("discard_drop", { token }),
  validateDraft: (draftId: string) => invoke<ValidationIssue[]>("validate_draft", { draftId }),
  getIdentity: () => invoke<PublisherIdentity>("get_publisher_identity"),
  createIdentity: () => invoke<PublisherIdentity>("create_publisher_identity"),
  rotateIdentity: () => invoke<PublisherIdentity>("rotate_publisher_identity"),
  setSignReleases: (enabled: boolean) => invoke<PublisherIdentity>("set_sign_releases", { enabled }),
  /** Opens a native save dialog; resolves `false` when cancelled. The passphrase is only used to encrypt. */
  exportIdentityBackup: (passphrase: string) => invoke<boolean>("export_identity_backup", { passphrase }),
  /** Opens a native picker; resolves `null` when cancelled. */
  restoreIdentityBackup: (passphrase: string) => invoke<PublisherIdentity | null>("restore_identity_backup", { passphrase }),
};
