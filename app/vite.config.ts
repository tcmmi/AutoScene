/// <reference types="vitest/config" />
import path from "node:path"
import { defineConfig } from "vite"
import react from "@vitejs/plugin-react"
import tailwindcss from "@tailwindcss/vite"
import { mockAsp } from "./vite.mock-asp.ts"

const repoRoot = path.resolve(import.meta.dirname, "..")
const fixtures = path.join(repoRoot, "fixtures", "out")

// Tauri expects a fixed port and must not have the terminal cleared. While
// developing, the browser mock's plugin pages load from a second server, on
// an origin of their own (vite.mock-asp.ts); not while testing.
export default defineConfig(({ command, mode }) => ({
  plugins: [
    react(),
    tailwindcss(),
    command === "serve" &&
      mode !== "test" &&
      mockAsp({
        examples: path.join(repoRoot, "examples", "plugins"),
        golden: path.join(import.meta.dirname, "src", "plugin-kit", "page-csp.golden.txt"),
        kit: path.join(import.meta.dirname, "dist", "_as"),
        fixtures,
      }),
  ],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    // The browser mock plays the barcoded test fixtures straight from fixtures/out.
    fs: { allow: [import.meta.dirname, fixtures] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  define:
    command === "serve"
      ? { "import.meta.env.VITE_FIXTURES_URL": JSON.stringify(`/@fs/${fixtures.split(path.sep).join("/")}/`) }
      : {},
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "./src") } },
  build: {
    target: "es2023",
    outDir: "dist",
    emptyOutDir: true,
    // Maps only in development: a shipped app doesn't need its sources inside.
    sourcemap: command === "serve",
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}", "vite.mock-asp.test.ts"],
    setupFiles: ["src/test/setup.ts"],
    // The kit's built theme.css and the base layer's cursor rule are read as
    // text (plugin-kit/kit.test.ts, styles/cursor.test.tsx); other CSS stays out
    // of the tests.
    css: { include: [/\/dist\/_as\/theme\.css/, /\/src\/styles\/base\.css/] },
  },
}))
