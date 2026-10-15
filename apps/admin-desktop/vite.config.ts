import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 1421, strictPort: true },
  test: {
    environment: "jsdom",
    clearMocks: true,
    setupFiles: ["src/test/setup.ts"],
    // Multi-step UI tests take 1-2 s alone but up to about 15 s while other builds
    // load the machine; the 5 s default made a different subset time out each run.
    testTimeout: 30_000,
  },
});
