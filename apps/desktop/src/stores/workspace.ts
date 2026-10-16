import { create } from "zustand";

/** What the side panel next to a lesson shows. */
export type SideTab = "notes" | "sketch" | "transcript";

export const SIDE_PANEL_MIN = 300;
export const SIDE_PANEL_MAX = 720;
const SIDE_PANEL_DEFAULT = 380;
const LAYOUT_KEY = "zeron.workspace";

interface Layout {
  contentsOpen: boolean;
  sideOpen: boolean;
  sideWidth: number;
}

function storedLayout(): Layout {
  const fallback = { contentsOpen: true, sideOpen: true, sideWidth: SIDE_PANEL_DEFAULT };
  try {
    const parsed = JSON.parse(window.localStorage.getItem(LAYOUT_KEY) ?? "null") as Partial<Layout> | null;
    if (!parsed) return fallback;
    return {
      contentsOpen: parsed.contentsOpen ?? fallback.contentsOpen,
      sideOpen: parsed.sideOpen ?? fallback.sideOpen,
      sideWidth: clampWidth(parsed.sideWidth ?? fallback.sideWidth),
    };
  } catch {
    return fallback;
  }
}

function remember(layout: Layout) {
  try {
    window.localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
  } catch {
    // Layout is a convenience; losing it is harmless.
  }
}

export function clampWidth(width: number) {
  return Math.round(Math.min(SIDE_PANEL_MAX, Math.max(SIDE_PANEL_MIN, width)));
}

interface WorkspaceState extends Layout {
  sideTab: SideTab;
  showContents: (open: boolean) => void;
  showSide: (open: boolean, tab?: SideTab) => void;
  setSideWidth: (width: number) => void;
}

/** Ephemeral layout of the course workspace; panel sizes survive restarts. */
export const useWorkspace = create<WorkspaceState>((set, get) => {
  const save = () => {
    const { contentsOpen, sideOpen, sideWidth } = get();
    remember({ contentsOpen, sideOpen, sideWidth });
  };
  return {
    ...storedLayout(),
    sideTab: "notes",
    showContents: (contentsOpen) => {
      set({ contentsOpen });
      save();
    },
    showSide: (sideOpen, tab) => {
      set(tab ? { sideOpen, sideTab: tab } : { sideOpen });
      save();
    },
    setSideWidth: (width) => {
      set({ sideWidth: clampWidth(width) });
      save();
    },
  };
});
