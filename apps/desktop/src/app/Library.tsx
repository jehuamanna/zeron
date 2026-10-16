import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowRight, BookMarked, BookOpen, Download, HardDrive, Magnet, Share2 } from "lucide-react";
import { useState } from "react";

import { Button } from "../components/ui/button";
import { CatalogsSection } from "../features/catalogs/CatalogsSection";
import { CourseWorkspace } from "../features/course/CourseWorkspace";
import type { WorkspaceTarget } from "../features/course/target";
import { LibraryNotebook } from "../features/notebook/LibraryNotebook";
import { ProgressBar } from "../features/progress/progress";
import { LibrarySearch } from "../features/search/LibrarySearch";
import { TorrentImportPanel } from "../features/torrent-import/TorrentImportPanel";
import { asCommandError, libraryApi, type CourseSummary, type SearchResult } from "../services/library";
import { useLibraryUi } from "../stores/library";

const formatBytes = (bytes: number) =>
  new Intl.NumberFormat(undefined, { style: "unit", unit: "megabyte", maximumFractionDigits: 1 }).format(
    bytes / 1_000_000,
  );

/** A stable cover colour for a course, so each one is recognisable at a glance. */
function coverHue(courseId: string) {
  let hash = 0;
  for (const char of courseId) hash = (hash * 31 + char.charCodeAt(0)) | 0;
  return Math.abs(hash) % 360;
}

/** Library: the courses you learn from. */
export function Library() {
  const queryClient = useQueryClient();
  const { selectedCourseId, selectCourse, notebookOpen, showNotebook } = useLibraryUi();
  // A search result or notebook entry opens its course, then the place it points at.
  const [target, setTarget] = useState<WorkspaceTarget | null>(null);
  const readiness = useQuery({ queryKey: ["readiness"], queryFn: libraryApi.readiness });
  const courses = useQuery({
    queryKey: ["courses"],
    queryFn: libraryApi.list,
    enabled: readiness.data?.state === "ready",
  });
  const importCourse = useMutation({
    mutationFn: libraryApi.chooseAndImport,
    onSuccess: async (receipt) => {
      if (receipt) await queryClient.invalidateQueries({ queryKey: ["courses"] });
    },
  });
  const importError = importCourse.isError
    ? (asCommandError(importCourse.error)?.message ?? "The course folder could not be imported.")
    : null;

  if (selectedCourseId) {
    return (
      <CourseWorkspace
        key={selectedCourseId}
        courseId={selectedCourseId}
        target={target}
        onTargetHandled={() => setTarget(null)}
        onLeave={() => selectCourse(null)}
      />
    );
  }

  const openSearchResult = (result: SearchResult) => {
    selectCourse(result.courseId);
    setTarget({ kind: "search", courseId: result.courseId, result });
  };

  if (notebookOpen && courses.data) {
    return (
      <main className="relative h-full overflow-y-auto">
        <div className="px-4 py-6 sm:px-6 sm:py-8">
          <LibraryNotebook
            courses={courses.data}
            onBack={() => showNotebook(false)}
            onOpen={(courseId, next) => {
              selectCourse(courseId);
              setTarget(next);
            }}
          />
        </div>
      </main>
    );
  }
  const resume = courses.data?.find((course) => course.completedLessons > 0 && course.completedLessons < course.totalLessons) ?? null;

  return (
    <main className="relative h-full overflow-y-auto">
      <div className="mx-auto max-w-7xl px-4 py-6 sm:px-6 sm:py-8">
        {importError ? (
          <div role="alert" className="mb-6 flex flex-wrap items-start justify-between gap-3 rounded-xl border border-red-400/30 bg-red-400/10 p-4 text-red-100">
            <div>
              <p className="font-medium">Course import failed</p>
              <p className="mt-1 text-sm text-red-200/80">{importError}</p>
            </div>
            <Button variant="ghost" size="sm" onClick={() => importCourse.reset()}>Dismiss</Button>
          </div>
        ) : null}

        <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
          <div>
            <p className="mb-1 text-xs font-bold uppercase tracking-[0.2em] text-accent-text">Library</p>
            <h1 className="font-display text-3xl font-semibold tracking-tight sm:text-4xl">Continue learning</h1>
          </div>
          <div className="flex w-full flex-1 items-center justify-end gap-2 sm:w-auto">
            <LibrarySearch onOpen={openSearchResult} />
            <Button variant="ghost" onClick={() => showNotebook(true)} className="shrink-0" title="All notes and sketches across your courses">
              <BookMarked size={16} /> Notebook
            </Button>
            <Button onClick={() => importCourse.mutate()} disabled={importCourse.isPending} className="shrink-0">
              <Download size={16} />
              {importCourse.isPending ? "Verifying…" : "Import local course"}
            </Button>
          </div>
        </div>

        {resume ? <ResumeCard course={resume} onOpen={() => selectCourse(resume.courseId)} /> : null}

        {readiness.isError || courses.isError ? (
          <div role="alert" className="rounded-xl border border-red-400/30 bg-red-400/10 p-5 text-red-100">
            Zeron could not open the local library. Restart the app and check that its data directory is writable.
          </div>
        ) : courses.isPending ? (
          <div aria-busy="true" className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
            <p className="sr-only">Opening your library…</p>
            {[0, 1, 2].map((index) => <div key={index} aria-hidden className="h-56 animate-pulse rounded-2xl bg-white/5" />)}
          </div>
        ) : courses.data?.length ? (
          <section aria-label="Your courses">
            <h2 className="mb-3 text-sm font-semibold text-muted">Your courses · {courses.data.length}</h2>
            <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
              {courses.data.map((course) => <CourseCard key={course.courseId} course={course} onOpen={() => selectCourse(course.courseId)} />)}
            </div>
          </section>
        ) : (
          <div className="rounded-2xl border border-dashed border-line bg-surface px-8 py-16 text-center">
            <div className="mx-auto mb-5 grid size-14 place-items-center rounded-2xl bg-accent/10 text-accent-text">
              <BookOpen size={26} />
            </div>
            <h2 className="font-display text-2xl font-semibold">Your library is ready</h2>
            <p className="mx-auto mt-2 max-w-md text-muted">Import a verified local course folder, or add a course from a .torrent file or magnet link.</p>
          </div>
        )}

        <section aria-label="Add courses" className="mt-10">
          <h2 className="mb-3 text-sm font-semibold text-muted">Add courses</h2>
          <TorrentImportPanel engine={readiness.data?.torrentImport ?? "pending"} />
          <CatalogsSection torrentsAvailable={readiness.data?.torrentImport === "ready"} />
        </section>
      </div>
    </main>
  );
}

