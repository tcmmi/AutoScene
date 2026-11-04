import { describe, expect, it } from "vitest"

// Vitest blanks CSS imports (even `?raw`), so the stylesheets are read from disk. The app's
// types leave Node out, so `node:fs` comes in untyped, with only what is used here.
type Fs = { existsSync(path: URL): boolean; readFileSync(path: URL, encoding: "utf8"): string }
const fs: Fs = await import("node:fs" as string)
// Through a variable: Vite would turn `new URL("./x", import.meta.url)` into an asset import.
const here = import.meta.url
const sheet = (name: string) => {
  const url = new URL(name, here)
  return fs.existsSync(url) ? fs.readFileSync(url, "utf8") : ""
}
// The base layer lives in base.css (plugin pages get it through the kit's theme.css); the
// theme tokens stay in index.css.
const css = sheet("index.css") + sheet("base.css")

/** The body of the block that starts with `head`, braces matched. */
function block(text: string, head: string): string {
  const start = text.indexOf(head)
  if (start < 0) return ""
  const open = text.indexOf("{", start)
  let depth = 0
  for (let i = open; i < text.length; i++) {
    if (text[i] === "{") depth++
    else if (text[i] === "}" && --depth === 0) return text.slice(open + 1, i)
  }
  return ""
}

/** The declarations of the rule whose selector list starts with `selector`. */
function rule(text: string, selector: string): string {
  const at = text.indexOf(selector)
  if (at < 0) return ""
  return text.slice(text.indexOf("{", at) + 1, text.indexOf("}", at))
}

const reduce = block(css, "@media (prefers-reduced-motion: reduce)")

describe("reduced motion", () => {
  it("plays every animation once, so no loop jumps to a new place every frame", () => {
    const all = rule(reduce, "*,")
    expect(all).toMatch(/animation-duration:\s*1ms !important/)
    expect(all).toMatch(/transition-duration:\s*1ms !important/)
    expect(all).toMatch(/animation-iteration-count:\s*1 !important/)
  })

  it("lets what is working fade slowly instead of moving", () => {
    const at = reduce.indexOf(".animate-spin,")
    const selectors = reduce.slice(at, reduce.indexOf("{", at)).split(",").map((s) => s.trim())
    expect(selectors).toEqual([".animate-spin", ".animate-ping", ".animate-indeterminate", ".skeleton", ".as-skeleton"])
    const busy = rule(reduce, ".animate-spin,")
    expect(busy).toMatch(/animation-name:\s*busy !important/)
    expect(busy).toMatch(/animation-duration:\s*2s !important/)
    expect(busy).toMatch(/animation-iteration-count:\s*infinite !important/)
    // Longhands only: a shorthand would reset an inline animation-play-state that pauses a hidden sweep.
    expect(busy).not.toMatch(/(^|[;\s])animation:/)
  })

  it("keeps the fade in the same layer, so it travels with the rule", () => {
    const layer = block(css, "@layer base")
    expect(layer).toContain("@media (prefers-reduced-motion: reduce)")
    const fade = block(layer, "@keyframes busy")
    expect(fade).toMatch(/opacity:\s*0\.4/)
  })

  it("names the sweep's animation as a class the rule can match", () => {
    expect(block(css, "@theme")).toMatch(/--animate-indeterminate:\s*indeterminate /)
  })
})
