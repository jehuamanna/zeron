import { useQueryClient } from "@tanstack/react-query";
import { MonitorPlay } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi } from "../../services/library";
import { describeYouTubeError } from "../player/YouTubePlayer";

/** How long YouTube's player may take to list a playlist. */
export const LISTING_TIMEOUT_MS = 25_000;

type Step =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "listing"; playlistId: string; pageUrl: string }
  | { kind: "looking-up"; playlistId: string; done: number; total: number }
  | { kind: "added"; courseId: string; title: string; videos: number; skipped: number; playlist: boolean }
  | { kind: "failed"; message: string };

const failedWith = (error: unknown, fallback: string): Step => ({ kind: "failed", message: asCommandError(error)?.message ?? fallback });

/**
 * Adds a YouTube video or playlist from a pasted link as an online-only
 * course (ADR 0018). A playlist's videos are listed by YouTube's own player,
 * in a gateway page framed out of sight, then checked one by one in Rust.
 */
export function YouTubeAddPanel({ onOpen }: { onOpen: (courseId: string) => void }) {
  const queryClient = useQueryClient();
  const [link, setLink] = useState("");
  const [step, setStep] = useState<Step>({ kind: "idle" });
  const lister = useRef<HTMLIFrameElement>(null);
  const busy = step.kind === "checking" || step.kind === "listing" || step.kind === "looking-up";

  const added = async (next: Step) => {
    setLink("");
    setStep(next);
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["courses"] }),
      queryClient.invalidateQueries({ queryKey: ["removed-courses"] }),
    ]);
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const value = link.trim();
    if (!value || busy) return;
    setStep({ kind: "checking" });
    try {
      const kind = await libraryApi.inspectYouTubeLink(value);
      if (kind.kind === "playlist") {
        setStep({ kind: "listing", playlistId: kind.playlistId, pageUrl: kind.pageUrl });
        return;
      }
      const receipt = await libraryApi.addYouTubeVideo(value);
      await added({ kind: "added", courseId: receipt.courseId, title: receipt.title, videos: 1, skipped: 0, playlist: false });
    } catch (error) {
      setStep(failedWith(error, "The video could not be added."));
    }
  };

  // While listing, take the video IDs from the framed player, then add them.
  const listing = step.kind === "listing" ? step : null;
  useEffect(() => {
    if (!listing) return;
    const origin = new URL(listing.pageUrl).origin;
    let finished = false;
    const finish = (next: () => Promise<void> | void) => {
      if (finished) return;
      finished = true;
      void next();
    };
    const fail = (message: string) =>
      finish(() => {
        void libraryApi.cancelYouTubePlaylist().catch(() => undefined);
        setStep({ kind: "failed", message });
      });
    const listener = (event: MessageEvent) => {
      const data = event.data as { zeron?: unknown; type?: unknown; videoIds?: unknown; code?: unknown } | null;
      if (event.source !== lister.current?.contentWindow || event.origin !== origin || !data || data.zeron !== "youtube") return;
      if (data.type === "error") {
        fail(typeof data.code === "number" || typeof data.code === "string" ? describeYouTubeError(data.code) : "YouTube could not list this playlist.");
      } else if (data.type === "playlist") {
        const ids = Array.isArray(data.videoIds) ? data.videoIds.filter((id): id is string => typeof id === "string") : [];
        if (!ids.length) {
          fail("YouTube listed no videos for this playlist. It may be private, empty, or a mix that cannot be listed.");
          return;
        }
        finish(async () => {
          setStep({ kind: "looking-up", playlistId: listing.playlistId, done: 0, total: Math.min(ids.length, 200) });
          try {
            const receipt = await libraryApi.addYouTubePlaylist(listing.playlistId, ids);
            await added({ kind: "added", courseId: receipt.courseId, title: receipt.title, videos: receipt.verifiedResourceCount, skipped: receipt.skipped, playlist: true });
          } catch (error) {
            const cancelled = asCommandError(error)?.code === "cancelled";
            setStep(cancelled ? { kind: "idle" } : failedWith(error, "The playlist could not be added."));
          }
        });
      }
    };
    window.addEventListener("message", listener);
    const timer = window.setTimeout(() => fail("YouTube took too long to list this playlist. Check the internet connection, then try again."), LISTING_TIMEOUT_MS);
    return () => {
      window.removeEventListener("message", listener);
      window.clearTimeout(timer);
    };
    // Runs once per listing.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [listing?.pageUrl]);

  // Progress while each video is looked up.
  const playlistId = step.kind === "looking-up" ? step.playlistId : null;
  useEffect(() => {
    if (!playlistId) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void libraryApi
      .onYouTubePlaylistProgress((progress) => {
        if (progress.playlistId !== playlistId) return;
        setStep((current) => (current.kind === "looking-up" ? { ...current, done: progress.done, total: progress.total } : current));
      })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [playlistId]);

  const cancel = () => {
    void libraryApi.cancelYouTubePlaylist().catch(() => undefined);
    if (step.kind === "listing") setStep({ kind: "idle" });
  };

  return (
    <section aria-labelledby="youtube-add-heading" className="mb-8 rounded-3xl border border-line bg-surface p-5">
      <div className="flex items-start gap-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-accent/10 text-accent-text"><MonitorPlay size={18} /></span>
        <div className="min-w-0">
          <h2 id="youtube-add-heading" className="font-semibold">Learn from YouTube</h2>
          <p className="mt-1 text-sm text-muted">
            Paste a link to a video or a playlist. Videos play in YouTube&apos;s own player while you are online; your notes, sketches, and progress stay on this computer.
          </p>
        </div>
      </div>
      <form onSubmit={(event) => void submit(event)} className="mt-4 flex flex-col gap-2 sm:flex-row">
        <label htmlFor="youtube-link" className="sr-only">YouTube video or playlist link</label>
        <input
          id="youtube-link"
          value={link}
          onChange={(event) => {
            setLink(event.target.value);
            if (step.kind === "failed" || step.kind === "added") setStep({ kind: "idle" });
          }}
          placeholder="https://www.youtube.com/watch?v=… or /playlist?list=…"
          spellCheck={false}
          autoComplete="off"
          maxLength={2048}
          disabled={busy}
          className="min-w-0 flex-1 rounded-xl border border-line bg-sunken px-3 py-2.5 font-mono text-sm text-fg placeholder:text-faint focus:border-accent/60 focus:outline-none"
        />
        <Button type="submit" disabled={busy || !link.trim()}>
          {step.kind === "checking" ? "Asking YouTube…" : "Add"}
        </Button>
      </form>

      {step.kind === "listing" || step.kind === "looking-up" ? (
        <div role="status" aria-live="polite" className="mt-3 flex flex-wrap items-center gap-3 text-sm text-fg-soft">
          {step.kind === "listing" ? (
            <span>Asking YouTube for the playlist&apos;s videos…</span>
          ) : (
            <>
              <span>Checking videos: {step.done} of {step.total}</span>
              <span aria-hidden className="h-1.5 w-40 overflow-hidden rounded-full bg-white/10">
                <span className="block h-full rounded-full bg-accent transition-all" style={{ width: `${step.total ? (step.done / step.total) * 100 : 0}%` }} />
              </span>
            </>
          )}
          <Button variant="ghost" size="sm" onClick={cancel}>Cancel</Button>
        </div>
      ) : step.kind === "failed" ? (
        <p role="alert" className="mt-3 text-sm text-red-300">{step.message}</p>
      ) : step.kind === "added" ? (
        <p role="status" className="mt-3 flex flex-wrap items-center gap-2 text-sm text-fg-soft">
          Added “{step.title}”{step.playlist ? `: ${step.videos} ${step.videos === 1 ? "video" : "videos"}` : ""}.
          {step.skipped ? ` ${step.skipped} private, removed, or blocked ${step.skipped === 1 ? "video was" : "videos were"} left out.` : ""}
          <button type="button" className="font-medium text-accent-text underline-offset-2 hover:underline" onClick={() => onOpen(step.courseId)}>
            Open it
          </button>
        </p>
      ) : null}

      {listing ? (
        // YouTube's player lists the playlist here, out of sight.
        <iframe
          ref={lister}
          src={listing.pageUrl}
          title="YouTube playlist lister"
          aria-hidden
          tabIndex={-1}
          referrerPolicy="no-referrer"
          className="pointer-events-none fixed -left-[10000px] top-0 h-[180px] w-[320px] border-0"
        />
      ) : null}
    </section>
  );
}
