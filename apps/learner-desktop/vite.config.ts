import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

/**
 * Serves Excalidraw's handwriting and UI fonts from the app itself, at
 * `/excalidraw/fonts/...`, so drawing never reaches a CDN and the CSP keeps
 * `font-src 'self'`. Xiaolai (13 MB of CJK glyphs) is left out; CJK text
 * falls back to a system font.
 */
function excalidrawFonts(): Plugin {
  const root = join(import.meta.dirname, "node_modules/@excalidraw/excalidraw/dist/prod/fonts");
  const files = (dir: string): string[] =>
    readdirSync(dir).flatMap((name) => {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) return name === "Xiaolai" ? [] : files(path);
      return name.endsWith(".woff2") ? [path] : [];
    });
  const published = () => files(root).map((path) => ({ path, name: `excalidraw/fonts/${relative(root, path).split(sep).join("/")}` }));
  return {
    name: "zeron-excalidraw-fonts",
    configureServer(server) {
      const fonts = new Map(published().map(({ path, name }) => [`/${name}`, path]));
      server.middlewares.use((request, response, next) => {
        const path = fonts.get(request.url?.split("?")[0] ?? "");
        if (!path) return next();
        response.setHeader("Content-Type", "font/woff2");
        response.end(readFileSync(path));
      });
    },
    generateBundle() {
      for (const { path, name } of published()) this.emitFile({ type: "asset", fileName: name, source: readFileSync(path) });
    },
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), excalidrawFonts()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: "jsdom",
    clearMocks: true,
    setupFiles: ["src/test/setup.ts"],
    // Multi-step UI tests take 1-2 s alone but up to about 15 s while other builds
    // load the machine; the 5 s default made a different subset time out each run.
    testTimeout: 30_000,
  },
});
