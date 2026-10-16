import { useMutation } from "@tanstack/react-query";
import { ArrowUpCircle } from "lucide-react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi } from "../../services/library";

/** Check for a newer Zeron only when asked, and install it after a signature check. */
export function UpdateSettings() {
  const check = useMutation({ mutationFn: libraryApi.checkForUpdate });
  const install = useMutation({ mutationFn: libraryApi.installUpdate });
  const update = check.data ?? null;
  return (
    <section aria-labelledby="updates-heading" className="rounded-3xl border border-white/10 bg-white/[0.035] p-6 text-sm">
      <h2 id="updates-heading" className="flex items-center gap-2 font-semibold"><ArrowUpCircle size={17} className="text-cyan-300" /> Updates</h2>
      <p className="mt-1 text-slate-400">Zeron contacts its release page only when you check. Every update must be signed with Zeron's release key before it is installed.</p>
      <div className="mt-4 flex flex-wrap items-center gap-2">
        <Button variant="ghost" onClick={() => check.mutate()} disabled={check.isPending || install.isPending}>{check.isPending ? "Checking…" : "Check for updates"}</Button>
        {update ? (
          <Button onClick={() => install.mutate()} disabled={install.isPending}>{install.isPending ? "Installing…" : `Install ${update.version} and restart`}</Button>
        ) : null}
      </div>
      {check.isSuccess && !update ? <p role="status" className="mt-2 text-xs text-emerald-300">Zeron is up to date.</p> : null}
      {update ? (
        <div role="status" className="mt-2 text-xs text-cyan-100">
          <p>Zeron {update.version} is available (you have {update.currentVersion}).</p>
          {update.notes ? <p className="mt-1 whitespace-pre-line text-slate-400">{update.notes}</p> : null}
        </div>
      ) : null}
      {check.isError ? <p role="alert" className="mt-2 text-xs text-red-300">{asCommandError(check.error)?.message ?? "Updates could not be checked."}</p> : null}
      {install.isError ? <p role="alert" className="mt-2 text-xs text-red-300">{asCommandError(install.error)?.message ?? "The update could not be installed."}</p> : null}
    </section>
  );
}
