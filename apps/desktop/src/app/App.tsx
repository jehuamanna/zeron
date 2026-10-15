import { BookOpen, Boxes, Network } from "lucide-react";

import { Button } from "../components/ui/button";
import { NetworkSettings } from "../features/network/NetworkSettings";
import { PublishArea } from "../publish/PublishArea";
import { useShell, type Area } from "../stores/shell";
import { Library } from "./Library";

const areas: Array<{ value: Area; label: string; icon: typeof BookOpen }> = [
  { value: "library", label: "Library", icon: BookOpen },
  { value: "publish", label: "Publish", icon: Boxes },
];

/** One Zeron app: Library and Publish side by side, with one Network setting. */
export function App() {
  const { area, showArea, networkOpen, showNetwork } = useShell();

  return (
    <main className="min-h-screen bg-[#071018] text-slate-100">
      <div className="mx-auto max-w-7xl px-6 py-8">
        <header className="flex flex-wrap items-center justify-between gap-4 border-b border-white/10 pb-6">
          <div className="flex items-center gap-3">
            <div className="grid size-10 place-items-center rounded-xl bg-cyan-300 text-slate-950">
              <BookOpen size={22} />
            </div>
            <div>
              <p className="text-xl font-bold tracking-tight">Zeron</p>
              <p className="text-sm text-slate-400">Learn and publish courses, available offline.</p>
            </div>
          </div>
          <nav aria-label="Zeron" className="flex items-center gap-1 rounded-xl border border-white/10 bg-white/[0.03] p-1">
            {areas.map(({ value, label, icon: Icon }) => (
              <button
                key={value}
                type="button"
                aria-current={!networkOpen && area === value ? "page" : undefined}
                onClick={() => showArea(value)}
                className={`flex items-center gap-2 rounded-lg px-4 py-2 text-sm font-medium ${!networkOpen && area === value ? "bg-white/10 text-white" : "text-slate-400 hover:text-white"}`}
              >
                <Icon size={16} /> {label}
              </button>
            ))}
          </nav>
          <Button variant="ghost" onClick={() => showNetwork(true)} aria-pressed={networkOpen}>
            <Network size={17} /> Network
          </Button>
        </header>

        <div className="pt-6">
          {networkOpen ? (
            <NetworkSettings onBack={() => showNetwork(false)} />
          ) : area === "publish" ? (
            <PublishArea onOpenNetwork={() => showNetwork(true)} />
          ) : (
            <Library />
          )}
        </div>
      </div>
    </main>
  );
}
