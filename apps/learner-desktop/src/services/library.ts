import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type CourseSource = "local" | "torrent";

export type NetworkProfile = "loopback" | "local_network" | "internet";

export interface NetworkSettings {
  configuredProfile: NetworkProfile;
  activeProfile: NetworkProfile;
  torrentEngine: "ready" | "unavailable";
  downloadPort: number | null;
  sharingPort: number | null;
  /** Stored fixed port for shared courses; null means a new port on each start. */
  peerPort: number | null;
  /** Stored UPnP choice, honored under the Internet profile only. */
  portForwarding: boolean;
  activePortForwarding: boolean;
  restartRequired: boolean;
  /** Download cap in KiB/s; null means unlimited. */
  downloadLimitKib: number | null;
  /** Upload cap for shared courses in KiB/s; null means unlimited. */
  uploadLimitKib: number | null;
}

export interface CourseSummary {
  courseId: string;
  courseVersion: string;
  title: string;
  publisherName: string;
  language: string;
  totalResourceBytes: number;
  importedAtUnixSeconds: number;
  source: CourseSource;
  torrentInfoHash: string | null;
  sharing: boolean;
  completedLessons: number;
  totalLessons: number;
}

export type NoteAnchor =
  | { kind: "lesson" }
  | { kind: "videoTime"; resourceId: string; seconds: number }
  | { kind: "pdfPage"; resourceId: string; page: number; region: { x: number; y: number; width: number; height: number } | null };

export interface Note {
  noteId: string;
  lessonId: string;
  anchor: NoteAnchor;
  /** Tiptap document as JSON text. */
  body: string;
  text: string;
  revision: number;
  createdAtUnixSeconds: number;
  updatedAtUnixSeconds: number;
}

export type NoteChange = { outcome: "saved"; note: Note } | { outcome: "conflict"; currentRevision: number } | { outcome: "notFound" };

export type LessonStatus = "not_started" | "in_progress" | "completed";

export interface CourseProgress {
  completedLessons: number;
  totalLessons: number;
  percent: number;
  lessons: Array<{ lessonId: string; status: LessonStatus }>;
}

export interface ImportReceipt {
  courseId: string;
  courseVersion: string;
  title: string;
  verifiedResourceCount: number;
  totalResourceBytes: number;
}

export interface Readiness {
  state: "ready";
  torrentImport: "ready" | "unavailable";
  sharing: "ready" | "unavailable";
}

export interface ResourceAvailability {
  resourceId: string;
  verifiedBytes: number;
  byteLength: number;
  /** Whether the learner keeps this lesson offline. */
  kept: boolean;
  state: AvailabilityState;
}

export type AvailabilityState = "online_only" | "downloading" | "partially_available" | "available_offline";

export interface CourseDownloads {
  /** Whether the learner paused this course's transfer. */
  paused: boolean;
  course: boolean;
  modules: string[];
  lessons: string[];
}

export interface RemovalReport {
  removed: string[];
  freedBytes: number;
  /** Requested lessons left alone because they are kept offline. */
  kept: string[];
}

export interface StorageUse {
  /** Bytes of streamed lessons that are not kept offline. */
  streamedBytes: number;
  cacheLimitBytes: number | null;
  /** Free bytes on the downloads disk, when known. */
  availableBytes: number | null;
  /** Downloads are paused because the disk is nearly full. */
  lowDisk: boolean;
}

export type DownloadScope = { kind: "course" } | { kind: "module"; id: string } | { kind: "lesson"; id: string };

export interface CourseSharing {
  courseId: string;
  available: boolean;
  sharing: boolean;
  networkProfile: NetworkProfile;
  state: "inactive" | "checking" | "downloading" | "ready" | "failed" | "paused";
  connectedPeers: number;
  uploadedBytes: number;
  uploadBytesPerSecond: number;
}

export type TorrentImportStage =
  | "queued"
  | "resolving_metadata"
  | "fetching_manifest"
  | "validating"
  | "imported"
  | "failed"
  | "cancelled";

export interface TorrentImport {
  jobId: string;
  sequence: number;
  stage: TorrentImportStage;
  infoHash: string | null;
  manifestVerifiedBytes: number | null;
  manifestTotalBytes: number | null;
  courseId: string | null;
  title: string | null;
  failure: CommandError | null;
}

export interface CommandError {
  code: string;
  message: string;
}

export const finishedTorrentImportStages = new Set<TorrentImportStage>(["imported", "failed", "cancelled"]);

export function asCommandError(error: unknown): CommandError | null {
  if (typeof error === "object" && error !== null && "code" in error && "message" in error) {
    const { code, message } = error as Record<string, unknown>;
    if (typeof code === "string" && typeof message === "string") return error as CommandError;
  }
  return null;
}

export interface MediaRegistration {
  url: string;
  mediaType: string;
  byteLength: number;
}

