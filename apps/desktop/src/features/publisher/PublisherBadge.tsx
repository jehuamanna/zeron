import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ShieldCheck, ShieldQuestion } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/ui/button";
import { asCommandError, libraryApi, type CoursePublisher } from "../../services/library";

/** Who published a course, and a way to stop requiring that publisher. */
export function PublisherBadge({ courseId, publisher }: { courseId: string; publisher: CoursePublisher }) {
  const queryClient = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const forget = useMutation({
    mutationFn: () => libraryApi.forgetCoursePublisher(courseId),
    onSuccess: () => {
      setConfirming(false);
      void queryClient.invalidateQueries({ queryKey: ["course-detail", courseId] });
    },
  });
  const signed = publisher.status === "verified" || publisher.status === "signed";
  return (
    <div className="mt-4 max-w-2xl text-sm">
      <p className={`flex flex-wrap items-center gap-2 ${signed ? "text-emerald-200" : "text-slate-400"}`}>
        {signed ? <ShieldCheck size={16} /> : <ShieldQuestion size={16} />}
        {publisher.status === "verified" ? "Verified publisher" : publisher.status === "signed" ? "Signed, publisher not pinned" : "Unsigned course"}
        {publisher.fingerprint ? <code className="rounded bg-white/5 px-1.5 py-0.5 text-xs tracking-wider text-slate-200">{publisher.fingerprint}</code> : null}
        {publisher.status === "verified" && !confirming ? (
          <button type="button" onClick={() => setConfirming(true)} className="text-xs text-slate-400 underline-offset-2 hover:underline">Forget publisher…</button>
        ) : null}
      </p>
      {publisher.status === "verified" ? (
        <p className="mt-1 text-xs text-slate-500">Updates must be signed by this publisher. Compare the fingerprint with the one your publisher gives you.</p>
      ) : publisher.status === "unsigned" ? (
        <p className="mt-1 text-xs text-slate-500">Zeron cannot tell who published this course. Anyone could publish an update of it.</p>
      ) : null}
      {confirming ? (
        <div role="alertdialog" aria-label="Forget this publisher?" className="mt-2 space-y-2 rounded-xl border border-amber-300/30 bg-amber-300/10 p-3 text-xs text-amber-100">
          <p>The next signed version you import will be trusted, whoever signed it. Do this only if your publisher told you they have a new identity. Your notes and progress stay.</p>
          <div className="flex gap-2">
            <Button onClick={() => forget.mutate()} disabled={forget.isPending}>{forget.isPending ? "Forgetting…" : "Forget publisher"}</Button>
            <Button variant="ghost" autoFocus onClick={() => setConfirming(false)}>Cancel</Button>
          </div>
          {forget.isError ? <p role="alert" className="text-red-200">{asCommandError(forget.error)?.message ?? "The publisher could not be forgotten."}</p> : null}
        </div>
      ) : null}
    </div>
  );
}
