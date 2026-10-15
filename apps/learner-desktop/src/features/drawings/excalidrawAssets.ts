// Evaluated before Excalidraw itself, which reads this when it registers its
// fonts. They ship with the app (see `excalidrawFonts` in vite.config.ts) and
// are never fetched from a CDN.

declare global {
  interface Window {
    EXCALIDRAW_ASSET_PATH?: string | string[];
  }
}

window.EXCALIDRAW_ASSET_PATH = `${window.location.origin}/excalidraw/`;

export {};
