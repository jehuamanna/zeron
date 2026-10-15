import { invoke } from "@tauri-apps/api/core";

export interface CourseSummary {
  courseId: string;
  courseVersion: string;
  title: string;
  publisherName: string;
  language: string;
  totalResourceBytes: number;
  importedAtUnixSeconds: number;
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
}

export const libraryApi = {
  readiness: () => invoke<Readiness>("readiness"),
  list: () => invoke<CourseSummary[]>("list_courses"),
  chooseAndImport: () => invoke<ImportReceipt | null>("choose_and_import_course"),
};
