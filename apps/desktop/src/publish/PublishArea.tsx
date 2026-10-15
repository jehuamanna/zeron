import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, BookPlus, Radio } from "lucide-react";
import { useCallback, useState } from "react";

import { Builder } from "./features/builder/Builder";
import { ReadinessPanel } from "./features/builder/ReadinessPanel";
import { PublishPanel } from "./features/publish/PublishPanel";
import { CourseList } from "./features/courses/CourseList";
import { DistributionPage } from "./features/distribution/DistributionPage";
import { publisherApi, type DraftDetail } from "./services/publisher";
import { useBuilderUi, type Workspace } from "./stores/builder";

const tabs: Array<{ value: Workspace; label: string; icon: typeof BookPlus }> = [
  { value: "courses", label: "Courses", icon: BookPlus },
  { value: "distribution", label: "Distribution", icon: Radio },
];

/** Publish: build courses, publish releases, and seed them. */
export function PublishArea({ onOpenNetwork }: { onOpenNetwork: () => void }) {
  const queryClient = useQueryClient();
  const { selectedDraftId, selectDraft, workspace, showWorkspace } = useBuilderUi();
  const [reloadCount, setReloadCount] = useState(0);
  const draft = useQuery({
    queryKey: ["publisher-draft", selectedDraftId],
    queryFn: () => publisherApi.getDraft(selectedDraftId!),
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
    await Promise.all([draft.refetch(), queryClient.invalidateQueries({ queryKey: ["publisher-draft-validation", selectedDraftId] })]);
    setReloadCount((count) => count + 1);
  };

  return (
    <section aria-label="Publish">
      <header className="flex flex-wrap items-end justify-between gap-4 border-b border-white/10 pb-6">
        <div>
          <p className="mb-2 text-xs font-bold uppercase tracking-[0.24em] text-amber-300">Publish</p>
          <h1 className="text-4xl font-semibold tracking-tight">{workspace === "distribution" ? "Distribution" : selectedDraftId ? "Course builder" : "Your courses"}</h1>
        </div>
        <div className="flex items-center gap-2">
          <nav aria-label="Publish sections" className="flex gap-1 rounded-xl border border-white/10 bg-white/[0.03] p-1 text-sm">
            {tabs.map(({ value, label, icon: Icon }) => (
              <button
                key={value}
                type="button"
                aria-current={workspace === value ? "page" : undefined}
                onClick={() => showWorkspace(value)}
                className={`flex items-center gap-2 rounded-lg px-3 py-1.5 ${workspace === value ? "bg-white/10 text-white" : "text-slate-400 hover:text-white"}`}
              >
                <Icon size={15} /> {label}
              </button>
            ))}
          </nav>
          {workspace === "courses" && !selectedDraftId ? (
            <button type="button" onClick={() => createDraft.mutate()} disabled={createDraft.isPending} className="flex items-center gap-2 rounded-xl bg-amber-300 px-4 py-2.5 text-sm font-semibold text-slate-950 disabled:opacity-50">
              <BookPlus size={17} /> {createDraft.isPending ? "Creating…" : "New course"}
            </button>
          ) : null}
        </div>
      </header>

      <div className="py-8">
        {workspace === "distribution" ? <DistributionPage onOpenNetwork={onOpenNetwork} /> : selectedDraftId ? (
          <div>
            <button type="button" onClick={() => selectDraft(null)} className="mb-6 flex items-center gap-2 text-sm text-slate-400"><ArrowLeft size={16} /> Back to courses</button>
            {draft.isPending ? <p className="text-slate-400">Opening draft…</p> : draft.data ? (
              <div className="grid grid-cols-[minmax(0,1fr)] gap-6 xl:grid-cols-[minmax(0,1fr)_320px]">
                <Builder key={`${draft.data.draftId}:${reloadCount}`} draft={draft.data} onSaved={handleSaved} onReload={() => void reloadDraft()} />
                <div className="space-y-6">
                  <ReadinessPanel draftId={draft.data.draftId} revision={draft.data.revision} />
                  <PublishPanel draftId={draft.data.draftId} revision={draft.data.revision} />
                </div>
              </div>
            ) : <p role="alert" className="text-red-300">This draft could not be recovered.</p>}
          </div>
        ) : (
          <>
            {createDraft.isError ? <p role="alert" className="mb-5 rounded-2xl border border-red-400/30 bg-red-400/10 p-5 text-red-200">The course could not be created.</p> : null}
            <CourseList onOpenDraft={selectDraft} />
          </>
        )}
      </div>
    </section>
  );
}
