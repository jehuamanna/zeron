import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BookOpen, Download, HardDrive, Search } from "lucide-react";

import { Button } from "../components/ui/button";
import { libraryApi } from "../services/library";

const formatBytes = (bytes: number) =>
  new Intl.NumberFormat(undefined, { style: "unit", unit: "megabyte", maximumFractionDigits: 1 }).format(
    bytes / 1_000_000,
  );

export function App() {
  const queryClient = useQueryClient();
  const readiness = useQuery({ queryKey: ["readiness"], queryFn: libraryApi.readiness });
  const courses = useQuery({
    queryKey: ["courses"],
    queryFn: libraryApi.list,
    enabled: readiness.data?.state === "ready",
  });
  const importCourse = useMutation({
    mutationFn: libraryApi.chooseAndImport,
    onSuccess: async (receipt) => {
      if (receipt) await queryClient.invalidateQueries({ queryKey: ["courses"] });
    },
  });

  return (
    <main className="min-h-screen bg-[#071018] text-slate-100">
      <div className="mx-auto max-w-7xl px-6 py-8">
        <header className="flex items-center justify-between border-b border-white/10 pb-6">
          <div className="flex items-center gap-3">
            <div className="grid size-10 place-items-center rounded-xl bg-cyan-300 text-slate-950">
              <BookOpen size={22} />
            </div>
            <div>
              <p className="text-xl font-bold tracking-tight">Zeron</p>
              <p className="text-sm text-slate-400">Your learning, available offline.</p>
            </div>
          </div>
          <Button onClick={() => importCourse.mutate()} disabled={importCourse.isPending}>
            <Download size={17} />
            {importCourse.isPending ? "Verifying…" : "Import local course"}
          </Button>
        </header>

        <section className="py-10">
          <div className="mb-8 flex items-end justify-between gap-6">
            <div>
              <p className="mb-2 text-xs font-bold uppercase tracking-[0.24em] text-cyan-300">Library</p>
              <h1 className="text-4xl font-semibold tracking-tight">Continue learning</h1>
            </div>
            <div className="flex items-center gap-2 rounded-xl border border-white/10 bg-white/5 px-3 py-2 text-slate-400">
              <Search size={16} />
              <span className="text-sm">Search arrives with lesson indexing</span>
            </div>
          </div>

          {readiness.isError || courses.isError || importCourse.isError ? (
            <div role="alert" className="rounded-2xl border border-red-400/30 bg-red-400/10 p-5 text-red-100">
              Zeron could not open the local library. Restart the app and check that its data directory is writable.
            </div>
          ) : courses.isPending ? (
            <div className="rounded-2xl border border-white/10 bg-white/[0.03] p-8 text-slate-400">Opening your library…</div>
          ) : courses.data?.length ? (
            <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
              {courses.data.map((course) => (
                <article key={course.courseId} className="rounded-2xl border border-white/10 bg-white/[0.04] p-5 shadow-2xl shadow-black/10">
                  <div className="mb-8 grid size-11 place-items-center rounded-xl bg-cyan-300/10 text-cyan-300">
                    <HardDrive size={20} />
                  </div>
                  <h2 className="text-lg font-semibold">{course.title}</h2>
                  <p className="mt-1 text-sm text-slate-400">{course.publisherName}</p>
                  <div className="mt-5 flex items-center justify-between border-t border-white/10 pt-4 text-xs text-slate-400">
                    <span>Version {course.courseVersion}</span>
                    <span>{formatBytes(course.totalResourceBytes)}</span>
                  </div>
                </article>
              ))}
            </div>
          ) : (
            <div className="rounded-3xl border border-dashed border-white/15 bg-white/[0.025] px-8 py-20 text-center">
              <div className="mx-auto mb-5 grid size-14 place-items-center rounded-2xl bg-white/5 text-cyan-300">
                <BookOpen size={26} />
              </div>
              <h2 className="text-xl font-semibold">Your library is ready</h2>
              <p className="mx-auto mt-2 max-w-md text-slate-400">Import a verified local course folder to begin. Torrent courses join this same library in Phase 3.</p>
            </div>
          )}
        </section>
      </div>
    </main>
  );
}
