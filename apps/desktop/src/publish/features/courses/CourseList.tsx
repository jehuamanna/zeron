import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookPlus, ChevronDown, ChevronRight, Lock, PencilLine, Trash2 } from "lucide-react";
import { useState } from "react";

import { formatBytes } from "../../lib/format";
import { asCommandError, publisherApi, type DraftSummary, type Release, type SeedingStatus } from "../../services/publisher";
import { ConfirmAction } from "./ConfirmAction";
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
          <div className="flex items-start justify-between gap-3">
            <div className="min-w-0">
              <h2 className="text-lg font-semibold">{course.title}</h2>
              <p className="mt-1 text-xs text-slate-500">
                {count(course.drafts.length, "draft")} · {count(course.releases.length, "release")}
              </p>
            </div>
            <DeleteCourse course={course} />
          </div>

          {course.drafts.length ? (
            <div className="mt-4 space-y-2">
              {course.drafts.map((draft) => <DraftRow key={draft.draftId} draft={draft} onOpen={onOpenDraft} />)}
            </div>
          ) : null}

          {course.releases.length ? (
            <section aria-label={`${course.title} releases`} className="mt-5">
              <h3 className="flex items-center gap-2 text-xs font-bold uppercase tracking-[0.18em] text-slate-500"><Lock size={13} /> Releases</h3>
              <ul aria-label="Release history" className="mt-3 space-y-2">
                {course.releases.map((release) => (
                  <ReleaseRow
                    key={release.releaseId}
                    release={release}
                    currentVersion={course.releases.find((candidate) => candidate.status === "published")?.courseVersion ?? null}
                    seeding={distribution.isError ? "unavailable" : seeding.get(release.releaseId)}
                    onOpenDraft={onOpenDraft}
                  />
                ))}
              </ul>
            </section>
          ) : null}
        </li>
      ))}
    </ul>
  );
}

function DraftRow({ draft, onOpen }: { draft: DraftSummary; onOpen: (draftId: string) => void }) {
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const remove = useMutation({
    mutationFn: () => publisherApi.deleteDraft(draft.draftId, draft.revision),
    onSuccess: () => {
      queryClient.removeQueries({ queryKey: ["publisher-draft", draft.draftId] });
      void queryClient.invalidateQueries({ queryKey: ["publisher-drafts"] });
    },
    onError: (error) => {
      if (asCommandError(error)?.code === "revision_conflict") void queryClient.invalidateQueries({ queryKey: ["publisher-drafts"] });
    },
  });
  const failure = remove.isError ? deleteError(remove.error) : null;
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <button type="button" onClick={() => onOpen(draft.draftId)} className="flex items-center gap-2 rounded-xl border border-white/10 px-3 py-2 text-sm hover:border-amber-300/40">
          <PencilLine size={15} className="text-amber-300" />
          Edit draft <span className="text-xs text-slate-500">v{draft.courseVersion} · r{draft.revision}</span>
        </button>
        {!confirming ? (
          <button type="button" aria-label={`Delete draft v${draft.courseVersion}`} onClick={() => setConfirming(true)} className="rounded-xl border border-white/10 p-2 text-slate-400 hover:border-red-400/40 hover:text-red-200">
            <Trash2 size={15} />
          </button>
        ) : null}
      </div>
      {confirming ? (
        <ConfirmAction
          title={`Delete draft v${draft.courseVersion}?`}
          confirmLabel="Delete draft"
          pendingLabel="Deleting…"
          pending={remove.isPending}
          error={failure}
          onConfirm={() => remove.mutate()}
          onCancel={() => { remove.reset(); setConfirming(false); }}
        >
          The draft's structure and settings are removed from Zeron. Published releases and your original media files are kept.
        </ConfirmAction>
      ) : null}
    </div>
  );
}

function deleteError(error: unknown) {
  const command = asCommandError(error);
  switch (command?.code) {
    case "revision_conflict":
      return "This draft was saved since the list loaded. Check it before deleting.";
    case "draft_busy":
      return "This draft has a release preparation running or ready to publish. Finish or cancel it first.";
    default:
      return command?.message ?? "The draft could not be deleted.";
  }
}

interface ReleaseRowProps {
  release: Release;
  /** Version of the course's current (published) release, if it has one. */
  currentVersion: string | null;
  seeding: SeedingStatus | "unavailable" | undefined;
  onOpenDraft: (draftId: string) => void;
}

function ReleaseRow({ release, currentVersion, seeding, onOpenDraft }: ReleaseRowProps) {
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
        <span className="text-xs text-slate-400">{release.files === "deleted" ? "Files deleted" : seedingLabel(seeding)}</span>
        <span className={`rounded-full px-2.5 py-0.5 text-xs capitalize ${statusStyles[release.status]}`}>{release.status}</span>
      </button>
      {open ? <ReleaseDetailsPanel id={detailsId} release={release} currentVersion={currentVersion} seeding={seeding} onOpenDraft={onOpenDraft} /> : null}
    </li>
  );
}

/** Deletes a whole course after the publisher types its title (ADR 0016). */
function DeleteCourse({ course }: { course: CourseGroup }) {
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const [typed, setTyped] = useState("");
  const remove = useMutation({
    mutationFn: () => publisherApi.deleteCourse(course.courseId, typed),
    onSuccess: () => {
      setConfirming(false);
      for (const key of ["publisher-drafts", "publisher-releases", "distribution", "publisher-catalog", "distribution-activity"]) {
        void queryClient.invalidateQueries({ queryKey: [key] });
      }
    },
  });
  const inputId = `delete-course-${course.courseId}`;
  const bytes = course.releases.filter((release) => release.files !== "deleted").reduce((sum, release) => sum + release.totalBytes, 0);
  const live = course.releases.filter((release) => release.status !== "archived").length;
  if (!confirming) {
    return (
      <button
        type="button"
        aria-label={`Delete course ${course.title}`}
        title="Delete course"
        onClick={() => setConfirming(true)}
        className="grid size-8 shrink-0 place-items-center rounded-lg text-slate-500 hover:bg-red-400/10 hover:text-red-300"
      >
        <Trash2 size={16} />
      </button>
    );
  }
  return (
    <div className="w-full max-w-md">
      <ConfirmAction
        title={`Delete “${course.title}”?`}
        confirmLabel="Delete course permanently"
        pendingLabel="Deleting…"
        pending={remove.isPending}
        confirmDisabled={typed.trim() !== course.title.trim()}
        error={remove.isError ? (asCommandError(remove.error)?.message ?? "The course could not be deleted.") : null}
        onConfirm={() => remove.mutate()}
        onCancel={() => { remove.reset(); setTyped(""); setConfirming(false); }}
      >
        <ul className="list-disc space-y-1 pl-4">
          <li>{count(course.drafts.length, "draft")} deleted.</li>
          {live ? <li>{count(live, "release")} stop seeding; learners can no longer download from this computer.</li> : null}
          {course.releases.length ? <li>Taken out of your catalog, and {formatBytes(bytes)} of release files deleted from this computer.</li> : null}
        </ul>
        <p className="mt-2">This cannot be undone. Release records stay hidden so a version number is never reused.</p>
        <label htmlFor={inputId} className="mt-3 block text-xs font-semibold text-slate-300">Type {course.title} to confirm</label>
        <input id={inputId} value={typed} onChange={(event) => setTyped(event.target.value)} autoComplete="off" spellCheck={false} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs text-slate-100" />
      </ConfirmAction>
    </div>
  );
}
