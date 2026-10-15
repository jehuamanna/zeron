import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AlertCircle, ArrowLeft, BookPlus, Boxes, CheckCircle2, Network, Settings2 } from "lucide-react";
import { useCallback, useState } from "react";

import { Builder } from "../features/builder/Builder";
import { publisherApi, type DraftDetail } from "../services/publisher";
import { useBuilderUi } from "../stores/builder";

export function App() {
  const queryClient = useQueryClient();
  const { selectedDraftId, selectDraft } = useBuilderUi();
  const [reloadCount, setReloadCount] = useState(0);
  const drafts = useQuery({ queryKey: ["publisher-drafts"], queryFn: publisherApi.listDrafts });
  const draft = useQuery({
    queryKey: ["publisher-draft", selectedDraftId],
    queryFn: () => publisherApi.getDraft(selectedDraftId!),
    enabled: selectedDraftId !== null,
  });
  const validation = useQuery({
    queryKey: ["publisher-draft-validation", selectedDraftId],
    queryFn: () => publisherApi.validateDraft(selectedDraftId!),
    enabled: selectedDraftId !== null,
  });
  const createDraft = useMutation({
    mutationFn: publisherApi.createDraft,
    onSuccess: async (created) => {
      queryClient.setQueryData(["publisher-draft", created.draftId], created);
      await queryClient.invalidateQueries({ queryKey: ["publisher-drafts"] });
      selectDraft(created.draftId);
    },
  });
  const handleSaved = useCallback(
    (saved: DraftDetail) => {
      queryClient.setQueryData(["publisher-draft", saved.draftId], saved);
      void queryClient.invalidateQueries({ queryKey: ["publisher-drafts"] });
      void queryClient.invalidateQueries({ queryKey: ["publisher-draft-validation", saved.draftId] });
    },
    [queryClient],
  );
  const reloadDraft = async () => {
    await Promise.all([draft.refetch(), validation.refetch()]);
    setReloadCount((count) => count + 1);
  };

  return (
    <main className="min-h-screen bg-[#090d16] text-slate-100">
      <div className="grid min-h-screen grid-cols-[220px_minmax(0,1fr)]">
        <aside className="border-r border-white/10 bg-black/20 p-5">
          <div className="mb-10 flex items-center gap-3">
            <div className="grid size-10 place-items-center rounded-xl bg-amber-300 text-slate-950"><Boxes size={21} /></div>
            <div><p className="font-bold">Zeron</p><p className="text-xs text-slate-500">Publisher</p></div>
          </div>
          <nav className="space-y-2 text-sm">
            <div className="rounded-xl bg-white/10 px-3 py-2.5 text-white">Courses</div>
            <div className="flex items-center gap-2 px-3 py-2.5 text-slate-500"><Network size={16} /> Distribution</div>
            <div className="flex items-center gap-2 px-3 py-2.5 text-slate-500"><Settings2 size={16} /> Settings</div>
          </nav>
        </aside>

        <section className="px-8 py-7">
          <header className="flex items-center justify-between border-b border-white/10 pb-6">
            <div>
              <p className="text-xs font-bold uppercase tracking-[0.22em] text-amber-300">Admin Desktop</p>
              <h1 className="mt-1 text-2xl font-semibold">{selectedDraftId ? "Course builder" : "Publisher courses"}</h1>
            </div>
            {!selectedDraftId ? (
              <button type="button" onClick={() => createDraft.mutate()} disabled={createDraft.isPending} className="flex items-center gap-2 rounded-xl bg-amber-300 px-4 py-2.5 text-sm font-semibold text-slate-950 disabled:opacity-50">
                <BookPlus size={17} /> {createDraft.isPending ? "Creating…" : "New course"}
              </button>
            ) : null}
          </header>

          <div className="py-8">
            {selectedDraftId ? (
              <div>
                <button type="button" onClick={() => selectDraft(null)} className="mb-6 flex items-center gap-2 text-sm text-slate-400"><ArrowLeft size={16} /> Back to courses</button>
                {draft.isPending ? <p className="text-slate-400">Opening draft…</p> : draft.data ? (
                  <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_320px]">
                    <Builder key={`${draft.data.draftId}:${reloadCount}`} draft={draft.data} onSaved={handleSaved} onReload={() => void reloadDraft()} />
                    <aside className="h-fit rounded-3xl border border-white/10 bg-black/20 p-6">
                      <h2 className="font-semibold">Publishing readiness</h2>
                      {validation.isPending ? <p className="mt-5 text-sm text-slate-400">Checking draft…</p> : validation.isError ? (
                        <p role="alert" className="mt-5 text-sm text-red-300">Validation is unavailable.</p>
                      ) : validation.data?.length ? (
                        <ul aria-label="Blocking issues" className="mt-5 space-y-3 text-sm">
                          {validation.data.map((issue) => (
                            <li key={`${issue.field}:${issue.code}`} className="flex gap-3 text-slate-300">
                              <AlertCircle className="mt-0.5 shrink-0 text-amber-300" size={16} />
                              <span><span className="block font-mono text-xs text-slate-500">{issue.field}</span>{issue.message}</span>
                            </li>
                          ))}
                        </ul>
                      ) : (
                        <div className="mt-5 flex gap-3 text-sm text-slate-400"><CheckCircle2 className="shrink-0 text-emerald-300" size={18} /><span>No blocking draft issues. Packaging checks run when publishing.</span></div>
                      )}
                    </aside>
                  </div>
                ) : <p role="alert" className="text-red-300">This draft could not be recovered.</p>}
              </div>
            ) : drafts.isPending ? <p className="text-slate-400">Loading publisher drafts…</p> : drafts.isError || createDraft.isError ? (
              <p role="alert" className="rounded-2xl border border-red-400/30 bg-red-400/10 p-5 text-red-200">Publisher storage is unavailable.</p>
            ) : drafts.data?.length ? (
              <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">{drafts.data.map((item) => (
                <button type="button" key={item.draftId} onClick={() => selectDraft(item.draftId)} className="rounded-2xl border border-white/10 bg-white/[0.035] p-5 text-left hover:border-amber-300/40">
                  <p className="text-xs text-amber-300">DRAFT · r{item.revision}</p><h2 className="mt-4 text-lg font-semibold">{item.title}</h2><p className="mt-1 text-sm text-slate-500">Version {item.courseVersion}</p>
                </button>
              ))}</div>
            ) : (
              <div className="rounded-3xl border border-dashed border-white/15 py-20 text-center"><BookPlus className="mx-auto mb-4 text-amber-300" size={30} /><h2 className="text-xl font-semibold">Create your first course</h2><p className="mt-2 text-slate-400">Drafts stay editable and separate from immutable published releases.</p></div>
            )}
          </div>
        </section>
      </div>
    </main>
  );
}
