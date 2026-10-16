import { create } from "zustand";

const PREFS_KEY = "zeron.player";

interface Prefs {
  speed: number;
  volume: number;
  muted: boolean;
  captions: boolean;
  /** Start the next lesson when a video ends, after a short countdown. */
  autoplayNext: boolean;
}

const DEFAULTS: Prefs = { speed: 1, volume: 1, muted: false, captions: false, autoplayNext: true };

function stored(): Prefs {
  try {
    const parsed = JSON.parse(window.localStorage.getItem(PREFS_KEY) ?? "null") as Partial<Prefs> | null;
    if (!parsed) return DEFAULTS;
    return {
      speed: typeof parsed.speed === "number" && parsed.speed >= 0.25 && parsed.speed <= 3 ? parsed.speed : DEFAULTS.speed,
      volume: typeof parsed.volume === "number" && parsed.volume >= 0 && parsed.volume <= 1 ? parsed.volume : DEFAULTS.volume,
      muted: parsed.muted === true,
      captions: parsed.captions === true,
      autoplayNext: parsed.autoplayNext !== false,
    };
  } catch {
    return DEFAULTS;
  }
}

interface PlayerPrefs extends Prefs {
  set: (change: Partial<Prefs>) => void;
}

/** How the learner likes videos to play; kept across lessons and restarts. */
export const usePlayerPrefs = create<PlayerPrefs>((set, get) => ({
  ...stored(),
  set: (change) => {
    set(change);
    const { speed, volume, muted, captions, autoplayNext } = get();
    try {
      window.localStorage.setItem(PREFS_KEY, JSON.stringify({ speed, volume, muted, captions, autoplayNext }));
    } catch {
      // Preferences are a convenience; losing them is harmless.
    }
  },
}));
