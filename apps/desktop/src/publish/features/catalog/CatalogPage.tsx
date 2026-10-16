import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FileDown, KeyRound, ShieldOff } from "lucide-react";
import { useState } from "react";

import { asCommandError, publisherApi, type PublisherCatalog, type Release } from "../../services/publisher";
import { ConfirmAction } from "../courses/ConfirmAction";

/** The publisher's signed catalog: which releases it lists, key revocations, and export. */
export function CatalogPage({ onOpenIdentity }: { onOpenIdentity: () => void }) {
  const queryClient = useQueryClient();
  const identity = useQuery({ queryKey: ["publisher-identity"], queryFn: publisherApi.getIdentity });
  const catalog = useQuery({ queryKey: ["publisher-catalog"], queryFn: publisherApi.getCatalog });
  const update = (next: PublisherCatalog) => queryClient.setQueryData(["publisher-catalog"], next);
  const exportCatalog = useMutation({ mutationFn: publisherApi.exportCatalog });

  if (catalog.isPending || identity.isPending) return <p className="text-slate-400">Reading the catalog…</p>;
  if (catalog.isError || identity.isError) return <p role="alert" className="text-red-300">The catalog is unavailable.</p>;
  if (identity.data.status === "none") {
    return (
      <div className="max-w-2xl rounded-3xl border border-white/10 bg-white/[0.035] p-6 text-sm">
        <p className="text-slate-300">A catalog is signed with your publisher identity, so learners can check it comes from you.</p>
        <button type="button" onClick={onOpenIdentity} className="mt-4 flex items-center gap-2 rounded-xl bg-accent hover:bg-accent-hover px-4 py-2 font-semibold text-slate-950">
          <KeyRound size={15} /> Create a signing identity
        </button>
      </div>
    );
  }
  const data = catalog.data;
  const releases = data.listings.filter((listing) => listing.release.status !== "archived");

  return (
    <div className="max-w-3xl space-y-6">
      <section aria-labelledby="catalog-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
        <h2 id="catalog-heading" className="font-semibold">Your catalog</h2>
        <p className="mt-1 text-sm text-slate-400">
          A signed list of the releases you choose, for learners to browse and import. Export it and put it anywhere: a website, a shared folder, or a message. Learners can follow an https:// address to get updates.
        </p>
        <TitleField title={data.title} onSaved={update} />
        <div className="mt-5 flex flex-wrap items-center gap-3">
          <button type="button" onClick={() => exportCatalog.mutate()} disabled={exportCatalog.isPending || identity.data.status !== "ready"} className="flex items-center gap-2 rounded-xl bg-accent hover:bg-accent-hover px-4 py-2 text-sm font-semibold text-slate-950 disabled:opacity-50">
            <FileDown size={15} /> {exportCatalog.isPending ? "Signing…" : "Export signed catalog…"}
          </button>
          {identity.data.status !== "ready" ? <p className="text-xs text-amber-200">Your signing key is not available on this computer; see Identity.</p> : null}
          {exportCatalog.isSuccess && exportCatalog.data ? <p role="status" className="text-xs text-emerald-300">Catalog saved.</p> : null}
          {exportCatalog.isError ? <p role="alert" className="text-xs text-red-300">{asCommandError(exportCatalog.error)?.message ?? "The catalog could not be exported."}</p> : null}
        </div>
      </section>

      <section aria-labelledby="catalog-releases-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6">
        <h2 id="catalog-releases-heading" className="font-semibold">Listed releases</h2>
        <p className="mt-1 text-sm text-slate-400">Nothing is listed until you confirm it. Archived releases are never listed.</p>
        {releases.length ? (
          <ul aria-label="Catalog releases" className="mt-4 space-y-2">
            {releases.map((listing) => <ListingRow key={listing.release.releaseId} release={listing.release} listed={listing.listed} onChanged={update} />)}
          </ul>
        ) : (
          <p className="mt-4 text-sm text-slate-500">Publish a release to list it.</p>
        )}
      </section>

      <Revocations catalog={data} onChanged={update} />
    </div>
  );
}

function TitleField({ title, onSaved }: { title: string; onSaved: (next: PublisherCatalog) => void }) {
  const [value, setValue] = useState(title);
  const save = useMutation({ mutationFn: () => publisherApi.setCatalogTitle(value), onSuccess: onSaved });
  return (
    <div className="mt-4 flex flex-wrap items-end gap-2">
      <label className="min-w-0 flex-1 text-xs text-slate-400">
        Title learners see
        <input value={value} onChange={(event) => setValue(event.target.value)} maxLength={200} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2 py-1.5 text-sm text-slate-100" />
      </label>
      <button type="button" onClick={() => save.mutate()} disabled={save.isPending || value.trim() === title || !value.trim()} className="rounded-lg bg-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/15 disabled:opacity-40">Save title</button>
      {save.isError ? <p role="alert" className="w-full text-xs text-red-300">{asCommandError(save.error)?.message ?? "The title could not be saved."}</p> : null}
    </div>
  );
}

