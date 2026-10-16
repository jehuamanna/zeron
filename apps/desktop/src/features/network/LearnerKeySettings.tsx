import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, FileDown, FileUp, KeyRound, ShieldAlert, ShieldCheck } from "lucide-react";
import { useRef, useState } from "react";

import { copyText } from "../../publish/lib/clipboard";
import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi, type LearnerKey } from "../../services/library";

const MIN_PASSPHRASE = 12;

/** This computer's learner key: create it, back it up, and restore it. No account, just a key. */
export function LearnerKeySettings() {
  const queryClient = useQueryClient();
  const key = useQuery({ queryKey: ["learner-key"], queryFn: libraryApi.getLearnerKey });
  const update = (next: LearnerKey | null | undefined) => {
    if (next) queryClient.setQueryData(["learner-key"], next);
  };
  const create = useMutation({ mutationFn: libraryApi.createLearnerKey, onSuccess: update });
  const data = key.data;

  return (
    <section aria-labelledby="learner-key-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6 text-sm">
      <h2 id="learner-key-heading" className="flex items-center gap-2 font-semibold"><KeyRound size={17} className="text-amber-300" /> Learner key</h2>
      <p className="mt-1 text-slate-400">
        Paid lessons will unlock with licenses made for this key. It stays in this computer's keychain; there is no account or password.
      </p>
      {key.isPending ? <p className="mt-4 text-slate-400">Reading the learner key…</p> : null}
      {key.isError ? <p role="alert" className="mt-4 text-red-300">The learner key is unavailable.</p> : null}
      {data?.status === "none" ? (
        <div className="mt-4 space-y-3">
          <Button onClick={() => create.mutate()} disabled={create.isPending}><KeyRound size={15} /> {create.isPending ? "Creating…" : "Create learner key"}</Button>
          {create.isError ? <p role="alert" className="text-xs text-red-300">{asCommandError(create.error)?.message ?? "The learner key could not be created."}</p> : null}
          <RestoreKey onRestored={update} />
        </div>
      ) : data ? (
        <KeyDetails learnerKey={data} onChanged={update} />
      ) : null}
    </section>
  );
}

function KeyDetails({ learnerKey, onChanged }: { learnerKey: LearnerKey; onChanged: (next: LearnerKey | null) => void }) {
  const ready = learnerKey.status === "ready";
  return (
    <div className="mt-4 space-y-4">
      <div className={`flex gap-2 rounded-2xl border p-4 ${ready ? "border-emerald-300/20 bg-emerald-300/5 text-emerald-100" : "border-amber-300/30 bg-amber-300/10 text-amber-100"}`}>
        {ready ? <ShieldCheck size={18} className="mt-0.5 shrink-0" /> : <ShieldAlert size={18} className="mt-0.5 shrink-0" />}
        <div>
          <p className="font-semibold">
            {ready ? "Ready" : learnerKey.status === "keychain_unavailable" ? "The keychain is unavailable" : "This computer's keychain no longer has this key"}
          </p>
          {learnerKey.problem ? <p className="mt-1 text-xs opacity-90">{learnerKey.problem}</p> : null}
          {learnerKey.status === "key_missing" ? <p className="mt-1 text-xs opacity-90">Restore it from a backup to keep the licenses made for it.</p> : null}
        </div>
      </div>
      {learnerKey.fingerprint ? (
        <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-2 text-xs">
          <dt className="text-slate-500">Fingerprint</dt>
          <dd><code className="text-base tracking-wider text-slate-100">{learnerKey.fingerprint}</code></dd>
          <dt className="text-slate-500">Created</dt>
          <dd className="text-slate-300">{learnerKey.createdAtUnixSeconds ? new Date(learnerKey.createdAtUnixSeconds * 1000).toLocaleString() : ""}</dd>
        </dl>
      ) : null}
      {learnerKey.learnerId ? <LearnerId value={learnerKey.learnerId} /> : null}
      <BackupKey disabled={!ready} />
      <RestoreKey onRestored={onChanged} />
    </div>
  );
}