function ResumeCard({ course, onOpen }: { course: CourseSummary; onOpen: () => void }) {
  const hue = coverHue(course.courseId);
  return (
    <button
      type="button"
      onClick={onOpen}
      className="group mb-8 flex w-full flex-wrap items-center gap-5 overflow-hidden rounded-2xl border border-line bg-surface p-4 text-left shadow-panel transition hover:border-accent/40"
    >
      <div
        aria-hidden
        className="grid h-24 w-40 shrink-0 place-items-center rounded-xl text-[#fff]"
        style={{ background: `linear-gradient(135deg, hsl(${hue} 55% 38%), hsl(${(hue + 40) % 360} 60% 22%))` }}
      >
        <BookOpen size={28} className="opacity-80" />
      </div>
      <div className="min-w-0 flex-1">
        <span className="block text-xs font-semibold uppercase tracking-wider text-accent-text">Pick up where you left off</span>
        <span className="mt-1 block truncate font-display text-2xl font-semibold">{course.title}</span>
        <span className="block text-sm text-muted">{course.publisherName}</span>
        <div className="mt-3 flex max-w-sm items-center gap-3">
          <span aria-hidden className="h-1.5 flex-1 overflow-hidden rounded-full bg-white/10">
            <span className="block h-full rounded-full bg-accent" style={{ width: `${Math.floor((course.completedLessons / course.totalLessons) * 100)}%` }} />
          </span>
          <span className="text-xs text-muted">{course.completedLessons} of {course.totalLessons} done</span>
        </div>
      </div>
      <span className="inline-flex items-center gap-1.5 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-on-accent transition group-hover:bg-accent-hover">
        Continue <ArrowRight size={15} />
      </span>
    </button>
  );
}

function CourseCard({ course, onOpen }: { course: CourseSummary; onOpen: () => void }) {
  const hue = coverHue(course.courseId);
  const done = course.totalLessons > 0 && course.completedLessons >= course.totalLessons;
  return (
    <button
      type="button"
      onClick={onOpen}
      className="group flex flex-col overflow-hidden rounded-2xl border border-line bg-surface text-left shadow-panel transition hover:-translate-y-0.5 hover:border-accent/40"
    >
      <div
        className="relative flex h-28 items-start justify-between p-3 text-[#fff]"
        style={{ background: `linear-gradient(135deg, hsl(${hue} 55% 40%), hsl(${(hue + 40) % 360} 60% 22%))` }}
      >
        <span className="grid size-9 place-items-center rounded-lg bg-[#fff]/15 backdrop-blur">
          {course.source === "torrent" ? <Magnet size={17} /> : <HardDrive size={17} />}
        </span>
        <div className="flex flex-col items-end gap-1">
          <span className="rounded-full bg-[#000]/30 px-2.5 py-0.5 text-[11px] font-medium backdrop-blur">
            {course.source === "torrent" ? "Torrent · outline only" : "Local · verified"}
          </span>
          {course.sharing ? (
            <span className="inline-flex items-center gap-1 rounded-full bg-[#fff]/20 px-2.5 py-0.5 text-[11px] font-medium backdrop-blur"><Share2 size={10} /> Sharing</span>
          ) : null}
          {done ? <span className="rounded-full bg-[#fff] px-2.5 py-0.5 text-[11px] font-semibold text-[#065f46]">Completed</span> : null}
        </div>
      </div>
      <div className="flex flex-1 flex-col p-4">
        <span className="font-display text-lg font-semibold leading-snug">{course.title}</span>
        <span className="mt-0.5 text-sm text-muted">{course.publisherName}</span>
        {course.totalLessons > 0 ? (
          <div className="mt-4">
            <ProgressBar label={`${course.title} progress`} completed={course.completedLessons} total={course.totalLessons} />
          </div>
        ) : null}
        <div className="mt-auto pt-4">
          <div className="flex items-center justify-between border-t border-line pt-3 text-xs text-subtle">
            <span>Version {course.courseVersion}</span>
            <span>{formatBytes(course.totalResourceBytes)}</span>
          </div>
        </div>
      </div>
    </button>
  );
}
