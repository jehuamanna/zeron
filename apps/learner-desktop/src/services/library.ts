import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type CourseSource = "local" | "torrent";

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
  onTorrentImport: (handler: (job: TorrentImport) => void) =>
    listen<TorrentImport>("torrent-import", (event) => handler(event.payload)),
};