/** The learner ID to send a publisher, who makes a license for it. */
function LearnerId({ value }: { value: string }) {
  const field = useRef<HTMLInputElement>(null);
  const [copy, setCopy] = useState<"idle" | "copied" | "manual">("idle");
  return (
    <div className="space-y-1">
      <label htmlFor="learner-id" className="text-xs font-semibold text-slate-400">Learner ID</label>
      <div className="flex gap-2">
        <input id="learner-id" ref={field} readOnly value={value} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1 font-mono text-xs text-slate-300" />
        <button type="button" onClick={async () => setCopy((await copyText(value, field.current)) ? "copied" : "manual")} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1 text-xs font-semibold hover:bg-white/15">
          {copy === "copied" ? <Check size={14} /> : <Copy size={14} />} {copy === "copied" ? "Copied" : "Copy"}
        </button>
      </div>
      <p className="text-xs text-slate-400">{copy === "manual" ? "Press Ctrl+C to copy the selected ID. " : ""}Send it to a publisher to be licensed. It holds only public keys.</p>
    </div>
  );
}

function BackupKey({ disabled }: { disabled: boolean }) {
  const [passphrase, setPassphrase] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const backup = useMutation({
    mutationFn: () => libraryApi.exportLearnerKeyBackup(passphrase),
    onSuccess: (saved) => {
      if (saved) {
        setPassphrase("");
        setConfirmation("");
      }
    },
  });
  const long = [...passphrase].length >= MIN_PASSPHRASE;
  const matches = passphrase === confirmation;
  return (
    <fieldset className="space-y-2 rounded-2xl border border-white/10 p-4">
      <legend className="px-1 text-xs font-semibold text-slate-300">Back up the key</legend>
      <p className="text-xs text-slate-400">Optional. A backup lets you move this key to another computer without signing in anywhere. It is encrypted with this passphrase; Zeron cannot recover it.</p>
      <div className="flex flex-wrap gap-2">
        <input type="password" aria-label="Learner key backup passphrase" placeholder="Passphrase" autoComplete="new-password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs" />
        <input type="password" aria-label="Repeat learner key backup passphrase" placeholder="Repeat passphrase" autoComplete="new-password" value={confirmation} onChange={(event) => setConfirmation(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs" />
        <button type="button" onClick={() => backup.mutate()} disabled={disabled || !long || !matches || backup.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1 text-xs font-semibold hover:bg-white/15 disabled:opacity-40">
          <FileDown size={14} /> {backup.isPending ? "Encrypting…" : "Save backup…"}
        </button>
      </div>
      {passphrase && !long ? <p className="text-xs text-amber-200">Use at least {MIN_PASSPHRASE} characters.</p> : null}
      {long && confirmation && !matches ? <p className="text-xs text-amber-200">The passphrases differ.</p> : null}
      {backup.isSuccess && backup.data ? <p role="status" className="text-xs text-emerald-300">Backup saved. Keep it and the passphrase somewhere safe.</p> : null}
      {backup.isError ? <p role="alert" className="text-xs text-red-300">{asCommandError(backup.error)?.message ?? "The backup could not be saved."}</p> : null}
    </fieldset>
  );
}

function RestoreKey({ onRestored }: { onRestored: (next: LearnerKey | null) => void }) {
  const [passphrase, setPassphrase] = useState("");
  const restore = useMutation({
    mutationFn: () => libraryApi.restoreLearnerKeyBackup(passphrase),
    onSuccess: (restored) => {
      if (restored) {
        setPassphrase("");
        onRestored(restored);
      }
    },
  });
  return (
    <fieldset className="space-y-2 rounded-2xl border border-white/10 p-4">
      <legend className="px-1 text-xs font-semibold text-slate-300">Restore from a backup</legend>
      <div className="flex flex-wrap gap-2">
        <input type="password" aria-label="Learner key restore passphrase" placeholder="Passphrase of the backup" autoComplete="current-password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs" />
        <button type="button" onClick={() => restore.mutate()} disabled={!passphrase || restore.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1 text-xs font-semibold hover:bg-white/15 disabled:opacity-40">
          <FileUp size={14} /> {restore.isPending ? "Restoring…" : "Choose backup…"}
        </button>
      </div>
      {restore.isSuccess && restore.data ? <p role="status" className="text-xs text-emerald-300">Key restored into this computer's keychain.</p> : null}
      {restore.isError ? <p role="alert" className="text-xs text-red-300">{asCommandError(restore.error)?.message ?? "The backup could not be restored."}</p> : null}
    </fieldset>
  );
}
