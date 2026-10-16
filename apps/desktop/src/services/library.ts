import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type CourseSource = "local" | "torrent" | "youtube";

/** Media type of a lesson played by YouTube's own player (ADR 0018). */
export const YOUTUBE_MEDIA_TYPE = "video/x-youtube";

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
  /** Whether Zeron asks the router to open its ports in this session. */
  activePortForwarding: boolean;
  restartRequired: boolean;
  /** Download cap in KiB/s; null means unlimited. */
  downloadLimitKib: number | null;
  /** Upload cap for shared courses in KiB/s; null means unlimited. */
  uploadLimitKib: number | null;
  /** What the router said about opening Zeron's ports; null when forwarding is off. */
  portMapping: PortMapping | null;
}

export type MappingProtocol = "pcp" | "nat_pmp" | "upnp";

export interface PortMapping {
  state: "probing" | "mapped" | "unavailable" | "stopped";
  protocol: MappingProtocol | null;
  ports: { internalPort: number; externalPort: number; transport: "tcp" | "udp" }[];
  /** Each protocol tried last, in order. */
  tried: { protocol: MappingProtocol; outcome: "mapped" | "no_answer" | "refused" | "unsupported" | "no_gateway" | "failed"; code: number | null }[];
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

export interface Region {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type HighlightColor = "yellow" | "green" | "blue" | "pink" | "orange" | "purple";
export type HighlightStyle = "highlight" | "underline" | "strike";

/** Selected text on a PDF page: what it says and its line boxes, in page fractions. */
export interface TextHighlight {
  color: HighlightColor;
  style: HighlightStyle;
  quote: string;
  rects: Region[];
}

export type NoteAnchor =
  | { kind: "lesson" }
  | { kind: "videoTime"; resourceId: string; seconds: number }
  | { kind: "pdfPage"; resourceId: string; page: number; region: Region | null; highlight?: TextHighlight };

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

export interface SearchResult {
  kind: "course" | "lesson" | "note" | "transcript" | "text";
  courseId: string;
  courseTitle: string;
  lessonId: string | null;
  lessonTitle: string | null;
  sourceId: string;
  position: number | null;
  /** Matched terms are wrapped in U+E000 and U+E001. */
  snippet: string;
}

export interface TranscriptCue {
  start: number;
  end: number;
  text: string;
}

export interface Transcript {
  resourceId: string;
  language: string | null;
  cues: TranscriptCue[];
}

export interface DrawingFile {
  id: string;
  mimeType: string;
  dataUrl: string;
}

export interface Drawing {
  sceneJson: string;
  revision: number;
  files: DrawingFile[];
}

export type DrawingSave = { outcome: "saved"; revision: number } | { outcome: "conflict"; currentRevision: number };

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

/** What a pasted YouTube link is; a playlist comes with the page that lists its videos. */
export type YouTubeLinkKind = { kind: "video" } | { kind: "playlist"; playlistId: string; pageUrl: string };

export interface PlaylistReceipt extends ImportReceipt {
  /** Videos left out because they are private, removed, or not embeddable. */
  skipped: number;
}

export interface PlaylistProgress {
  playlistId: string;
  done: number;
  total: number;
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

export interface AvailableUpdate {
  version: string;
  currentVersion: string;
  notes: string | null;
}

/** What a backup holds; `contents` lists `library` and `publish`. */
export interface Backup {
  createdAtUnixSeconds: number | null;
  contents: Array<"library" | "publish">;
  totalBytes: number;
}

export interface CatalogEntry {
  courseId: string;
  courseVersion: string;
  title: string;
  description: string;
  magnet: string;
  infoHash: string;
  status: "available" | "in_library" | "update" | "other_version" | "other_publisher";
}

/** A verified publisher catalog kept by the Library. */
export interface Catalog {
  publisherId: string;
  fingerprint: string;
  title: string;
  issuedAtUnixSeconds: number;
  /** HTTPS address it refreshes from; `null` when imported from a file. */
  sourceUrl: string | null;
  entries: CatalogEntry[];
}

/**
 * `verified`: signed by the publisher this course is pinned to. `signed`: signed,
 * but no longer pinned. `unsigned`: no publisher signature.
 */
export interface CoursePublisher {
  status: "verified" | "signed" | "pinned_unsigned" | "unsigned";
  fingerprint: string | null;
  publisherId: string | null;
  signedAtUnixSeconds: number | null;
}

export interface CourseDetail {
  courseId: string;
  courseVersion: string;
  title: string;
  description: string | null;
  publisherName: string;
  source: CourseSource;
  torrentInfoHash: string | null;
  /** Who published the course, as far as the Library can verify. */
  publisher: CoursePublisher;
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

/** Annotations on a lesson the current version no longer has, or notes pointing at files a kept lesson lost. */
export interface AnnotationMove {
  fromLessonId: string;
  /** Title in the version it came from, when known. */
  fromTitle: string | null;
  fromVersion: string | null;
  suggestedLessonId: string | null;
  suggestedTitle: string | null;
  reason: "same_lesson" | "same_content" | "same_title" | null;
  notes: number;
  /** Notes that would point at the whole lesson after moving to the suggestion. */
  notesWidened: number;
  hasDrawing: boolean;
  hasProgress: boolean;
}

export interface AnnotationMoveReport {
  notesMoved: number;
  notesWidened: number;
  drawingMoved: boolean;
  /** The target lesson had its own drawing, so this one stayed where it was. */
  drawingKept: boolean;
  progressMoved: boolean;
  positionsMoved: number;
}

export const libraryApi = {
  readiness: () => invoke<Readiness>("readiness"),
  list: () => invoke<CourseSummary[]>("list_courses"),
  detail: (courseId: string) => invoke<CourseDetail | null>("get_course_detail", { courseId }),
  registerMedia: (courseId: string, resourceId: string) =>
    invoke<MediaRegistration>("register_media", { courseId, resourceId }),
  chooseAndImport: () => invoke<ImportReceipt | null>("choose_and_import_course"),
  inspectYouTubeLink: (link: string) => invoke<YouTubeLinkKind>("inspect_youtube_link", { link }),
  addYouTubeVideo: (link: string) => invoke<ImportReceipt>("add_youtube_video", { link }),
  addYouTubePlaylist: (playlistId: string, videoIds: string[]) =>
    invoke<PlaylistReceipt>("add_youtube_playlist", { playlistId, videoIds }),
  cancelYouTubePlaylist: () => invoke<boolean>("cancel_youtube_playlist"),
  onYouTubePlaylistProgress: (handler: (progress: PlaylistProgress) => void) =>
    listen<PlaylistProgress>("youtube-playlist-progress", (event) => handler(event.payload)),
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
  /** Verified bytes of a PDF resource. */
  readDocument: (courseId: string, resourceId: string) => invoke<ArrayBuffer>("read_document", { courseId, resourceId }),
  notes: (courseId: string, lessonId: string | null) => invoke<Note[]>("list_notes", { courseId, lessonId }),
  /** Opens a native save dialog; resolves `null` when cancelled. */
  exportBackup: () => invoke<Backup | null>("export_backup"),
  /** Opens a native picker, checks the backup, and stages it for the next start; `null` when cancelled. */
  chooseBackupToRestore: () => invoke<Backup | null>("choose_backup_to_restore"),
  pendingRestore: () => invoke<Backup | null>("get_pending_restore"),
  cancelRestore: () => invoke<void>("cancel_restore"),
  restartApp: () => invoke<void>("restart_app"),
  /** Asks the release server only when called; `null` when Zeron is up to date. */
  checkForUpdate: () => invoke<AvailableUpdate | null>("check_for_update"),
  /** Downloads, verifies against the release key, installs, and restarts. */
  installUpdate: () => invoke<void>("install_update"),
  listCatalogs: () => invoke<Catalog[]>("list_catalogs"),
  /** Opens a native picker; resolves `null` when cancelled. */
  chooseCatalogFile: () => invoke<Catalog | null>("choose_catalog_file"),
  importCatalogUrl: (url: string) => invoke<Catalog>("import_catalog_url", { url }),
  refreshCatalog: (publisherId: string) => invoke<Catalog>("refresh_catalog", { publisherId }),
  /** Courses imported from the catalog stay. */
  removeCatalog: (publisherId: string) => invoke<boolean>("remove_catalog", { publisherId }),
  /** Stop requiring the pinned publisher; the next signed import pins again. */
  forgetCoursePublisher: (courseId: string) => invoke<CoursePublisher>("forget_course_publisher", { courseId }),
  annotationMoves: (courseId: string) => invoke<AnnotationMove[]>("get_annotation_moves", { courseId }),
  moveAnnotations: (courseId: string, fromLessonId: string, toLessonId: string) =>
    invoke<AnnotationMoveReport>("move_annotations", { courseId, fromLessonId, toLessonId }),
  createNote: (courseId: string, lessonId: string, anchor: NoteAnchor, body: string) =>
    invoke<Note>("create_note", { courseId, lessonId, anchor, body }),
  /** Saves only if the note is still at `expectedRevision`; never overwrites a newer note. */
  updateNote: (noteId: string, expectedRevision: number, anchor: NoteAnchor, body: string) =>
    invoke<NoteChange>("update_note", { noteId, expectedRevision, anchor, body }),
  search: (query: string) => invoke<SearchResult[]>("search_library", { query }),
  readText: (courseId: string, resourceId: string) =>
    invoke<{ format: "markdown" | "text"; text: string }>("read_text_resource", { courseId, resourceId }),
  transcript: (courseId: string, mediaResourceId: string) => invoke<Transcript | null>("read_transcript", { courseId, mediaResourceId }),
  /**
   * Remove a course: its torrent stops and its downloads are deleted; a local
   * course's folder is never touched. Notes, sketches, and progress stay.
   * Resolves `false` for a course not in the library.
   */
  removeCourse: (courseId: string) => invoke<boolean>("remove_course", { courseId }),
  /** Courses the learner removed whose notes are kept. */
  removedCourses: () => invoke<CourseSummary[]>("list_removed_courses"),
  /** Lessons of a course with a drawing; IDs only, so the notebook stays light. */
  drawnLessons: (courseId: string) => invoke<string[]>("list_drawn_lessons", { courseId }),
  drawing: (courseId: string, lessonId: string) => invoke<Drawing | null>("get_drawing", { courseId, lessonId }),
  saveDrawing: (courseId: string, lessonId: string, expectedRevision: number | null, sceneJson: string, files: DrawingFile[]) =>
    invoke<DrawingSave>("save_drawing", { courseId, lessonId, expectedRevision, sceneJson, files }),
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
