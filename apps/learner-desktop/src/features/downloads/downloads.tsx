import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { CheckCircle2, CloudDownload, HardDriveDownload, Pause, Play } from "lucide-react";

import { asCommandError, libraryApi, type AvailabilityState, type CourseDownloads, type DownloadScope, type ResourceAvailability } from "../../services/library";

const labels: Record<AvailabilityState, string> = {
  online_only: "Online only",
  downloading: "Downloading for offline",
  partially_available: "Partly on this computer",
  available_offline: "Available offline",
};

export function percentOf(item: ResourceAvailability | undefined): number {
  if (!item) return 0;
  return item.byteLength > 0 ? Math.floor((item.verifiedBytes / item.byteLength) * 100) : 100;
}

/** Availability of one lesson resource. */
export function AvailabilityLabel({ item }: { item: ResourceAvailability | undefined }) {
  const state = item?.state ?? "online_only";
  return (
    <span data-availability={state} className={`inline-flex items-center gap-1 text-[11px] ${state === "available_offline" ? "text-emerald-300" : state === "downloading" ? "text-cyan-200" : "text-slate-400"}`}>
      {state === "available_offline" ? <CheckCircle2 size={12} /> : null}
      {labels[state]}
    </span>
  );
}

export function useCourseDownloads(courseId: string, enabled: boolean) {
  const queryClient = useQueryClient();
  const downloads = useQuery({ queryKey: ["downloads", courseId], queryFn: () => libraryApi.downloads(courseId), enabled });
  const change = useMutation({
    mutationFn: ({ scope, keep }: { scope: DownloadScope; keep: boolean }) => libraryApi.setDownload(courseId, scope, keep),
    onSuccess: (updated) => {
      queryClient.setQueryData<CourseDownloads>(["downloads", courseId], updated);
      void queryClient.invalidateQueries({ queryKey: ["availability", courseId] });
    },
  });
  const pause = useMutation({
    mutationFn: (paused: boolean) => libraryApi.setPaused(courseId, paused),
    onSuccess: (updated) => {
      queryClient.setQueryData<CourseDownloads>(["downloads", courseId], updated);
      void queryClient.invalidateQueries({ queryKey: ["availability", courseId] });
    },
  });
  const failed = change.isError ? change.error : pause.isError ? pause.error : null;
  const error = failed ? (asCommandError(failed)?.message ?? "The download choice could not be saved.") : null;
  return { downloads: downloads.data, change, pause, error };
}

/** A pressed-state toggle for keeping a course, module, or lesson offline. */
export function KeepOfflineToggle({ label, kept, inherited = false, disabled, onToggle }: {
  label: string;
  kept: boolean;
  /** Kept because a larger scope is; the toggle is then locked on. */
  inherited?: boolean;
  disabled: boolean;
  onToggle: (keep: boolean) => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={kept || inherited}
      aria-label={label}
      title={inherited ? "Kept offline with the whole module or course" : kept ? "Stop keeping offline. What is downloaded stays on this computer." : "Download in full and keep offline"}
      disabled={disabled || inherited}
      onClick={() => onToggle(!kept)}
      className={`inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs disabled:opacity-60 ${kept || inherited ? "bg-emerald-300/15 text-emerald-200" : "bg-white/5 text-slate-300 hover:bg-white/10"}`}
    >
      {kept || inherited ? <HardDriveDownload size={12} /> : <CloudDownload size={12} />}
      {kept || inherited ? "Kept offline" : "Keep offline"}
    </button>
  );
}

/** Pause or resume all downloading and sharing for one course. */
export function PauseTransfersButton({ paused, disabled, onToggle }: { paused: boolean; disabled: boolean; onToggle: (paused: boolean) => void }) {
  return (
    <button
      type="button"
      aria-pressed={paused}
      disabled={disabled}
      onClick={() => onToggle(!paused)}
      className="inline-flex items-center gap-1 rounded-md bg-white/5 px-2 py-1 text-xs text-slate-300 hover:bg-white/10 disabled:opacity-60"
    >
      {paused ? <Play size={12} /> : <Pause size={12} />}
      {paused ? "Resume downloads" : "Pause downloads"}
    </button>
  );
}
