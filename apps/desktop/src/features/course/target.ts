import type { SearchResult } from "../../services/library";
import type { SideTab } from "../../stores/workspace";

/** Where to take the learner once a course opens, from search or the library notebook. */
export type WorkspaceTarget =
  | { kind: "search"; courseId: string; result: SearchResult }
  | { kind: "moment"; courseId: string; resourceId: string; seconds: number }
  | { kind: "page"; courseId: string; resourceId: string; page: number }
  | { kind: "lesson"; courseId: string; lessonId: string; tab: SideTab };
