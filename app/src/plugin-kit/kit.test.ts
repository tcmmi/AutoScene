import { describe, expect, it } from "vitest"
import manifest from "./fonts/sources.json"

// Node's fs and crypto, with only what is used here: the app's types leave Node out.
type Bytes = { toString(encoding: "base64" | "hex" | "utf8"): string }
const fs: { readFileSync(path: URL): Bytes } = await import("node:fs" as string)
const crypto: { createHash(algorithm: "sha256"): { update(data: Bytes): { digest(encoding: "hex"): string } } } = await import(
  "node:crypto" as string
)
// Through a variable: Vite would turn `new URL("./x", import.meta.url)` into an asset import.
const here = import.meta.url
const romanian = (name: string) => fs.readFileSync(new URL(`./fonts/${name}`, here))
// The app's package.json, read as a file: the kit imports nothing from outside src/plugin-kit.
const pkg: { dependencies: Record<string, string> } = JSON.parse(fs.readFileSync(new URL("../../package.json", here)).toString("utf8"))

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
    expect(urls.length).toBe(8)
    expect(urls.every((u) => u.startsWith("url(data:font/woff2;base64,"))).toBe(true)
    expect(files["theme.css"]).toContain(".as-btn")
    expect(files["theme.css"]).toContain("--bg-canvas")
  })

  it.skipIf(missing && !import.meta.env.CI)("a page's controls show the hand like the app's, a disabled one the arrow", () => {
    const css = files["theme.css"]
    // The build lowers the rule's nesting, so the disabled arrow is a rule of its own: a pattern
    // that only the lowered form can match.
    const hand = css.match(/([^{}]*)\{cursor:\s*pointer\}/)?.[1] ?? ""
    for (const part of ["a[href]", "button", "[role=slider]", ":not([role=option]"]) expect(hand).toContain(part)
    const arrow = css.match(/([^{}]*:is\(:disabled[^{}]*)\{cursor:\s*default\}/)?.[1] ?? ""
    expect(arrow).toContain("a[href]")
    expect(arrow).toContain("button")
  })

  it.skipIf(missing && !import.meta.env.CI)("every face has the Romanian letters outside Latin-1, after its Latin one", () => {
    const css: string = files["theme.css"]
    const faces = (css.match(/@font-face\s*\{[^}]*\}/g) ?? []).map((face) => ({
      family: face.match(/font-family:\s*"?([^;"]+)"?/)?.[1],
      style: face.match(/font-style:\s*(\w+)/)?.[1],
      range: face.match(/unicode-range:\s*([^;}]+)/)?.[1],
    }))
    const kinds = new Set(faces.map((f) => `${f.family}|${f.style}`))
    expect([...kinds].sort()).toEqual(["Geist Mono Variable|normal", "Instrument Serif|italic", "Instrument Serif|normal", "Inter Variable|normal"])
    for (const kind of kinds) {
      // Of two faces that cover a character the later one is asked, so the Romanian one must come last.
      const own = faces.filter((f) => `${f.family}|${f.style}` === kind)
      expect(own, kind).toHaveLength(2)
      // ă U+0102-0103 and ș ț U+0218-021B, as the build writes them; nothing else.
      expect(own[1].range, kind).toBe("U+102-103,U+218-21B")
    }
  })

  it.skipIf(missing && !import.meta.env.CI)("the Romanian letters cost about 20 kB of theme.css", () => {
    // 196.5 kB before the letters, 216.3 kB with the four 2.4-4.6 kB faces (14.3 kB as fonts, a third more as base64);
    // the full latin-ext files would have added about 230 kB. A bigger file means a face grew: check scripts/subset-kit-fonts.py.
    const size = files["theme.css"].length
    expect(size).toBeGreaterThan(210_000)
    expect(size).toBeLessThan(222_000)
  })

  it.skipIf(missing && !import.meta.env.CI)("each Romanian face holds its own font's letters, not another's", () => {
    const css: string = files["theme.css"]
    const faces = (css.match(/@font-face\s*\{[^}]*\}/g) ?? []).map((face) => ({
      kind: `${face.match(/font-family:\s*"?([^;"]+)"?/)?.[1]}|${face.match(/font-style:\s*(\w+)/)?.[1]}`,
      data: face.match(/url\(data:font\/woff2;base64,([^)]*)\)/)?.[1],
    }))
    const own: Record<string, string> = {
      "Inter Variable|normal": "inter-ro-opsz-normal.woff2",
      "Instrument Serif|normal": "instrument-serif-ro-400-normal.woff2",
      "Instrument Serif|italic": "instrument-serif-ro-400-italic.woff2",
      "Geist Mono Variable|normal": "geist-mono-ro-wght-normal.woff2",
    }
    for (const [kind, file] of Object.entries(own)) {
      const second = faces.filter((f) => f.kind === kind)[1]
      expect(second?.data, kind).toBe(romanian(file).toString("base64"))
    }
  })

  it("the Romanian fonts are the ones cut from the @fontsource versions package.json pins", () => {
    // scripts/subset-kit-fonts.py writes sources.json; a bump of a package without a rerun leaves old glyphs beside new Latin ones.
    for (const [name, version] of Object.entries(manifest.fontsource)) expect(pkg.dependencies[name], name).toBe(version)
    for (const [file, hash] of Object.entries(manifest.sha256)) {
      expect(crypto.createHash("sha256").update(romanian(file)).digest("hex"), file).toBe(hash)
    }
  })
})