export interface CourseDetail {
  courseId: string;
  courseVersion: string;
  title: string;
  description: string | null;
  publisherName: string;
  source: CourseSource;
  torrentInfoHash: string | null;
  modules: Array<{
    id: string;
    title: string;
    lessons: Array<{
      id: string;
      title: string;
      resources: Array<{
        id: string;
        kind: string;
        mediaType: string;
        byteLength: number;
        durationSeconds: number | null;
      }>;
    }>;
  }>;
}

export const libraryApi = {
  readiness: () => invoke<Readiness>("readiness"),
  list: () => invoke<CourseSummary[]>("list_courses"),
  detail: (courseId: string) => invoke<CourseDetail | null>("get_course_detail", { courseId }),
  registerMedia: (courseId: string, resourceId: string) =>
    invoke<MediaRegistration>("register_media", { courseId, resourceId }),
  chooseAndImport: () => invoke<ImportReceipt | null>("choose_and_import_course"),
  chooseTorrentFile: () => invoke<TorrentImport | null>("choose_torrent_file_and_import"),
  importMagnet: (uri: string) => invoke<TorrentImport>("import_magnet", { uri }),
  getTorrentImport: (jobId: string) => invoke<TorrentImport | null>("get_torrent_import", { jobId }),
  cancelTorrentImport: (jobId: string) => invoke<boolean>("cancel_torrent_import", { jobId }),
  getNetworkSettings: () => invoke<NetworkSettings>("get_network_settings"),
  setNetworkProfile: (profile: NetworkProfile) => invoke<void>("set_network_profile", { profile }),
  setPeerReachability: (peerPort: number | null, portForwarding: boolean) =>
    invoke<void>("set_peer_reachability", { peerPort, portForwarding }),
  /** Caps in KiB/s, null for unlimited; they apply immediately. */
  setRateLimits: (downloadKib: number | null, uploadKib: number | null) =>
    invoke<void>("set_rate_limits", { downloadKib, uploadKib }),
  /** Where the learner stopped in a lesson, in seconds, if recorded. */
  playbackPosition: (courseId: string, resourceId: string) =>
    invoke<number | null>("get_playback_position", { courseId, resourceId }),
  /** A position in the last seconds of a lesson is stored as its start. */
  savePlaybackPosition: (courseId: string, resourceId: string, positionSeconds: number, durationSeconds: number) =>
    invoke<boolean>("save_playback_position", { courseId, resourceId, positionSeconds, durationSeconds }),
  downloads: (courseId: string) => invoke<CourseDownloads>("get_course_downloads", { courseId }),
  /** Keeping a scope offline downloads it in full; stopping keeps what is already verified. */
  setDownload: (courseId: string, scope: DownloadScope, keep: boolean) =>
    invoke<CourseDownloads>("set_course_download", { courseId, scope, keep }),
  /** Pausing stops downloading and sharing; verified lessons still play. */
  setPaused: (courseId: string, paused: boolean) => invoke<CourseDownloads>("set_course_paused", { courseId, paused }),
  /** Removes downloaded lessons that are not kept offline; all of the course when no IDs are given. */
  removeDownloads: (courseId: string, resourceIds?: string[]) =>
    invoke<RemovalReport>("remove_downloads", { courseId, resourceIds: resourceIds ?? null }),
  storage: () => invoke<StorageUse>("get_storage"),
  /** Caps streamed lessons; null for no cap. Applies at once. */
  setCacheLimit: (limitBytes: number | null) => invoke<RemovalReport>("set_cache_limit", { limitBytes }),
  notes: (courseId: string, lessonId: string | null) => invoke<Note[]>("list_notes", { courseId, lessonId }),
  createNote: (courseId: string, lessonId: string, anchor: NoteAnchor, body: string) =>
    invoke<Note>("create_note", { courseId, lessonId, anchor, body }),
  /** Saves only if the note is still at `expectedRevision`; never overwrites a newer note. */
  updateNote: (noteId: string, expectedRevision: number, anchor: NoteAnchor, body: string) =>
    invoke<NoteChange>("update_note", { noteId, expectedRevision, anchor, body }),
  deleteNote: (noteId: string, expectedRevision: number) => invoke<NoteChange>("delete_note", { noteId, expectedRevision }),
  progress: (courseId: string) => invoke<CourseProgress>("get_course_progress", { courseId }),
  setLessonCompleted: (courseId: string, lessonId: string, completed: boolean) =>
    invoke<CourseProgress>("set_lesson_completed", { courseId, lessonId, completed }),
  availability: (courseId: string) => invoke<ResourceAvailability[]>("get_course_availability", { courseId }),
  getCourseSharing: (courseId: string) => invoke<CourseSharing | null>("get_course_sharing", { courseId }),
  setCourseSharing: (courseId: string, sharing: boolean) =>
    invoke<CourseSharing>("set_course_sharing", { courseId, sharing }),
  onTorrentImport: (handler: (job: TorrentImport) => void) =>
    listen<TorrentImport>("torrent-import", (event) => handler(event.payload)),
};
