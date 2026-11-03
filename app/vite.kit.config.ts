import { defineConfig } from "vite"

// The plugin-page kit (`npm run build:kit`): sdk.js, one classic script that
// defines the global `AutoScene`, and theme.css beside it, with the fonts
// inlined. AutoScene serves both at /_as/ (crates/as-plugin/src/pages.rs);
// the app's build empties dist/, so this runs after it.
export default defineConfig({
  publicDir: false,
  build: {
    lib: {
      entry: "src/plugin-kit/entry.ts",
      name: "AutoScene",
      formats: ["iife"],
      fileName: () => "sdk.js",
      cssFileName: "theme",
    },
    outDir: "dist/_as",
    emptyOutDir: true,
    target: "es2023",
    minify: true,
    sourcemap: false,
  },
})
