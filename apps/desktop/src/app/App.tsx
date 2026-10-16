import { BookOpen, Boxes, Moon, Network, Sun } from "lucide-react";
import { useEffect, useReducer } from "react";

import { Tooltips } from "../components/ui/Tooltips";
import { NetworkSettings } from "../features/network/NetworkSettings";
import { PublishArea } from "../publish/PublishArea";
import { applyTheme, resolveTheme, useShell, type Area } from "../stores/shell";
import { Library } from "./Library";

const areas: Array<{ value: Area; label: string; icon: typeof BookOpen }> = [
  { value: "library", label: "Library", icon: BookOpen },
  { value: "publish", label: "Publish", icon: Boxes },
];

/** One Zeron app: Library and Publish side by side, with one Network setting. */
export function App() {
  const { area, showArea, networkOpen, showNetwork, theme, setTheme } = useShell();
  const [, systemChanged] = useReducer((count: number) => count + 1, 0);
  const resolved = resolveTheme(theme);

  useEffect(() => {
    applyTheme(theme);
    if (theme !== "system" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const follow = () => {
      applyTheme("system");
      systemChanged();
    };
    query.addEventListener("change", follow);
    return () => query.removeEventListener("change", follow);
  }, [theme]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-bg text-fg">
      <Tooltips />
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-line bg-panel px-3 sm:px-4">
        <div className="flex items-center gap-2">
          <div aria-hidden className="grid size-7 place-items-center rounded-lg bg-accent text-on-accent">
            <BookOpen size={15} />
          </div>
          <p className="font-display text-lg font-bold tracking-tight">Zeron</p>
        </div>
        <nav aria-label="Zeron" className="ml-2 flex items-center gap-0.5 rounded-lg bg-sunken p-0.5">
          {areas.map(({ value, label, icon: Icon }) => (
            <button
              key={value}
              type="button"
              aria-current={!networkOpen && area === value ? "page" : undefined}
              onClick={() => showArea(value)}
              className={`flex items-center gap-1.5 rounded-md px-3 py-1 text-sm font-medium transition ${!networkOpen && area === value ? "bg-surface text-fg shadow-sm" : "text-muted hover:text-fg"}`}
            >
              <Icon size={15} /> {label}
            </button>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-1">
          <button
            type="button"
            onClick={() => setTheme(resolved === "dark" ? "light" : "dark")}
            aria-label={resolved === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            title={resolved === "dark" ? "Light theme" : "Dark theme"}
            className="grid size-8 place-items-center rounded-lg text-muted hover:bg-white/5 hover:text-fg"
          >
            {resolved === "dark" ? <Sun size={16} /> : <Moon size={16} />}
          </button>
          <button
            type="button"
            onClick={() => showNetwork(true)}
            aria-pressed={networkOpen}
            className={`flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-medium ${networkOpen ? "bg-white/10 text-fg" : "text-muted hover:bg-white/5 hover:text-fg"}`}
          >
            <Network size={15} /> Network
          </button>
        </div>
      </header>

      <div className="min-h-0 flex-1">
        {networkOpen ? (
          <Scrolling>
            <NetworkSettings onBack={() => showNetwork(false)} />
          </Scrolling>
        ) : area === "publish" ? (
          <Scrolling>
            <PublishArea onOpenNetwork={() => showNetwork(true)} />
          </Scrolling>
        ) : (
          <Library />
        )}
      </div>
    </div>
  );
}

/** A full-height area whose page content scrolls inside it. */
function Scrolling({ children }: { children: React.ReactNode }) {
  return (
    <main className="relative h-full overflow-y-auto">
      <div className="mx-auto max-w-7xl px-4 py-6 sm:px-6">{children}</div>
    </main>
  );
}
