import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: {
      // A worktree-mode Mission run against CID's own repo materializes a full
      // copy of it under `.cid/worktrees/<id>/` — including an `index.html` and
      // a `tsconfig.json`. Vite treats the latter as a changed tsconfig and
      // forces a full page reload, which wipes the in-memory store and
      // deselects the repo mid-Mission. Observed live, not theorized.
      // `.cid` is CID's own runtime state, never app source.
      ignored: ["**/.cid/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_PLATFORM == "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      output: {
        manualChunks: {
          // `monaco-editor` deliberately absent. Naming it here forces it into
          // an eagerly-reachable chunk, and once Monaco was actually bundled
          // (rather than fetched from a CDN — see src/lib/monaco-setup.ts) that
          // put a 4 MB `modulepreload` in index.html on first paint, undoing
          // the React.lazy split in App.tsx. Left unlisted, it lands inside the
          // lazily-imported EditorPane chunk and is only fetched when the
          // Editor tab is first opened.
          xterm: ["@xterm/xterm", "@xterm/addon-fit"],
          vendor: ["react", "react-dom", "zustand"],
        },
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});
