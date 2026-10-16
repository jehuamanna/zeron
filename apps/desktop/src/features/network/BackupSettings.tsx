import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Archive, RotateCcw } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi, type Backup } from "../../services/library";
import { formatSize } from "../downloads/downloads";

function describe(backup: Backup): string {
  const parts = backup.contents.map((part) => (part === "library" ? "your library, notes, and progress" : "your Publish drafts and release records"));
  return `${parts.join(" and ")} (${formatSize(backup.totalBytes)})`;
}

/** Back up the Library's and Publish's databases, and restore them on the next start. */
export function BackupSettings() {
  const queryClient = useQueryClient();
  const pending = useQuery({ queryKey: ["pending-restore"], queryFn: libraryApi.pendingRestore });
  const [confirming, setConfirming] = useState(false);
  const save = useMutation({ mutationFn: libraryApi.exportBackup });
  const restore = useMutation({
    mutationFn: libraryApi.chooseBackupToRestore,
    onSuccess: (staged) => {
      if (staged) {
        setConfirming(false);
        void queryClient.invalidateQueries({ queryKey: ["pending-restore"] });
      }
    },
  });
  const cancel = useMutation({
    mutationFn: libraryApi.cancelRestore,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["pending-restore"] }),
  });
  const restart = useMutation({ mutationFn: libraryApi.restartApp });

  return (
    <section aria-labelledby="backup-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6 text-sm">
      <h2 id="backup-heading" className="flex items-center gap-2 font-semibold"><Archive size={17} className="text-cyan-300" /> Backup</h2>
      <p className="mt-1 text-slate-400">
        A backup holds your library, notes, drawings, progress, catalogs, and Publish drafts and release records. It does not hold course videos, release files, or your keys; back up your learner key below and your signing key from Publish → Identity.
      </p>
      <div className="mt-4 flex flex-wrap gap-2">
        <Button onClick={() => save.mutate()} disabled={save.isPending}>{save.isPending ? "Saving…" : "Save backup…"}</Button>
        {!confirming ? <Button variant="ghost" onClick={() => setConfirming(true)}><RotateCcw size={15} /> Restore from backup…</Button> : null}
      </div>
      {save.isSuccess && save.data ? <p role="status" className="mt-2 text-xs text-emerald-300">Saved a backup of {describe(save.data)}.</p> : null}
      {save.isError ? <p role="alert" className="mt-2 text-xs text-red-300">{asCommandError(save.error)?.message ?? "The backup could not be saved."}</p> : null}

      {confirming ? (
        <div role="alertdialog" aria-label="Restore from a backup?" className="mt-3 space-y-2 rounded-2xl border border-amber-300/30 bg-amber-300/10 p-4 text-xs text-amber-100">
          <p>Restoring replaces your current library and Publish data with the backup's when Zeron next starts. The current data is kept in a folder named “before-restore” in Zeron's data folder.</p>
          <div className="flex gap-2">
            <Button onClick={() => restore.mutate()} disabled={restore.isPending}>{restore.isPending ? "Checking…" : "Choose backup…"}</Button>
            <Button variant="ghost" autoFocus onClick={() => { restore.reset(); setConfirming(false); }}>Cancel</Button>
          </div>
          {restore.isError ? <p role="alert" className="text-red-200">{asCommandError(restore.error)?.message ?? "The backup could not be used."}</p> : null}
        </div>
      ) : null}

      {pending.data ? (
        <div role="status" className="mt-3 space-y-2 rounded-2xl border border-cyan-300/30 bg-cyan-300/10 p-4 text-xs text-cyan-50">
          <p>A backup of {describe(pending.data)} is ready. Restart Zeron to restore it.</p>
          <div className="flex gap-2">
            <Button onClick={() => restart.mutate()} disabled={restart.isPending}>Restart now</Button>
            <Button variant="ghost" onClick={() => cancel.mutate()} disabled={cancel.isPending}>Don't restore</Button>
          </div>
        </div>
      ) : null}
    </section>
  );
}
