import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FileDown, FileUp, KeyRound, RefreshCw, ShieldCheck, ShieldAlert } from "lucide-react";
import { useState } from "react";

import { asCommandError, publisherApi, type PublisherIdentity } from "../../services/publisher";
import { ConfirmAction } from "../courses/ConfirmAction";

const MIN_PASSPHRASE = 12;

/** The publisher's signing identity: create it, back it up, restore it, and rotate its key. */
export function IdentityPage() {
  const queryClient = useQueryClient();
  const identity = useQuery({ queryKey: ["publisher-identity"], queryFn: publisherApi.getIdentity });
  const update = (next: PublisherIdentity | null | undefined) => {
    if (next) queryClient.setQueryData(["publisher-identity"], next);
  };
  const create = useMutation({ mutationFn: publisherApi.createIdentity, onSuccess: update });

  if (identity.isPending) return <p className="text-slate-400">Reading the publisher identity…</p>;
  if (identity.isError) return <p role="alert" className="text-red-300">The publisher identity is unavailable.</p>;
  const data = identity.data;

  return (
    <div className="max-w-3xl space-y-6">
      <section aria-labelledby="identity-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
        <h2 id="identity-heading" className="flex items-center gap-2 font-semibold"><KeyRound size={17} className="text-amber-300" /> Signing identity</h2>
        <p className="mt-1 text-sm text-slate-400">
          Releases signed with your key let learners check they come from you. The Library refuses a newer version of your course signed by anyone else. The key stays in this computer's keychain.
        </p>
        {data.status === "none" ? (
          <div className="mt-5 space-y-3">
            <button type="button" onClick={() => create.mutate()} disabled={create.isPending} className="flex items-center gap-2 rounded-xl bg-amber-300 px-4 py-2 text-sm font-semibold text-slate-950 disabled:opacity-50">
              <KeyRound size={15} /> {create.isPending ? "Creating…" : "Create signing identity"}
            </button>
            {create.isError ? <p role="alert" className="text-sm text-red-300">{asCommandError(create.error)?.message ?? "The identity could not be created."}</p> : null}
            <RestoreBackup onRestored={update} />
          </div>
        ) : (
          <IdentityDetails identity={data} onChanged={update} />
        )}
      </section>
    </div>
  );
}

function IdentityDetails({ identity, onChanged }: { identity: PublisherIdentity; onChanged: (next: PublisherIdentity | null) => void }) {
  const [rotating, setRotating] = useState(false);
  const toggle = useMutation({ mutationFn: (enabled: boolean) => publisherApi.setSignReleases(enabled), onSuccess: onChanged });
  const rotate = useMutation({
    mutationFn: publisherApi.rotateIdentity,
    onSuccess: (next) => {
      setRotating(false);
      onChanged(next);
    },
  });
  const ready = identity.status === "ready";
  return (
    <div className="mt-5 space-y-5 text-sm">
      <div className={`flex gap-2 rounded-2xl border p-4 ${ready ? "border-emerald-300/20 bg-emerald-300/5 text-emerald-100" : "border-amber-300/30 bg-amber-300/10 text-amber-100"}`}>
        {ready ? <ShieldCheck size={18} className="mt-0.5 shrink-0" /> : <ShieldAlert size={18} className="mt-0.5 shrink-0" />}
        <div>
          <p className="font-semibold">{ready ? "Ready to sign" : identity.status === "keychain_unavailable" ? "The keychain is unavailable" : "This computer has no key for this identity"}</p>
          {identity.problem ? <p className="mt-1 text-xs opacity-90">{identity.problem}</p> : null}
          {identity.status === "key_missing" ? <p className="mt-1 text-xs opacity-90">Restore your key from a backup to sign again.</p> : null}
        </div>
      </div>

      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-2 text-xs">
        <dt className="text-slate-500">Fingerprint</dt>
        <dd><code className="text-base tracking-wider text-slate-100">{identity.fingerprint}</code></dd>
        <dt className="text-slate-500">Publisher ID</dt>
        <dd><code className="break-all text-slate-300">{identity.publisherId}</code></dd>
        <dt className="text-slate-500">Created</dt>
        <dd className="text-slate-300">{identity.createdAtUnixSeconds ? new Date(identity.createdAtUnixSeconds * 1000).toLocaleString() : ""}</dd>
        <dt className="text-slate-500">Key rotations</dt>
        <dd className="text-slate-300">{identity.rotations}</dd>
      </dl>
      <p className="text-xs text-slate-400">Learners can compare the fingerprint with what their Library shows, for example by message or phone, to be sure a course really comes from you.</p>

      <label className="flex items-center gap-2 text-slate-200">
        <input type="checkbox" checked={identity.signReleases} disabled={toggle.isPending} onChange={(event) => toggle.mutate(event.target.checked)} />
        Sign new releases
      </label>
      {!identity.signReleases ? <p className="text-xs text-amber-200">New releases will be unsigned, and the Library will refuse them as updates of courses you signed before.</p> : null}

      <ExportBackup disabled={!ready} />
      <RestoreBackup onRestored={onChanged} />

      <div>
        {rotating ? (
          <ConfirmAction
            title="Rotate the signing key?"
            confirmLabel="Rotate key"
            pendingLabel="Rotating…"
            pending={rotate.isPending}
            error={rotate.isError ? (asCommandError(rotate.error)?.message ?? "The key could not be rotated.") : null}
            onConfirm={() => rotate.mutate()}
            onCancel={() => { rotate.reset(); setRotating(false); }}
          >
            A new key signs from now on, and your old key signs the handover so learners keep trusting you. The old key is removed from this keychain. Make a new backup afterwards: older backups hold the retired key.
          </ConfirmAction>
        ) : (
          <button type="button" onClick={() => setRotating(true)} disabled={!ready} className="flex items-center gap-2 rounded-xl border border-white/10 px-3 py-2 text-xs hover:border-amber-300/40 disabled:opacity-40">
            <RefreshCw size={14} /> Rotate key…
          </button>
        )}
      </div>
    </div>
  );
}

