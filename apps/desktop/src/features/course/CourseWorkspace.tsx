import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookMarked, ChevronLeft, ChevronRight, Keyboard, LayoutDashboard, Maximize, Minimize, NotebookPen, PanelLeft, PanelRight } from "lucide-react";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { useMediaQuery } from "../../lib/useMediaQuery";
import { asCommandError, libraryApi, type NoteAnchor, type Region } from "../../services/library";
import { useWorkspace, type SideTab } from "../../stores/workspace";
import { DocumentViewer } from "../documents/DocumentViewer";
import { percentOf, useCourseDownloads } from "../downloads/downloads";
import { DrawingPanel } from "../drawings/DrawingPanel";
import { NotesPanel, type NoteRequest } from "../notes/NotesPanel";
import { LessonPlayer, type PlayerHandle } from "../player/LessonPlayer";
import { MarkCompleteButton, useCourseProgress } from "../progress/progress";
import { TranscriptPanel } from "../transcripts/TranscriptPanel";
import { CourseContents, type Showing } from "./CourseContents";
import { CourseNotebook } from "./CourseNotebook";
import { CourseOverview } from "./CourseOverview";
import { isReadable, nextToLearn, placeLessons, primaryResource, resourceLabel, type Lesson, type PlacedLesson } from "./lessons";
import { SidePanel } from "./SidePanel";
import type { WorkspaceTarget } from "./target";

// Markdown rendering and Shiki load only when a lesson text is opened.
const TextReader = lazy(() => import("../reader/TextReader").then((module) => ({ default: module.TextReader })));

/** What the stage, the large area in the middle, shows. */
type Stage =
  | { kind: "overview" }
  | { kind: "notebook" }
  | { kind: "video"; resourceId: string; url: string; startAt: number | null }
  | { kind: "pdf"; resourceId: string; page: number; region: Region | null }
  | { kind: "text"; resourceId: string };

const SHORTCUTS: Array<[string, string]> = [
  ["K", "Play or pause"],
  ["J / L", "Back or forward 10 seconds"],
  ["← / →", "Back or forward 5 seconds, or PDF page"],
  ["↑ / ↓", "Volume"],
  ["M", "Mute"],
  ["C", "Captions"],
  ["F", "Full-screen video"],
  ["N", "New note at this moment or page"],
  ["[", "Show or hide contents"],
  ["]", "Show or hide the side panel"],
];

/**
 * One course, fitted to the window: contents on the left, the lesson in the
 * middle, and notes, sketch, and transcript on the right. Nothing scrolls the
 * page; each pane scrolls on its own.
 */
