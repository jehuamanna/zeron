import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpen, ChevronLeft, Download, File, HardDrive, Magnet, Network, Play, Search } from "lucide-react";
import { useState } from "react";

import { Button } from "../components/ui/button";
import { NetworkSettings } from "../features/network/NetworkSettings";
import { AvailabilityLabel, formatSize, KeepOfflineToggle, PauseTransfersButton, useCourseDownloads } from "../features/downloads/downloads";
import { LessonPlayer } from "../features/player/LessonPlayer";
import { LessonStatusIcon, MarkCompleteButton, ProgressBar, useCourseProgress } from "../features/progress/progress";
import { CourseSharingCard } from "../features/sharing/CourseSharingCard";
import { TorrentImportPanel } from "../features/torrent-import/TorrentImportPanel";
import { asCommandError, libraryApi } from "../services/library";
import { useLibraryUi } from "../stores/library";

const formatBytes = (bytes: number) =>
  new Intl.NumberFormat(undefined, { style: "unit", unit: "megabyte", maximumFractionDigits: 1 }).format(
    bytes / 1_000_000,
  );

export function App() {
  const queryClient = useQueryClient();
  const { selectedCourseId, selectCourse, networkOpen, showNetwork } = useLibraryUi();
  const [mediaUrl, setMediaUrl] = useState<string | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);
  const [startAt, setStartAt] = useState<number | null>(null);
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
  const detail = useQuery({
    queryKey: ["course-detail", selectedCourseId],
    queryFn: () => libraryApi.detail(selectedCourseId!),
    enabled: selectedCourseId !== null,
  });
  const openMedia = useMutation({
    mutationFn: async (resourceId: string) => {
      const [registration, position] = await Promise.all([
        libraryApi.registerMedia(selectedCourseId!, resourceId),
        libraryApi.playbackPosition(selectedCourseId!, resourceId).catch(() => null),
      ]);
      return { registration, position };
    },
    onSuccess: ({ registration, position }, resourceId) => {
      setStartAt(position);
      setMediaUrl(registration.url);
      setPlaying(resourceId);
    },
  });
  const savePosition = (courseId: string, resourceId: string) => (positionSeconds: number, durationSeconds: number) => {
    void libraryApi
      .savePlaybackPosition(courseId, resourceId, positionSeconds, durationSeconds)
      // Watching far enough completes the lesson, so refresh its progress.
      .then(() => queryClient.invalidateQueries({ queryKey: ["progress", courseId] }))
      .catch(() => undefined);
  };

  const torrentCourse = detail.data?.source === "torrent";
  const importError = importCourse.isError
    ? (asCommandError(importCourse.error)?.message ?? "The course folder could not be imported.")
    : null;
  const availability = useQuery({
    queryKey: ["availability", selectedCourseId],
    queryFn: () => libraryApi.availability(selectedCourseId!),
    enabled: selectedCourseId !== null && torrentCourse,
    refetchInterval: 2000,
  });
  const downloaded = new Map(
    (availability.data ?? []).map((item) => [
      item.resourceId,
      item.byteLength > 0 ? Math.floor((item.verifiedBytes / item.byteLength) * 100) : 100,
    ]),
  );

  const availabilityById = new Map((availability.data ?? []).map((item) => [item.resourceId, item]));
  const offline = useCourseDownloads(selectedCourseId ?? "", selectedCourseId !== null && torrentCourse);
  const lessonProgress = useCourseProgress(selectedCourseId);
  const storage = useQuery({
    queryKey: ["storage"],
    queryFn: libraryApi.storage,
    enabled: selectedCourseId !== null && torrentCourse,
    refetchInterval: 10_000,
  });
  const lowDisk = storage.data?.lowDisk ?? false;

  const leaveCourse = () => {
    setMediaUrl(null);
    setPlaying(null);
    selectCourse(null);
  };

  return (
    <main className="min-h-screen bg-[#071018] text-slate-100">
      <div className="mx-auto max-w-7xl px-6 py-8">
        <header className="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-6">
          <div className="flex items-center gap-3">
            <div className="grid size-10 place-items-center rounded-xl bg-cyan-300 text-slate-950">
              <BookOpen size={22} />
            </div>
            <div>
              <p className="text-xl font-bold tracking-tight">Zeron</p>
              <p className="text-sm text-slate-400">Your learning, available offline.</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" onClick={() => showNetwork(true)} aria-pressed={networkOpen}>
              <Network size={17} /> Network
            </Button>
            <Button onClick={() => importCourse.mutate()} disabled={importCourse.isPending}>
              <Download size={17} />
              {importCourse.isPending ? "Verifying…" : "Import local course"}
            </Button>
          </div>
        </header>

        <section className="py-10">
          {importError ? (
            <div role="alert" className="mb-8 flex flex-wrap items-start justify-between gap-3 rounded-2xl border border-red-400/30 bg-red-400/10 p-5 text-red-100">
              <div>
                <p className="font-medium">Course import failed</p>
                <p className="mt-1 text-sm text-red-200/80">{importError}</p>
              </div>
              <Button variant="ghost" onClick={() => importCourse.reset()}>Dismiss</Button>
            </div>
          ) : null}
          {networkOpen ? (
            <NetworkSettings onBack={() => showNetwork(false)} />
          ) : selectedCourseId ? (
            <div>
              <Button variant="ghost" onClick={leaveCourse} className="mb-7">
                <ChevronLeft size={17} /> Back to library
              </Button>
              {detail.isPending ? (
                <p className="text-slate-400">Opening course…</p>
              ) : detail.isError || !detail.data ? (
                <div role="alert" className="rounded-2xl border border-red-400/30 bg-red-400/10 p-5 text-red-100">
                  This course could not be recovered from the local library.
                </div>
              ) : (
                <div className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_360px]">
                  <div>
                    <p className="mb-2 text-xs font-bold uppercase tracking-[0.24em] text-cyan-300">Course workspace</p>
                    <h1 className="text-4xl font-semibold tracking-tight">{detail.data.title}</h1>
                    {lessonProgress.progress ? (
                      <div className="mt-4 max-w-sm">
                        <ProgressBar label="Course progress" completed={lessonProgress.progress.completedLessons} total={lessonProgress.progress.totalLessons} />
                      </div>
                    ) : null}
                    <p className="mt-3 max-w-2xl text-slate-400">{detail.data.description ?? `Published by ${detail.data.publisherName}`}</p>
                    {torrentCourse ? (
                      <p className="mt-5 max-w-2xl rounded-2xl border border-amber-300/20 bg-amber-300/10 px-4 py-3 text-sm text-amber-100">
                        This course streams from a torrent. Lessons download while you watch, and only data verified against the torrent is played. Each lesson shows how much of it is on this computer.
                      </p>
                    ) : null}
                    {torrentCourse ? <CourseSharingCard courseId={detail.data.courseId} /> : null}
                    <div className="mt-10 rounded-3xl border border-white/10 bg-black/20 p-8">
                      {mediaUrl && playing ? (
                        <LessonPlayer
                          key={mediaUrl}
                          url={mediaUrl}
                          torrent={torrentCourse}
                          verifiedPercent={torrentCourse ? (downloaded.get(playing) ?? 0) : null}
                          onRetry={() => openMedia.mutate(playing)}
                          startAt={startAt}
                          transfersPaused={(offline.downloads?.paused ?? false) || lowDisk}
                          onPosition={savePosition(detail.data.courseId, playing)}
                        />
                      ) : (
                        <div className="grid aspect-video place-items-center rounded-2xl bg-[#03080d] text-slate-500">
                          <div className="text-center">
                            <Play className="mx-auto mb-3" size={30} />
                            <p>{torrentCourse ? "Choose a lesson to stream" : "Choose a video lesson to begin"}</p>
                          </div>
                        </div>
                      )}
                      {openMedia.isError ? (
                        <p role="alert" className="mt-3 text-sm text-red-300">
                          {torrentCourse
                            ? (asCommandError(openMedia.error)?.message ?? "This lesson could not be streamed.")
                            : "This verified media file is no longer available."}
                        </p>
                      ) : null}
                    </div>
                  </div>
                  <aside aria-label="Course contents" className="rounded-3xl border border-white/10 bg-white/[0.04] p-5">
                    <h2 className="mb-3 font-semibold">Course contents</h2>
                    {torrentCourse ? (
                      <div className="mb-5 space-y-2">
                        <div className="flex flex-wrap items-center gap-2">
                        <PauseTransfersButton
                          paused={offline.downloads?.paused ?? false}
                          disabled={offline.pause.isPending || !offline.downloads}
                          onToggle={(paused) => offline.pause.mutate(paused)}
                        />
                        <KeepOfflineToggle
                          label="Keep the whole course offline"
                          kept={offline.downloads?.course ?? false}
                          disabled={offline.change.isPending || !offline.downloads}
                          onToggle={(keep) => offline.change.mutate({ scope: { kind: "course" }, keep })}
                        />
                        </div>
                        <button
                          type="button"
                          onClick={() => offline.remove.mutate(undefined)}
                          disabled={offline.remove.isPending}
                          className="inline-flex items-center gap-1 rounded-md bg-white/5 px-2 py-1 text-xs text-slate-300 hover:bg-white/10 disabled:opacity-60"
                        >
                          {offline.remove.isPending ? "Freeing space…" : "Free up space"}
                        </button>
                        {offline.remove.isSuccess ? (
                          <p role="status" className="text-xs text-slate-400">
                            Freed {formatSize(offline.remove.data.freedBytes)}. Lessons kept offline were not touched.
                          </p>
                        ) : null}
                        {lowDisk ? <p role="alert" className="text-xs text-amber-200">Downloads are paused because less than 1 GB is free on this disk. Free up space to continue.</p> : null}
                        {offline.downloads?.paused ? <p className="text-xs text-amber-200">Downloads and sharing are paused. Lessons already on this computer still play.</p> : null}
                        <p className="text-xs text-slate-500">Kept lessons download in full. Stopping keeps what is already on this computer.</p>
                        {offline.error ? <p role="alert" className="text-xs text-red-300">{offline.error}</p> : null}
                      </div>
                    ) : null}
                    <div className="space-y-6">
                      {detail.data.modules.map((module) => (
                        <section key={module.id}>
                          <div className="mb-2 flex items-center justify-between gap-2">
                            <h3 className="text-xs font-bold uppercase tracking-wider text-slate-500">{module.title}</h3>
                            {torrentCourse ? (
                              <KeepOfflineToggle
                                label={`Keep module ${module.title} offline`}
                                kept={offline.downloads?.modules.includes(module.id) ?? false}
                                inherited={offline.downloads?.course ?? false}
                                disabled={offline.change.isPending || !offline.downloads}
                                onToggle={(keep) => offline.change.mutate({ scope: { kind: "module", id: module.id }, keep })}
                              />
                            ) : null}
                          </div>
                          <div className="space-y-2">
                            {module.lessons.map((lesson) => (
                              <div key={lesson.id} className="rounded-xl border border-white/10 bg-black/10 p-3">
                                <div className="flex items-start justify-between gap-2">
                                  <p className="flex items-start gap-2 text-sm font-medium">
                                    <LessonStatusIcon status={lessonProgress.statusOf(lesson.id)} />
                                    {lesson.title}
                                  </p>
                                  <div className="flex shrink-0 flex-wrap justify-end gap-1">
                                  <MarkCompleteButton
                                    lessonTitle={lesson.title}
                                    completed={lessonProgress.statusOf(lesson.id) === "completed"}
                                    disabled={lessonProgress.mark.isPending}
                                    onToggle={(completed) => lessonProgress.mark.mutate({ lessonId: lesson.id, completed })}
                                  />
                                  {torrentCourse ? (
                                    <KeepOfflineToggle
                                      label={`Keep lesson ${lesson.title} offline`}
                                      kept={offline.downloads?.lessons.includes(lesson.id) ?? false}
                                      inherited={(offline.downloads?.course ?? false) || (offline.downloads?.modules.includes(module.id) ?? false)}
                                      disabled={offline.change.isPending || !offline.downloads}
                                      onToggle={(keep) => offline.change.mutate({ scope: { kind: "lesson", id: lesson.id }, keep })}
                                    />
                                  ) : null}
                                  </div>
                                </div>
                                <div className="mt-2 flex flex-wrap gap-2">
                                  {lesson.resources.map((resource) => resource.kind === "video" ? (
                                    <span key={resource.id} className="inline-flex flex-wrap items-center gap-2">
                                    <button
                                      type="button"
                                      onClick={() => openMedia.mutate(resource.id)}
                                      disabled={openMedia.isPending}
                                      aria-current={playing === resource.id ? "true" : undefined}
                                      className={`inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs hover:bg-cyan-300/20 disabled:opacity-50 ${playing === resource.id ? "bg-cyan-300/25 text-cyan-100" : "bg-cyan-300/10 text-cyan-200"}`}
                                    >
                                      <Play size={12} /> {playing === resource.id ? "Playing" : "Play video"}
                                      {torrentCourse ? <span className="text-cyan-200/70">· {downloaded.get(resource.id) ?? 0}% downloaded</span> : null}
                                    </button>
                                    {torrentCourse ? <AvailabilityLabel item={availabilityById.get(resource.id)} /> : null}
                                    {torrentCourse && availabilityById.get(resource.id)?.kept === false && (availabilityById.get(resource.id)?.verifiedBytes ?? 0) > 0 && playing !== resource.id ? (
                                      <button
                                        type="button"
                                        aria-label={`Remove download of ${lesson.title}`}
                                        onClick={() => offline.remove.mutate([resource.id])}
                                        disabled={offline.remove.isPending}
                                        className="text-[11px] text-slate-400 underline-offset-2 hover:text-red-200 hover:underline disabled:opacity-60"
                                      >
                                        Remove download
                                      </button>
                                    ) : null}
                                    </span>
                                  ) : (
                                    <span key={resource.id} className="inline-flex items-center gap-1 rounded-md bg-white/5 px-2 py-1 text-xs text-slate-400">
                                      <File size={12} /> {resource.kind}
                                    </span>
                                  ))}
                                </div>
                              </div>
                            ))}
                          </div>
                        </section>
                      ))}
                    </div>
                  </aside>
                </div>
              )}
            </div>
          ) : (
          <>
          <div className="mb-8 flex items-end justify-between gap-6">
            <div>
              <p className="mb-2 text-xs font-bold uppercase tracking-[0.24em] text-cyan-300">Library</p>
              <h1 className="text-4xl font-semibold tracking-tight">Continue learning</h1>
            </div>
            <div className="flex items-center gap-2 rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-slate-400">
              <Search size={16} />
              <span className="text-sm">Search arrives with lesson indexing</span>
            </div>
          </div>

          <TorrentImportPanel engine={readiness.data?.torrentImport ?? "pending"} />

          {readiness.isError || courses.isError ? (
            <div role="alert" className="rounded-2xl border border-red-400/30 bg-red-400/10 p-5 text-red-100">
              Zeron could not open the local library. Restart the app and check that its data directory is writable.
            </div>
          ) : courses.isPending ? (
            <div className="rounded-2xl border border-white/10 bg-white/[0.03] p-8 text-slate-400">Opening your library…</div>
          ) : courses.data?.length ? (
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
              {courses.data.map((course) => (
                <button type="button" onClick={() => selectCourse(course.courseId)} key={course.courseId} className="rounded-2xl border border-white/10 bg-white/[0.04] p-5 text-left shadow-2xl shadow-black/10 transition hover:-translate-y-0.5 hover:border-cyan-300/40">
                  <div className="mb-8 flex items-start justify-between gap-3">
                    <div className="grid size-11 place-items-center rounded-xl bg-cyan-300/10 text-cyan-300">
                      {course.source === "torrent" ? <Magnet size={20} /> : <HardDrive size={20} />}
                    </div>
                    <div className="flex flex-col items-end gap-1">
                      <span className="rounded-full border border-white/10 px-2.5 py-1 text-xs text-slate-400">
                        {course.source === "torrent" ? "Torrent · outline only" : "Local · verified"}
                      </span>
                      {course.sharing ? (
                        <span className="rounded-full bg-cyan-300/10 px-2.5 py-1 text-xs text-cyan-200">Sharing</span>
                      ) : null}
                    </div>
                  </div>
                  <h2 className="text-lg font-semibold">{course.title}</h2>
                  <p className="mt-1 text-sm text-slate-400">{course.publisherName}</p>
                  {course.totalLessons > 0 ? (
                    <div className="mt-4">
                      <ProgressBar label={`${course.title} progress`} completed={course.completedLessons} total={course.totalLessons} />
                    </div>
                  ) : null}
                  <div className="mt-5 flex items-center justify-between border-t border-white/10 pt-4 text-xs text-slate-400">
                    <span>Version {course.courseVersion}</span>
                    <span>{formatBytes(course.totalResourceBytes)}</span>
                  </div>
                </button>
              ))}
            </div>
          ) : (
            <div className="rounded-3xl border border-dashed border-white/15 bg-white/[0.025] px-8 py-20 text-center">
              <div className="mx-auto mb-5 grid size-14 place-items-center rounded-2xl bg-white/5 text-cyan-300">
                <BookOpen size={26} />
              </div>
              <h2 className="text-xl font-semibold">Your library is ready</h2>
              <p className="mx-auto mt-2 max-w-md text-slate-400">Import a verified local course folder, or add a course from a .torrent file or magnet link.</p>
            </div>
          )}
          </>
          )}
        </section>
      </div>
    </main>
  );
}
