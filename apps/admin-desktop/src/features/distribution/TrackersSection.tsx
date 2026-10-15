import { useMutation } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";

import { asCommandError, publisherApi, type Distribution } from "../../services/publisher";
import { TrackerListsForm, TrackersForm } from "./TrackersForm";

function fetchedLabel(fetchedAt: number | null, now: number): string {
  if (fetchedAt === null) return "not downloaded yet";
  const minutes = Math.max(0, Math.floor((now - fetchedAt) / 60));
  if (minutes < 1) return "downloaded just now";
  if (minutes < 60) return `downloaded ${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  return hours < 48 ? `downloaded ${hours} h ago` : `downloaded ${Math.floor(hours / 24)} days ago`;
}

export function TrackersSection({ data, onChanged }: { data: Distribution; onChanged: () => void }) {
  const refresh = useMutation({ mutationFn: publisherApi.refreshTrackerLists, onSettled: onChanged });
  const saveLists = useMutation({
    mutationFn: (urls: string[]) => publisherApi.setTrackerLists(urls),
    onSuccess: () => refresh.mutate(),
    onSettled: onChanged,
  });
  const saveTrackers = useMutation({ mutationFn: (trackers: string[]) => publisherApi.setTrackers(trackers), onSettled: onChanged });
  const failures = new Map((refresh.data ?? []).filter((result) => result.error).map((result) => [result.url, result.error!]));
  const message = (error: unknown, fallback: string) => asCommandError(error)?.message ?? fallback;
  const now = Date.now() / 1000;

  return (
    <section aria-labelledby="trackers-heading" className="mt-6 border-t border-white/10 pt-5">
      <h3 id="trackers-heading" className="text-sm font-semibold">Trackers</h3>
      <p className="mt-1 text-xs text-slate-400">
        Written into releases you publish from now on; published releases keep theirs. Learners find your seeder faster through trackers than through the DHT alone.
        Only the Internet profile announces to trackers.
      </p>
      <p className="mt-1 text-xs text-amber-200/80">
        A public tracker shows the IP address of everyone downloading a release to anyone who has its magnet link.
      </p>

      <TrackerListsForm
        key={data.trackerLists.map((list) => list.url).join("\n")}
        urls={data.trackerLists.map((list) => list.url)}
        saving={saveLists.isPending}
        error={saveLists.isError ? message(saveLists.error, "The tracker lists could not be saved.") : null}
        onSave={(urls) => saveLists.mutate(urls)}
      />
      {data.trackerLists.length ? (
        <ul aria-label="Tracker list status" className="mt-3 space-y-1 text-xs">
          {data.trackerLists.map((list) => (
            <li key={list.url} className="flex flex-wrap gap-x-2">
              <span className="truncate font-mono text-slate-400">{list.url}</span>
              <span className="text-slate-500">
                {list.trackerCount} {list.trackerCount === 1 ? "tracker" : "trackers"} · {fetchedLabel(list.fetchedAtUnixSeconds, now)}
              </span>
              {failures.has(list.url) ? <span role="alert" className="text-red-300">Refresh failed: {failures.get(list.url)}</span> : null}
            </li>
          ))}
        </ul>
      ) : null}
      <button
        type="button"
        onClick={() => refresh.mutate()}
        disabled={refresh.isPending || !data.trackerLists.length}
        className="mt-3 flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-40"
      >
        {refresh.isPending ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />} Refresh lists
      </button>
      {refresh.isError ? <p role="alert" className="mt-1 text-xs text-red-300">{message(refresh.error, "The lists could not be refreshed.")}</p> : null}

      <TrackersForm
        key={data.trackers.join("\n")}
        trackers={data.trackers}
        saving={saveTrackers.isPending}
        error={saveTrackers.isError ? message(saveTrackers.error, "The trackers could not be saved.") : null}
        onSave={(trackers) => saveTrackers.mutate(trackers)}
      />
      <p className="mt-3 text-xs text-slate-300">
        {data.effectiveTrackerCount
          ? `New releases will carry ${data.effectiveTrackerCount} ${data.effectiveTrackerCount === 1 ? "tracker" : "trackers"}: yours first, then the lists.`
          : "New releases will carry no trackers."}
      </p>
    </section>
  );
}
