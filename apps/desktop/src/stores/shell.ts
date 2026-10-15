import { create } from "zustand";

/** The app's top-level areas, always reachable from the header. */
export type Area = "library" | "publish";

interface ShellState {
  area: Area;
  showArea: (area: Area) => void;
  /** Network settings cover the whole app; they open over the current area. */
  networkOpen: boolean;
  showNetwork: (open: boolean) => void;
}

export const useShell = create<ShellState>((set) => ({
  area: "library",
  showArea: (area) => set({ area, networkOpen: false }),
  networkOpen: false,
  showNetwork: (networkOpen) => set({ networkOpen }),
}));
