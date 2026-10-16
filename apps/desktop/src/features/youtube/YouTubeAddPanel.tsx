import { useMutation, useQueryClient } from "@tanstack/react-query";
import { MonitorPlay } from "lucide-react";
import { useState, type FormEvent } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi } from "../../services/library";

/** Adds a YouTube video from a pasted link as an online-only course (ADR 0018). */
export function YouTubeAddPanel({ onOpen }: { onOpen: (courseId: string) => void }) {
  const queryClient = useQueryClient();
  const [link, setLink] = useState("");
  const add = useMutation({
    mutationFn: (value: string) => libraryApi.addYouTubeVideo(value),
    onSuccess: async () => {
      setLink("");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["courses"] }),
        queryClient.invalidateQueries({ queryKey: ["removed-courses"] }),
      ]);
    },
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (link.trim() && !add.isPending) add.mutate(link.trim());
  };

  return (
    <section aria-labelledby="youtube-add-heading" className="mb-8 rounded-3xl border border-line bg-surface p-5">
      <div className="flex items-start gap-3">
        <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-accent/10 text-accent-text"><MonitorPlay size={18} /></span>
        <div className="min-w-0">
          <h2 id="youtube-add-heading" className="font-semibold">Learn from a YouTube video</h2>
          <p className="mt-1 text-sm text-muted">
            Paste a link to a video. It plays in YouTube&apos;s own player while you are online; your notes, sketches, and progress stay on this computer.
          </p>
        </div>
      </div>
      <form onSubmit={submit} className="mt-4 flex flex-col gap-2 sm:flex-row">
        <label htmlFor="youtube-link" className="sr-only">YouTube video link</label>
        <input
          id="youtube-link"
          value={link}
          onChange={(event) => {
            setLink(event.target.value);
            if (add.isError || add.isSuccess) add.reset();
          }}
          placeholder="https://www.youtube.com/watch?v=…"
          spellCheck={false}
          autoComplete="off"
          maxLength={2048}
          className="min-w-0 flex-1 rounded-xl border border-line bg-sunken px-3 py-2.5 font-mono text-sm text-fg placeholder:text-faint focus:border-accent/60 focus:outline-none"
        />
        <Button type="submit" disabled={add.isPending || !link.trim()}>
          {add.isPending ? "Asking YouTube…" : "Add video"}
        </Button>
      </form>
      {add.isError ? (
        <p role="alert" className="mt-3 text-sm text-red-300">
          {asCommandError(add.error)?.message ?? "The video could not be added."}
        </p>
      ) : add.isSuccess ? (
        <p role="status" className="mt-3 flex flex-wrap items-center gap-2 text-sm text-fg-soft">
          Added “{add.data.title}”.
          <button type="button" className="font-medium text-accent-text underline-offset-2 hover:underline" onClick={() => onOpen(add.data.courseId)}>
            Open it
          </button>
        </p>
      ) : null}
    </section>
  );
}
