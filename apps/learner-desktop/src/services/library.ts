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
  availability: (courseId: string) => invoke<ResourceAvailability[]>("get_course_availability", { courseId }),
  getCourseSharing: (courseId: string) => invoke<CourseSharing | null>("get_course_sharing", { courseId }),
  setCourseSharing: (courseId: string, sharing: boolean) =>
    invoke<CourseSharing>("set_course_sharing", { courseId, sharing }),
  onTorrentImport: (handler: (job: TorrentImport) => void) =>
    listen<TorrentImport>("torrent-import", (event) => handler(event.payload)),
};