function ListingRow({ release, listed, onChanged }: { release: Release; listed: boolean; onChanged: (next: PublisherCatalog) => void }) {
  const [confirming, setConfirming] = useState(false);
  const change = useMutation({
    mutationFn: (next: boolean) => publisherApi.setCatalogRelease(release.releaseId, next),
    onSuccess: (next) => {
      setConfirming(false);
      onChanged(next);
    },
  });
  return (
    <li className="rounded-2xl border border-white/10 bg-black/10 px-4 py-3 text-sm">
      <div className="flex flex-wrap items-center gap-3">
        <span className="min-w-0 flex-1">
          <span className="font-semibold">{release.title}</span> <span className="text-xs text-slate-500">version {release.courseVersion}</span>
        </span>
        {listed ? (
          <>
            <span className="rounded-full bg-emerald-300/10 px-2.5 py-0.5 text-xs text-emerald-300">Listed</span>
            <button type="button" onClick={() => change.mutate(false)} disabled={change.isPending} className="rounded-lg bg-white/10 px-2.5 py-1 text-xs hover:bg-white/15">Unlist</button>
          </>
        ) : !confirming ? (
          <button type="button" onClick={() => setConfirming(true)} className="rounded-lg bg-white/10 px-2.5 py-1 text-xs hover:bg-white/15">List in catalog…</button>
        ) : null}
      </div>
      {confirming ? (
        <div className="mt-3">
          <ConfirmAction
            title={`List ${release.title} ${release.courseVersion} in your catalog?`}
            confirmLabel="List release"
            pendingLabel="Listing…"
            pending={change.isPending}
            error={change.isError ? (asCommandError(change.error)?.message ?? "The release could not be listed.") : null}
            onConfirm={() => change.mutate(true)}
            onCancel={() => { change.reset(); setConfirming(false); }}
          >
            Everyone who gets your next exported catalog will see this release, its description, and its magnet link.
          </ConfirmAction>
        </div>
      ) : null}
      {change.isError && !confirming ? <p role="alert" className="mt-2 text-xs text-red-300">{asCommandError(change.error)?.message ?? "The change failed."}</p> : null}
    </li>
  );
}

function Revocations({ catalog, onChanged }: { catalog: PublisherCatalog; onChanged: (next: PublisherCatalog) => void }) {
  const [revoking, setRevoking] = useState<string | null>(null);
  const [reason, setReason] = useState("");
  const revoke = useMutation({
    mutationFn: (key: string) => publisherApi.revokeRetiredKey(key, reason),
    onSuccess: (next) => {
      setRevoking(null);
      setReason("");
      onChanged(next);
    },
  });
  if (!catalog.retiredKeys.length && !catalog.revocations.length) return null;
  return (
    <section aria-labelledby="revocations-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6 text-sm">
      <h2 id="revocations-heading" className="flex items-center gap-2 font-semibold"><ShieldOff size={16} className="text-slate-400" /> Retired keys</h2>
      <p className="mt-1 text-slate-400">If a retired key may have been copied, revoke it. Learners who import your catalog then refuse anything that key signed after you retired it.</p>
      <ul className="mt-3 space-y-2">
        {catalog.revocations.map((revocation) => (
          <li key={revocation.revokedKey} className="text-xs text-slate-300">
            <code className="tracking-wider">{revocation.fingerprint}</code> revoked: {revocation.reason}
          </li>
        ))}
        {catalog.retiredKeys.map((retired) => (
          <li key={retired.key} className="space-y-2 text-xs">
            <div className="flex flex-wrap items-center gap-3">
              <code className="tracking-wider text-slate-200">{retired.fingerprint}</code>
              <span className="text-slate-500">retired {new Date(retired.retiredAtUnixSeconds * 1000).toLocaleDateString()}</span>
              {revoking !== retired.key ? <button type="button" onClick={() => setRevoking(retired.key)} className="rounded-lg bg-white/10 px-2.5 py-1 hover:bg-white/15">Revoke…</button> : null}
            </div>
            {revoking === retired.key ? (
              <ConfirmAction
                title="Revoke this retired key?"
                confirmLabel="Revoke key"
                pendingLabel="Revoking…"
                pending={revoke.isPending}
                confirmDisabled={!reason.trim()}
                error={revoke.isError ? (asCommandError(revoke.error)?.message ?? "The key could not be revoked.") : null}
                onConfirm={() => revoke.mutate(retired.key)}
                onCancel={() => { revoke.reset(); setRevoking(null); setReason(""); }}
              >
                <label className="block">
                  Reason learners will see
                  <input value={reason} onChange={(event) => setReason(event.target.value)} maxLength={200} className="mt-1 w-full rounded-lg border border-white/10 bg-black/30 px-2 py-1 text-slate-100" />
                </label>
              </ConfirmAction>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}