function ExportBackup({ disabled }: { disabled: boolean }) {
  const [passphrase, setPassphrase] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const exportBackup = useMutation({
    mutationFn: () => publisherApi.exportIdentityBackup(passphrase),
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
      <p className="text-xs text-slate-400">Without the keychain entry or a backup, nobody can sign updates of your courses again. The backup is encrypted with this passphrase; Zeron cannot recover it.</p>
      <div className="flex flex-wrap gap-2">
        <input type="password" aria-label="Backup passphrase" placeholder="Passphrase" autoComplete="new-password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs" />
        <input type="password" aria-label="Repeat backup passphrase" placeholder="Repeat passphrase" autoComplete="new-password" value={confirmation} onChange={(event) => setConfirmation(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs" />
        <button type="button" onClick={() => exportBackup.mutate()} disabled={disabled || !long || !matches || exportBackup.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1 text-xs font-semibold hover:bg-white/15 disabled:opacity-40">
          <FileDown size={14} /> {exportBackup.isPending ? "Encrypting…" : "Save backup…"}
        </button>
      </div>
      {passphrase && !long ? <p className="text-xs text-amber-200">Use at least {MIN_PASSPHRASE} characters.</p> : null}
      {long && confirmation && !matches ? <p className="text-xs text-amber-200">The passphrases differ.</p> : null}
      {exportBackup.isSuccess && exportBackup.data ? <p role="status" className="text-xs text-emerald-300">Backup saved. Keep it and the passphrase somewhere safe.</p> : null}
      {exportBackup.isError ? <p role="alert" className="text-xs text-red-300">{asCommandError(exportBackup.error)?.message ?? "The backup could not be saved."}</p> : null}
    </fieldset>
  );
}

function RestoreBackup({ onRestored }: { onRestored: (next: PublisherIdentity | null) => void }) {
  const [passphrase, setPassphrase] = useState("");
  const restore = useMutation({
    mutationFn: () => publisherApi.restoreIdentityBackup(passphrase),
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
        <input type="password" aria-label="Restore passphrase" placeholder="Passphrase of the backup" autoComplete="current-password" value={passphrase} onChange={(event) => setPassphrase(event.target.value)} className="min-w-0 flex-1 rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-xs" />
        <button type="button" onClick={() => restore.mutate()} disabled={!passphrase || restore.isPending} className="flex items-center gap-1.5 rounded-lg bg-white/10 px-3 py-1 text-xs font-semibold hover:bg-white/15 disabled:opacity-40">
          <FileUp size={14} /> {restore.isPending ? "Restoring…" : "Choose backup…"}
        </button>
      </div>
      {restore.isSuccess && restore.data ? <p role="status" className="text-xs text-emerald-300">Key restored into this computer's keychain.</p> : null}
      {restore.isError ? <p role="alert" className="text-xs text-red-300">{asCommandError(restore.error)?.message ?? "The backup could not be restored."}</p> : null}
    </fieldset>
  );
}
