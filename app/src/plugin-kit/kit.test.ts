import { describe, expect, it } from "vitest"

// What `npm run build:kit` wrote, read as text. CI builds the kit before the
// tests, so it is there; a local run without a build skips this.
const built = import.meta.glob<string>("/dist/_as/*", { query: "?raw", import: "default", eager: true })
const files = Object.fromEntries(Object.entries(built).map(([path, text]) => [path.split("/").pop(), text]))
const missing = Object.keys(files).length === 0

describe("the kit build", () => {
  it.skipIf(missing && !import.meta.env.CI)("has only sdk.js and theme.css", () => {
    expect(Object.keys(files).sort(), "run `npm run build:kit` first").toEqual(["sdk.js", "theme.css"])
    // One classic script that defines the global, and no import of its own.
    expect(files["sdk.js"]).toMatch(/^var AutoScene=/)
    expect(files["sdk.js"]).not.toMatch(/\bimport\s*[("{*]/)
    // The fonts inlined, and nothing fetched from anywhere: a page loads no
    // font but what the theme brings.
    const urls: string[] = files["theme.css"].match(/url\([^)]*\)/g) ?? []
    expect(urls.length).toBe(4)
    expect(urls.every((u) => u.startsWith("url(data:font/woff2;base64,"))).toBe(true)
    expect(files["theme.css"]).toContain(".as-btn")
    expect(files["theme.css"]).toContain("--bg-canvas")
  })
})
