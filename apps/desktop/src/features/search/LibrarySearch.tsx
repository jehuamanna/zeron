import { useQuery } from "@tanstack/react-query";
import { FileText, GraduationCap, NotebookPen, Captions, Search, BookOpen } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";

import { asCommandError, libraryApi, type SearchResult } from "../../services/library";

/** How long typing pauses before searching. */
export const SEARCH_DELAY_MS = 200;

const KIND_LABEL: Record<SearchResult["kind"], string> = {
  course: "Course",
  lesson: "Lesson",
  note: "Your note",
  transcript: "Transcript",
  text: "Lesson text",
};

const KIND_ICON: Record<SearchResult["kind"], ReactNode> = {
  course: <GraduationCap size={14} />,
  lesson: <BookOpen size={14} />,
  note: <NotebookPen size={14} />,
  transcript: <Captions size={14} />,
  text: <FileText size={14} />,
};

/** A snippet with its matched terms (between U+E000 and U+E001) emphasised. */
export function Snippet({ text }: { text: string }) {
  const parts = text.split(/([])/);
  let inMatch = false;
  return (
    <>
      {parts.map((part, index) => {
        if (part === "") {
          inMatch = true;
          return null;
        }
        if (part === "") {
          inMatch = false;
          return null;
        }
        return inMatch ? (
          <mark key={index} className="rounded bg-cyan-300/25 px-0.5 text-cyan-50">{part}</mark>
        ) : (
          <span key={index}>{part}</span>
        );
      })}
    </>
  );
}

function clock(seconds: number) {
  const whole = Math.floor(seconds);
  const hours = Math.floor(whole / 3600);
  const minutes = String(Math.floor((whole % 3600) / 60));
  const rest = String(whole % 60).padStart(2, "0");
  return hours > 0 ? `${hours}:${minutes.padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
}

/** Offline search over every course, lesson, note, transcript, and lesson text. */
export function LibrarySearch({ onOpen }: { onOpen: (result: SearchResult) => void }) {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  useEffect(() => {
    const timer = setTimeout(() => setQuery(input.trim()), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [input]);
  const results = useQuery({
    queryKey: ["search", query],
    queryFn: () => libraryApi.search(query),
    enabled: query.length > 0,
    staleTime: 0,
  });

  return (
    <div className="relative w-full max-w-md">
      <label className="flex items-center gap-2 rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-slate-300 focus-within:border-cyan-300/50">
        <Search size={16} />
        <input
          type="search"
          aria-label="Search your courses"
          placeholder="Search courses, notes, transcripts…"
          value={input}
          onChange={(event) => setInput(event.target.value)}
          className="w-full bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-500"
        />
      </label>
      {query ? (
        <div role="region" aria-label="Search results" className="absolute right-0 z-20 mt-2 max-h-[60vh] w-full overflow-y-auto rounded-2xl border border-white/10 bg-surface p-2 shadow-panel">
          {results.isError ? (
            <p role="alert" className="p-3 text-sm text-red-300">{asCommandError(results.error)?.message ?? "Search failed."}</p>
          ) : results.isPending ? (
            <p className="p-3 text-sm text-slate-400">Searching…</p>
          ) : results.data.length === 0 ? (
            <p className="p-3 text-sm text-slate-400">Nothing matches “{query}”.</p>
          ) : (
            <ul aria-label="Matches" className="space-y-1">
              {results.data.map((result, index) => (
                <li key={`${result.kind}-${result.sourceId}-${index}`}>
                  <button
                    type="button"
                    onClick={() => {
                      setInput("");
                      setQuery("");
                      onOpen(result);
                    }}
                    className="w-full rounded-xl px-3 py-2 text-left hover:bg-white/5"
                  >
                    <span className="flex items-center gap-2 text-xs text-slate-400">
                      {KIND_ICON[result.kind]} {KIND_LABEL[result.kind]}
                      {result.position !== null ? ` at ${clock(result.position)}` : ""} · {result.courseTitle}
                      {result.lessonTitle ? ` · ${result.lessonTitle}` : ""}
                    </span>
                    <span className="mt-1 line-clamp-2 block text-sm text-slate-200">
                      <Snippet text={result.snippet} />
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}