export function CourseWorkspace({ courseId, target, onTargetHandled, onLeave }: {
  courseId: string;
  /** A search match or notebook entry to open once the course has loaded. */
  target: WorkspaceTarget | null;
  onTargetHandled: () => void;
  onLeave: () => void;
}) {
  const queryClient = useQueryClient();
  const [stage, setStage] = useState<Stage>({ kind: "overview" });
  const [activeLessonId, setActiveLessonId] = useState<string | null>(null);
  const [noteRequest, setNoteRequest] = useState<NoteRequest | null>(null);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const player = useRef<PlayerHandle>(null);
  // A note's timestamp to start at when it opens a video that is not playing.
  const pendingSeek = useRef<number | null>(null);

  const layout = useWorkspace();
  const contentsDocked = useMediaQuery("(min-width: 1280px)");
  const sideDocked = useMediaQuery("(min-width: 1024px)");
  // Overlay drawers on narrow windows start closed and do not touch the saved layout.
  const [contentsOverlay, setContentsOverlay] = useState(false);
  const [sideOverlay, setSideOverlay] = useState(false);
  const contentsOpen = contentsDocked ? layout.contentsOpen : contentsOverlay;
  const sideOpen = sideDocked ? layout.sideOpen : sideOverlay;
  const showContents = (open: boolean) => (contentsDocked ? layout.showContents(open) : setContentsOverlay(open));
  const showSide = (open: boolean, tab?: SideTab) => {
    if (sideDocked) layout.showSide(open, tab);
    else {
      setSideOverlay(open);
      if (tab) layout.showSide(layout.sideOpen, tab);
    }
  };

  const detail = useQuery({ queryKey: ["course-detail", courseId], queryFn: () => libraryApi.detail(courseId) });
  const course = detail.data ?? null;
  const torrent = course?.source === "torrent";
  const placed = useMemo(() => (course ? placeLessons(course) : []), [course]);
  const progress = useCourseProgress(courseId);
  const offline = useCourseDownloads(courseId, torrent);
  const availability = useQuery({
    queryKey: ["availability", courseId],
    queryFn: () => libraryApi.availability(courseId),
    enabled: torrent,
    refetchInterval: 2000,
  });
  const availabilityById = new Map((availability.data ?? []).map((item) => [item.resourceId, item]));
  const storage = useQuery({ queryKey: ["storage"], queryFn: libraryApi.storage, enabled: torrent, refetchInterval: 10_000 });
  const lowDisk = storage.data?.lowDisk ?? false;

  const placedOf = (lessonId: string | null) => placed.find((item) => item.lesson.id === lessonId) ?? null;
  const lessonOf = (resourceId: string) => placed.find((item) => item.lesson.resources.some((resource) => resource.id === resourceId))?.lesson ?? null;
  const active = placedOf(activeLessonId);

  const openMedia = useMutation({
    mutationFn: async (resourceId: string) => {
      const [registration, position] = await Promise.all([
        libraryApi.registerMedia(courseId, resourceId),
        libraryApi.playbackPosition(courseId, resourceId).catch(() => null),
      ]);
      return { registration, position };
    },
    onSuccess: ({ registration, position }, resourceId) => {
      const startAt = pendingSeek.current ?? position;
      pendingSeek.current = null;
      setStage({ kind: "video", resourceId, url: registration.url, startAt });
      setActiveLessonId(lessonOf(resourceId)?.id ?? null);
    },
  });
  const savePosition = (resourceId: string) => (positionSeconds: number, durationSeconds: number) => {
    void libraryApi
      .savePlaybackPosition(courseId, resourceId, positionSeconds, durationSeconds)
      // Watching far enough completes the lesson, so refresh its progress.
      .then(() => queryClient.invalidateQueries({ queryKey: ["progress", courseId] }))
      .catch(() => undefined);
  };

  const openDocument = (resourceId: string, page = 1) => {
    const lesson = lessonOf(resourceId);
    if (!lesson) return;
    setActiveLessonId(lesson.id);
    setStage({ kind: "pdf", resourceId, page, region: null });
  };
  const openText = (resourceId: string) => {
    const lesson = lessonOf(resourceId);
    if (!lesson) return;
    setActiveLessonId(lesson.id);
    setStage({ kind: "text", resourceId });
  };
  const openResource = (lesson: Lesson, resourceId: string) => {
    const resource = lesson.resources.find((candidate) => candidate.id === resourceId);
    if (!resource) return;
    if (!contentsDocked) setContentsOverlay(false);
    if (resource.kind === "video") openMedia.mutate(resourceId);
    else if (resource.kind === "pdf") openDocument(resourceId);
    else if (isReadable(resource)) openText(resourceId);
  };
  const openLesson = (lesson: Lesson) => {
    const resource = primaryResource(lesson);
    if (resource) openResource(lesson, resource.id);
    else {
      setActiveLessonId(lesson.id);
      setStage({ kind: "overview" });
      if (!contentsDocked) setContentsOverlay(false);
    }
  };
  const seekTo = (resourceId: string, seconds: number) => {
    if (stage.kind === "video" && stage.resourceId === resourceId) {
      player.current?.seek(seconds);
      return;
    }
    pendingSeek.current = seconds;
    openMedia.mutate(resourceId);
  };
  const currentPlayerTime = useCallback(() => player.current?.currentTime() ?? 0, []);

  const openLessonWith = (lesson: Lesson, tab: SideTab) => {
    openLesson(lesson);
    showSide(true, tab);
  };

  // A search match or notebook entry opens its place once the course has loaded.
  useEffect(() => {
    if (!target || course?.courseId !== target.courseId) return;
    onTargetHandled();
    if (target.kind === "moment") seekTo(target.resourceId, target.seconds);
    else if (target.kind === "page") openDocument(target.resourceId, target.page);
    else if (target.kind === "lesson") {
      const lesson = placedOf(target.lessonId)?.lesson;
      if (lesson) openLessonWith(lesson, target.tab);
    } else {
      const { result } = target;
      if (result.kind === "transcript") seekTo(result.sourceId, result.position ?? 0);
      else if (result.kind === "text") openText(result.sourceId);
      else if (result.lessonId) setActiveLessonId(result.lessonId);
    }
    // Runs once the target's course has loaded; the openers are fresh each render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, course]);

  const documentAnchor: Extract<NoteAnchor, { kind: "pdfPage" }> | null =
    stage.kind === "pdf" ? { kind: "pdfPage", resourceId: stage.resourceId, page: stage.page, region: stage.region } : null;
  const playingInActive = stage.kind === "video" && active?.lesson.resources.some((resource) => resource.id === stage.resourceId) ? stage.resourceId : null;

  const requestNote = (anchor: NoteAnchor, body?: string) => {
    showSide(true, "notes");
    setNoteRequest(body === undefined ? { id: Date.now(), anchor } : { id: Date.now(), anchor, body });
  };
  const editNote = (noteId: string) => {
    showSide(true, "notes");
    setNoteRequest({ id: Date.now(), edit: noteId });
  };

  // Notes of the playing lesson mark the timeline; its transcript gives captions.
  const videoNotes = useQuery({
    queryKey: ["notes", courseId, activeLessonId],
    queryFn: () => libraryApi.notes(courseId, activeLessonId),
    enabled: stage.kind === "video" && activeLessonId !== null,
  });
  const transcript = useQuery({
    queryKey: ["transcript", courseId, stage.kind === "video" ? stage.resourceId : null],
    queryFn: () => libraryApi.transcript(courseId, stage.kind === "video" ? stage.resourceId : ""),
    enabled: stage.kind === "video",
    staleTime: Infinity,
  });

  const sideTabs: SideTab[] = stage.kind === "video" ? ["notes", "sketch", "transcript"] : ["notes", "sketch"];
  const sideTab = sideTabs.includes(layout.sideTab) ? layout.sideTab : "notes";

  // Keyboard shortcuts, ignored while typing.
  const keys = useRef<(event: KeyboardEvent) => void>(() => {});
  keys.current = (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey || event.defaultPrevented) return;
    const target = event.target;
    if (target instanceof Element && target.closest("input, textarea, select, [contenteditable='true'], .excalidraw")) return;
    const key = event.key.toLowerCase();
    if (key === "[") showContents(!contentsOpen);
    else if (key === "]") showSide(!sideOpen);
    else if (key === "?") setShortcutsOpen((open) => !open);
    else if (stage.kind === "video" && key === "k") player.current?.togglePlay();
    else if (stage.kind === "video" && key === "j") player.current?.skip(-10);
    else if (stage.kind === "video" && key === "l") player.current?.skip(10);
    else if (stage.kind === "video" && key === "arrowleft") player.current?.skip(-5);
    else if (stage.kind === "video" && key === "arrowright") player.current?.skip(5);
    else if (stage.kind === "video" && key === "arrowup") player.current?.nudgeVolume(0.1);
    else if (stage.kind === "video" && key === "arrowdown") player.current?.nudgeVolume(-0.1);
    else if (stage.kind === "video" && key === "m") player.current?.toggleMute();
    else if (stage.kind === "video" && key === "c") player.current?.toggleCaptions();
    else if (stage.kind === "video" && key === "f") player.current?.toggleFullscreen();
    else if (stage.kind === "video" && key === "n") requestNote({ kind: "videoTime", resourceId: stage.resourceId, seconds: Math.floor(currentPlayerTime()) });
    else if (stage.kind === "pdf" && key === "n" && documentAnchor) requestNote(documentAnchor);
    else if (stage.kind === "pdf" && key === "arrowright") setStage({ ...stage, page: stage.page + 1, region: null });
    else if (stage.kind === "pdf" && key === "arrowleft" && stage.page > 1) setStage({ ...stage, page: stage.page - 1, region: null });
    else if (key === "escape" && shortcutsOpen) setShortcutsOpen(false);
    else return;
    event.preventDefault();
  };
  useEffect(() => {
    const follow = () => setFullscreen(document.fullscreenElement === document.documentElement);
    document.addEventListener("fullscreenchange", follow);
    return () => document.removeEventListener("fullscreenchange", follow);
  }, []);

  useEffect(() => {
    const listener = (event: KeyboardEvent) => keys.current(event);
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, []);

  if (detail.isPending) {
    return (
      <div className="grid h-full place-items-center text-sm text-muted">
        <p>Opening course…</p>
      </div>
    );
  }
  if (detail.isError || !course) {
    return (
      <div className="p-6">
        <BackButton onLeave={onLeave} />
        <div role="alert" className="mt-4 rounded-xl border border-red-400/30 bg-red-400/10 p-5 text-red-100">
          This course could not be recovered from the local library.
        </div>
      </div>
    );
  }

  const showing: Showing = stage.kind === "overview" || stage.kind === "notebook" ? null : { kind: stage.kind, resourceId: stage.resourceId };
  const previous = active ? placed[active.index - 1] ?? null : null;
  const next = active ? placed[active.index + 1] ?? null : null;
  const upNext = nextToLearn(placed, progress.statusOf);
  const percent = progress.progress && progress.progress.totalLessons > 0 ? Math.floor((progress.progress.completedLessons / progress.progress.totalLessons) * 100) : 0;
  const headerButton = "grid size-8 shrink-0 place-items-center rounded-md text-muted hover:bg-white/10 hover:text-fg";

  const contents = (
    <CourseContents
      course={course}
      placed={placed}
      activeLessonId={activeLessonId}
      showing={showing}
      opening={openMedia.isPending}
      torrent={torrent}
      availability={availabilityById}
      offline={offline}
      progress={progress}
      lowDisk={lowDisk}
      onOpenResource={openResource}
      onOpenLesson={openLesson}
      onNotes={(lesson) => {
        setActiveLessonId(lesson.id);
        showSide(true, "notes");
      }}
    />
  );

  const side = (
    <SidePanel tabs={sideTabs} active={sideTab} resizable={sideDocked} onClose={() => showSide(false)}>
      {!active ? (
        <div className="grid h-full place-items-center p-6 text-center">
          <div>
            <NotebookPen size={22} className="mx-auto mb-2 text-faint" />
            <p className="text-sm text-muted">Open a lesson to take notes and sketch.</p>
          </div>
        </div>
      ) : sideTab === "sketch" ? (
        <DrawingPanel key={`drawing-${active.lesson.id}`} fill courseId={courseId} lessonId={active.lesson.id} lessonTitle={active.lesson.title} />
      ) : sideTab === "transcript" && stage.kind === "video" ? (
        <TranscriptPanel
          key={stage.resourceId}
          courseId={courseId}
          mediaResourceId={stage.resourceId}
          currentTime={currentPlayerTime}
          onSeek={(seconds) => seekTo(stage.resourceId, seconds)}
          missing={<p className="p-6 text-center text-sm text-muted">This lesson has no transcript.</p>}
        />
      ) : (
        <NotesPanel
          key={active.lesson.id}
          courseId={courseId}
          lessonId={active.lesson.id}
          lessonTitle={active.lesson.title}
          videoResourceId={playingInActive}
          currentTime={currentPlayerTime}
          onSeek={seekTo}
          onOpenPage={(resourceId, page) => openDocument(resourceId, page)}
          documentAnchor={documentAnchor}
          onNoteCreated={() => stage.kind === "pdf" && setStage({ ...stage, region: null })}
          request={noteRequest}
          onRequestHandled={() => setNoteRequest(null)}
        />
      )}
    </SidePanel>
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* Course bar */}
      <div className="flex h-12 shrink-0 items-center gap-1.5 border-b border-line bg-surface px-2 sm:px-3">
        <BackButton onLeave={onLeave} />
        <button type="button" className={headerButton} aria-label={contentsOpen ? "Hide contents" : "Show contents"} aria-pressed={contentsOpen} title="Contents ([)" onClick={() => showContents(!contentsOpen)}>
          <PanelLeft size={17} />
        </button>
        <div className="mx-1 min-w-0 flex-1">
          <p className="truncate font-display text-[15px] font-semibold leading-tight">{course.title}</p>
          <p className="truncate text-[11px] text-subtle">{course.publisherName}</p>
        </div>
        {progress.progress ? (
          <div className="hidden items-center gap-2 md:flex" title={`${progress.progress.completedLessons} of ${progress.progress.totalLessons} lessons complete`}>
            <div aria-hidden className="h-1.5 w-28 overflow-hidden rounded-full bg-white/10">
              <div className="h-full rounded-full bg-accent" style={{ width: `${percent}%` }} />
            </div>
            <span className="text-xs tabular-nums text-muted">{percent}%</span>
          </div>
        ) : null}
        <button
          type="button"
          className={`${headerButton} ${stage.kind === "overview" ? "text-accent-text" : ""}`}
          aria-label="Course overview"
          title="Course overview"
          onClick={() => setStage({ kind: "overview" })}
        >
          <LayoutDashboard size={17} />
        </button>
        <button
          type="button"
          aria-pressed={stage.kind === "notebook"}
          title="All notes and sketches of this course"
          onClick={() => setStage({ kind: "notebook" })}
          className={`flex h-8 shrink-0 items-center gap-1.5 rounded-md px-2 text-sm ${stage.kind === "notebook" ? "bg-accent/10 text-accent-text" : "text-muted hover:bg-white/10 hover:text-fg"}`}
        >
          <BookMarked size={16} /> <span className="hidden sm:inline">Notebook</span>
          <span className="sr-only sm:hidden">Notebook</span>
        </button>
        <div className="relative">
          <button type="button" className={headerButton} aria-label="Keyboard shortcuts" aria-expanded={shortcutsOpen} title="Keyboard shortcuts (?)" onClick={() => setShortcutsOpen(!shortcutsOpen)}>
            <Keyboard size={17} />
          </button>
          {shortcutsOpen ? (
            <div role="dialog" aria-label="Keyboard shortcuts" className="absolute right-0 top-10 z-40 w-72 rounded-xl border border-line bg-surface p-3 shadow-panel">
              <p className="mb-2 text-xs font-semibold text-muted">Keyboard shortcuts</p>
              <dl className="space-y-1.5 text-sm">
                {SHORTCUTS.map(([key, action]) => (
                  <div key={key} className="flex items-center justify-between gap-3">
                    <dt className="text-fg-soft">{action}</dt>
                    <dd><kbd className="rounded border border-line bg-sunken px-1.5 py-0.5 font-mono text-[11px]">{key}</kbd></dd>
                  </div>
                ))}
              </dl>
            </div>
          ) : null}
        </div>
        <button
          type="button"
          className={headerButton}
          aria-label={fullscreen ? "Exit full screen" : "Full screen"}
          title={fullscreen ? "Exit full screen" : "Full screen: the whole course fills the screen"}
          onClick={() => (fullscreen ? void document.exitFullscreen?.().catch(() => undefined) : void document.documentElement.requestFullscreen?.().catch(() => undefined))}
        >
          {fullscreen ? <Minimize size={16} /> : <Maximize size={16} />}
        </button>
        <button type="button" className={headerButton} aria-label={sideOpen ? "Hide side panel" : "Show side panel"} aria-pressed={sideOpen} title="Notes and sketch (])" onClick={() => showSide(!sideOpen)}>
          <PanelRight size={17} />
        </button>
      </div>

      <div className="relative flex min-h-0 flex-1">
        {/* Contents */}
        {contentsOpen ? (
          contentsDocked ? (
            <aside aria-label="Course contents" className="w-[300px] shrink-0 border-r border-line bg-surface">{contents}</aside>
          ) : (
            <>
              <button type="button" aria-label="Close contents" className="absolute inset-0 z-20 bg-[#000]/30" onClick={() => setContentsOverlay(false)} />
              <aside aria-label="Course contents" className="absolute inset-y-0 left-0 z-30 w-[min(340px,88%)] border-r border-line bg-surface shadow-panel">{contents}</aside>
            </>
          )
        ) : null}

        {/* Stage */}
        <main aria-label="Lesson" className="flex min-w-0 flex-1 flex-col">
          {stage.kind !== "overview" && stage.kind !== "notebook" && active ? (
            <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 px-4 pb-2 pt-3">
              <div className="min-w-0 flex-1">
                <p className="truncate text-[11px] font-medium uppercase tracking-wider text-subtle">
                  {active.moduleTitle} · Lesson {active.index + 1} of {placed.length}
                </p>
                <h1 className="truncate font-display text-xl font-semibold leading-tight">{active.lesson.title}</h1>
              </div>
              {active.lesson.resources.length > 1 ? (
                <div role="group" aria-label="Lesson materials" className="flex items-center gap-0.5 rounded-lg bg-sunken p-0.5">
                  {active.lesson.resources.filter((resource) => resource.kind === "video" || resource.kind === "pdf" || isReadable(resource)).map((resource) => (
                    <button
                      key={resource.id}
                      type="button"
                      aria-pressed={showing?.resourceId === resource.id}
                      onClick={() => openResource(active.lesson, resource.id)}
                      className={`rounded-md px-2.5 py-1 text-xs font-medium ${showing?.resourceId === resource.id ? "bg-surface text-fg shadow-sm" : "text-muted hover:text-fg"}`}
                    >
                      {resourceLabel[resource.kind] ?? resource.kind}
                    </button>
                  ))}
                </div>
              ) : null}
              <div className="flex items-center gap-1">
                <MarkCompleteButton
                  lessonTitle={active.lesson.title}
                  label="Mark this lesson complete"
                  completed={progress.statusOf(active.lesson.id) === "completed"}
                  disabled={progress.mark.isPending}
                  onToggle={(completed) => progress.mark.mutate({ lessonId: active.lesson.id, completed })}
                />
                <LessonStep placed={previous} direction="previous" onOpen={openLesson} />
                <LessonStep placed={next} direction="next" onOpen={openLesson} />
              </div>
            </div>
          ) : null}

          <div className={`min-h-0 flex-1 ${stage.kind === "overview" || stage.kind === "notebook" ? "" : "px-4 pb-3"}`}>
            {stage.kind === "notebook" ? (
              <CourseNotebook
                courseId={courseId}
                placed={placed}
                onOpenLesson={openLesson}
                onSeek={seekTo}
                onOpenPage={(resourceId, page) => openDocument(resourceId, page)}
                onOpenSketch={(lesson) => openLessonWith(lesson, "sketch")}
                onEditNotes={(lesson) => openLessonWith(lesson, "notes")}
              />
            ) : stage.kind === "pdf" ? (
              <DocumentViewer
                key={stage.resourceId}
                courseId={courseId}
                resourceId={stage.resourceId}
                title={`${active?.lesson.title ?? "Lesson"} · PDF`}
                page={stage.page}
                lessonId={active?.lesson.id ?? ""}
                onPage={(page) => setStage((current) => (current.kind === "pdf" && current.page !== page ? { ...current, page, region: null } : current))}
                pendingRegion={stage.region}
                onRegion={(page, region) => {
                  setStage({ ...stage, page, region });
                  if (region) showSide(true, "notes");
                }}
                onComment={requestNote}
                onEditNote={editNote}
              />
            ) : stage.kind === "text" ? (
              <div className="relative h-full overflow-y-auto rounded-xl border border-line bg-surface">
                <Suspense fallback={<p className="p-6 text-sm text-muted">Opening {active?.lesson.title}…</p>}>
                  <TextReader key={stage.resourceId} courseId={courseId} resourceId={stage.resourceId} title={active?.lesson.title ?? "Lesson"} />
                </Suspense>
              </div>
            ) : stage.kind === "video" ? (
              <LessonPlayer
                handle={player}
                key={stage.url}
                url={stage.url}
                torrent={torrent}
                verifiedPercent={torrent ? percentOf(availabilityById.get(stage.resourceId)) : null}
                onRetry={() => openMedia.mutate(stage.resourceId)}
                startAt={stage.startAt}
                transfersPaused={(offline.downloads?.paused ?? false) || lowDisk}
                onPosition={savePosition(stage.resourceId)}
                onAddNote={(seconds) => requestNote({ kind: "videoTime", resourceId: stage.resourceId, seconds })}
                markers={(videoNotes.data ?? []).flatMap((note) =>
                  note.anchor.kind === "videoTime" && note.anchor.resourceId === stage.resourceId ? [{ seconds: note.anchor.seconds, label: note.text }] : [],
                )}
                cues={transcript.data?.cues ?? null}
                upNext={next ? { title: next.lesson.title } : null}
                onNext={() => next && openLesson(next.lesson)}
              />
            ) : (
              <CourseOverview course={course} placed={placed} progress={progress} next={upNext} onStart={(item) => openLesson(item.lesson)} />
            )}
          </div>
          {openMedia.isError ? (
            <p role="alert" className="shrink-0 px-4 pb-3 text-sm text-red-300">
              {torrent ? (asCommandError(openMedia.error)?.message ?? "This lesson could not be streamed.") : "This verified media file is no longer available."}
            </p>
          ) : openMedia.isPending ? (
            <p role="status" className="shrink-0 px-4 pb-3 text-xs text-muted">Opening the lesson…</p>
          ) : null}
        </main>

        {/* Notes, sketch, transcript */}
        {sideOpen ? (
          sideDocked ? (
            <aside aria-label="Notes and tools" style={{ width: layout.sideWidth }} className="relative shrink-0 border-l border-line">{side}</aside>
          ) : (
            <>
              <button type="button" aria-label="Close side panel" className="absolute inset-0 z-20 bg-[#000]/30" onClick={() => setSideOverlay(false)} />
              <aside aria-label="Notes and tools" className="absolute inset-y-0 right-0 z-30 w-[min(420px,92%)] border-l border-line shadow-panel">{side}</aside>
            </>
          )
        ) : null}
      </div>
    </div>
  );
}

function BackButton({ onLeave }: { onLeave: () => void }) {
  return (
    <button
      type="button"
      onClick={onLeave}
      aria-label="Back to library"
      title="Back to library"
      className="flex h-8 shrink-0 items-center gap-1 rounded-md pl-1 pr-2 text-sm text-muted hover:bg-white/10 hover:text-fg"
    >
      <ChevronLeft size={17} /> <span className="hidden sm:inline">Library</span>
    </button>
  );
}

function LessonStep({ placed, direction, onOpen }: { placed: PlacedLesson | null; direction: "previous" | "next"; onOpen: (lesson: Lesson) => void }) {
  return (
    <button
      type="button"
      aria-label={direction === "previous" ? "Previous lesson" : "Next lesson"}
      title={placed ? `${direction === "previous" ? "Previous" : "Next"}: ${placed.lesson.title}` : undefined}
      disabled={!placed}
      onClick={() => placed && onOpen(placed.lesson)}
      className="grid size-8 place-items-center rounded-md text-muted hover:bg-white/10 hover:text-fg disabled:opacity-30 disabled:hover:bg-transparent"
    >
      {direction === "previous" ? <ChevronLeft size={17} /> : <ChevronRight size={17} />}
    </button>
  );
}
