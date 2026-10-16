import { create } from "zustand";

/** The app's top-level areas, always reachable from the header. */
export type Area = "library" | "publish";

/** The learner's colour preference; "system" follows the desktop. */
export type ThemePreference = "system" | "light" | "dark";

const THEME_KEY = "zeron.theme";

function storedTheme(): ThemePreference {
  try {
    const value = window.localStorage.getItem(THEME_KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

interface ShellState {
  area: Area;
  showArea: (area: Area) => void;
  /** Network settings cover the whole app; they open over the current area. */
  networkOpen: boolean;
  showNetwork: (open: boolean) => void;
  theme: ThemePreference;
  setTheme: (theme: ThemePreference) => void;
}

export const useShell = create<ShellState>((set) => ({
  area: "library",
  showArea: (area) => set({ area, networkOpen: false }),
  networkOpen: false,
  showNetwork: (networkOpen) => set({ networkOpen }),
  theme: storedTheme(),
  setTheme: (theme) => {
    try {
      window.localStorage.setItem(THEME_KEY, theme);
    } catch {
      // A private profile keeps the choice for this session only.
    }
    set({ theme });
  },
}));

/** Whether the desktop asks for a dark theme. */
export function systemPrefersDark(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function resolveTheme(theme: ThemePreference): "light" | "dark" {
  return theme === "system" ? (systemPrefersDark() ? "dark" : "light") : theme;
}

/** The theme in effect, following the desktop while the preference is "system". */
export function useResolvedTheme(): "light" | "dark" {
  const theme = useShell((state) => state.theme);
  return resolveTheme(theme);
}

/** Applies the theme to the document; called before the first paint and on change. */
export function applyTheme(theme: ThemePreference) {
  document.documentElement.dataset.theme = resolveTheme(theme);
}
