import { useQuery } from "@tanstack/react-query";
import { BookPlus, ChevronDown, ChevronRight, Lock, PencilLine } from "lucide-react";
import { useState } from "react";

import { formatBytes } from "../../lib/format";
import { publisherApi, type DraftSummary, type Release, type SeedingStatus } from "../../services/publisher";
import { ReleaseDetailsPanel } from "./ReleaseDetailsPanel";
import { count, seedingLabel } from "./labels";

export interface CourseGroup {
  courseId: string;
  title: string;
  drafts: DraftSummary[];
  /** Newest first, as the backend lists them. */
  releases: Release[];
  lastActivityUnixSeconds: number;
}

/** Group drafts and releases that share a course id, most recently active first. */
export function groupCourses(drafts: DraftSummary[], releases: Release[]): CourseGroup[] {
  const groups = new Map<string, CourseGroup>();
  const group = (courseId: string) => {
    let existing = groups.get(courseId);
    if (!existing) {
      existing = { courseId, title: "", drafts: [], releases: [], lastActivityUnixSeconds: 0 };
      groups.set(courseId, existing);
    }
    return existing;
  };
  for (const draft of drafts) {
    const course = group(draft.courseId);
    course.drafts.push(draft);
    course.lastActivityUnixSeconds = Math.max(course.lastActivityUnixSeconds, draft.updatedAtUnixSeconds);
  }
  for (const release of releases) {
    const course = group(release.courseId);
    course.releases.push(release);
    course.lastActivityUnixSeconds = Math.max(course.lastActivityUnixSeconds, release.publishedAtUnixSeconds);
  }
  for (const course of groups.values()) {
    course.drafts.sort((a, b) => b.updatedAtUnixSeconds - a.updatedAtUnixSeconds);
    course.title = course.drafts[0]?.title ?? course.releases[0]?.title ?? "Untitled course";
  }
  return [...groups.values()].sort((a, b) => b.lastActivityUnixSeconds - a.lastActivityUnixSeconds);
}

const statusStyles: Record<Release["status"], string> = {
  published: "bg-emerald-300/10 text-emerald-300",
  superseded: "bg-white/10 text-slate-300",
  archived: "bg-white/5 text-slate-500",
};

export function CourseList({ onOpenDraft }: { onOpenDraft: (draftId: string) => void }) {
  const drafts = useQuery({ queryKey: ["publisher-drafts"], queryFn: publisherApi.listDrafts });
  const releases = useQuery({ queryKey: ["publisher-releases"], queryFn: publisherApi.listReleases });
  const distribution = useQuery({ queryKey: ["distribution"], queryFn: publisherApi.getDistribution, refetchInterval: 5000 });

  if (drafts.isPending || releases.isPending) return <p className="text-slate-400">Loading courses…</p>;
  if (drafts.isError || releases.isError) {
    return <p role="alert" className="rounded-2xl border border-red-400/30 bg-red-400/10 p-5 text-red-200">Publisher storage is unavailable.</p>;
  }
  const courses = groupCourses(drafts.data, releases.data);
  if (!courses.length) {
    return (
      <div className="rounded-3xl border border-dashed border-white/15 py-20 text-center">
        <BookPlus className="mx-auto mb-4 text-amber-300" size={30} />
        <h2 className="text-xl font-semibold">Create your first course</h2>
        <p className="mt-2 text-slate-400">Drafts stay editable and separate from immutable published releases.</p>
      </div>
    );
  }
  const seeding = new Map(distribution.data?.releases.map((status) => [status.releaseId, status]));

  return (
    <ul aria-label="Courses" className="space-y-5">
      {courses.map((course) => (
        <li key={course.courseId} aria-label={course.title} className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
          <h2 className="text-lg font-semibold">{course.title}</h2>
          <p className="mt-1 text-xs text-slate-500">
            {count(course.drafts.length, "draft")} · {count(course.releases.length, "release")}
          </p>

          {course.drafts.length ? (
            <div className="mt-4 flex flex-wrap gap-2">
              {course.drafts.map((draft) => (
                <button key={draft.draftId} type="button" onClick={() => onOpenDraft(draft.draftId)} className="flex items-center gap-2 rounded-xl border border-white/10 px-3 py-2 text-sm hover:border-amber-300/40">
                  <PencilLine size={15} className="text-amber-300" />
                  Edit draft <span className="text-xs text-slate-500">v{draft.courseVersion} · r{draft.revision}</span>
                </button>
              ))}
            </div>
          ) : null}

          {course.releases.length ? (
            <section aria-label={`${course.title} releases`} className="mt-5">
              <h3 className="flex items-center gap-2 text-xs font-bold uppercase tracking-[0.18em] text-slate-500"><Lock size={13} /> Releases</h3>
              <ul aria-label="Release history" className="mt-3 space-y-2">
                {course.releases.map((release) => (
                  <ReleaseRow key={release.releaseId} release={release} seeding={distribution.isError ? "unavailable" : seeding.get(release.releaseId)} />
                ))}
              </ul>
            </section>
          ) : null}
        </li>
      ))}
    </ul>
  );
}

function ReleaseRow({ release, seeding }: { release: Release; seeding: SeedingStatus | "unavailable" | undefined }) {
  const [open, setOpen] = useState(false);
  const detailsId = `release-details-${release.releaseId}`;
  return (
    <li className="rounded-2xl border border-white/10 bg-black/10 text-sm">
      <button type="button" aria-expanded={open} aria-controls={detailsId} onClick={() => setOpen((value) => !value)} className="flex w-full flex-wrap items-center gap-x-5 gap-y-1 px-4 py-3 text-left">
        {open ? <ChevronDown size={15} className="text-slate-400" /> : <ChevronRight size={15} className="text-slate-400" />}
        <span className="min-w-0 flex-1">
          <span className="font-semibold">Version {release.courseVersion}</span>
          <span className="ml-3 text-xs text-slate-500">
            {new Date(release.publishedAtUnixSeconds * 1000).toLocaleDateString()} · {count(release.fileCount, "file")} · {formatBytes(release.totalBytes)}
          </span>
        </span>
        <span className="text-xs text-slate-400">{seedingLabel(seeding)}</span>
        <span className={`rounded-full px-2.5 py-0.5 text-xs capitalize ${statusStyles[release.status]}`}>{release.status}</span>
      </button>
      {open ? <ReleaseDetailsPanel id={detailsId} release={release} seeding={seeding} /> : null}
    </li>
  );
}
