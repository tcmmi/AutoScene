import { describe, expect, it } from "vitest"
import { type Baseline, compare, groupOf, type Hit, inScope, scan } from "./scan"

// The UI's own code, as text (scan.ts decides what is in scope).
const sources = Object.fromEntries(
  Object.entries(import.meta.glob<string>("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }))
    .map(([path, code]) => [path.replace(/^\/src\//, ""), code] as const)
    .filter(([file]) => inScope(file)),
)
// What each sweep group still has to move into the catalog.
const groups = Object.fromEntries(
  Object.entries(import.meta.glob<Baseline>("./hardcoded/*.json", { import: "default", eager: true })).map(
    ([path, baseline]) => [path.replace(/^.*\/(.+)\.json$/, "$1"), baseline] as const,
  ),
)

const scanAll = (files: Record<string, string>) =>
  Object.fromEntries(Object.entries(files).map(([file, code]) => [file, scan(file, code)] as const))
const where = (hits: (Hit & { file: string })[]) => hits.map((h) => `${h.file}:${h.line} ${JSON.stringify(h.text)}`)

describe("words written into the code", () => {
  it("are only those the sweep groups still have to move into the catalog", () => {
    const merged: Baseline = Object.assign({}, ...Object.values(groups))
    const { added, gone } = compare(scanAll(sources), merged)
    // New words go into src/locales/en/<namespace>.ts and reach the UI through t() (docs/DESIGN.md, Copy);
    // `// i18n-ignore: <why>` on the line before excuses a word that is never translated.
    expect(where(added), "words not in the catalog").toEqual([])
    // A baseline only shrinks: `npm run i18n -- baseline` after moving words out.
    expect(gone.map((g) => `${g.file} ${JSON.stringify(g.text)}`), "baseline entries no longer in the code").toEqual([])
  })

  it("only shrink: no baseline lists more than it did when the guard came in", () => {
    // `npm run i18n -- baseline` never adds; a word the code gains goes into the catalog. Raising a number
    // here is for a change to the guard itself (`baseline --init`), never for a sweep or a feature.
    const most: Record<string, number> = { g1: 215, g2: 211, g3: 283, g4: 190, g5: 136, g6: 171, g7: 158, g8: 299 }
    for (const [group, baseline] of Object.entries(groups)) {
      expect(Object.values(baseline).flat().length, `${group}.json`).toBeLessThanOrEqual(most[group] ?? 0)
    }
  })

  it("are listed in the baseline of the group that owns their file", () => {
    for (const [group, baseline] of Object.entries(groups)) {
      for (const file of Object.keys(baseline)) expect(`${file} ${groupOf(file)}`).toBe(`${file} ${group}`)
    }
    for (const file of Object.keys(sources)) expect(groupOf(file), `${file} has no sweep group`).not.toBeNull()
  })
})

describe("the guard", () => {
  const hitsOf = (file: string, code: string) => ({ [file]: scan(file, code) })

  it("refuses a new word in JSX, an attribute, a toast or a UI prop", () => {
    const code = [
      `import { toast } from "sonner"`,
      `export const items = [{ id: "open", label: "Open the project" }]`,
      `export function A() {`,
      `  toast.error("Couldn't open it", { action: { label: "Undo", onClick() {} } })`,
      `  return <p aria-label="Your projects" className="text-sm px-2">Nothing here yet {"or here"}</p>`,
      `}`,
    ].join("\n")
    const { added } = compare(hitsOf("features/x.tsx", code), {})
    expect(added.map((h) => [h.line, h.ctx, h.text])).toEqual([
      [2, "prop:label", "Open the project"],
      [4, "toast", "Couldn't open it"],
      [4, "prop:label", "Undo"],
      [5, "attr:aria-label", "Your projects"],
      [5, "jsx-text", "Nothing here yet"],
      [5, "jsx-expr", "or here"],
    ])
  })

  it("refuses English plurals and one-word fragments, wherever only words go", () => {
    const code = [
      `export function A({ n, x }: { n: number; x: string }) {`,
      `  const unit = n === 1 ? "episode" : "episodes"`,
      `  return (`,
      `    <p aria-label={\`\${x} options\`} className={n === 1 ? "px-1" : "px-2"}>`,
      `      {\`\${n} faces\`} {n === 1 ? "face" : "faces"} {n} item{n !== 1 ? "s" : ""} {plural(n, "face")}`,
      `    </p>`,
      `  )`,
      `}`,
    ].join("\n")
    expect(compare(hitsOf("features/x.tsx", code), {}).added.map((h) => [h.line, h.ctx, h.text])).toEqual([
      [2, "plural", "episode"],
      [2, "plural", "episodes"],
      [4, "attr:aria-label", "{x} options"],
      [5, "jsx-expr", "{x} faces"],
      [5, "plural", "face"],
      [5, "plural", "faces"],
      [5, "jsx-text", "item"],
      [5, "plural", "s"],
      [5, "helper", "plural("],
    ])
    // cast.ts is where the deprecated helpers live until #17 deletes them.
    expect(compare(hitsOf("features/cast/cast.ts", `export const f = (n: number) => plural(n, "face")`), {}).added).toEqual([])
  })

  it("refuses words in the options of a toast and in the values of t()", () => {
    const code = [
      `export function a(p: Promise<void>, name: string, title: string) {`,
      `  toast.promise(p, { loading: "Saving…", success: "Saved", error: "Couldn't save" })`,
      `  pluginToast(name, { text: \`\${title} couldn't start\`, tone: "error" })`,
      `  return t("project:remove", { what: "the whole project", context: "male", ns: "project" })`,
      `}`,
    ].join("\n")
    expect(compare(hitsOf("lib/x.ts", code), {}).added.map((h) => [h.line, h.ctx, h.text])).toEqual([
      [2, "toast", "Saving…"],
      [2, "toast", "Saved"],
      [2, "toast", "Couldn't save"],
      [3, "toast", "{x} couldn't start"],
      [4, "t-value", "the whole project"],
    ])
  })

  it("refuses an OS dialog's filter name", () => {
    const code = `export const f = () => open({ filters: [{ name: "Video", extensions: ["mp4"] }] })`
    expect(compare(hitsOf("lib/x.ts", code), {}).added.map((h) => h.text)).toEqual(["Video"])
  })

  it("refuses locale-bound formatting outside lib/i18n.ts", () => {
    const code = `export const a = (n: number) => [n.toLocaleString("en-GB"), new Intl.NumberFormat("en-GB").format(n)]`
    expect(compare(hitsOf("lib/x.ts", code), {}).added.map((h) => h.text)).toEqual([
      "toLocaleString(",
      "en-GB",
      "Intl.NumberFormat(",
      "en-GB",
    ])
    expect(compare(hitsOf("lib/i18n.ts", code), {}).added).toEqual([])
  })

  it("lets through keys, identifiers, class lists, brand names and excused words", () => {
    const code = [
      `export function A({ t }: { t: (k: string) => string }) {`,
      `  // i18n-ignore: the product's name in its own type`,
      `  const name = { title: "AutoScene Studio" }`,
      `  const menu = [{ action: "undo", label: t("shell:text.undo") }]`,
      `  return (`,
      `    <div className="flex items-center gap-2" data-state="open" style={{ gridArea: "main" }} onClick={() => go("Somewhere else")}>`,
      `      <Icon weight="bold" />`,
      `      {t("project:episodes.emptyTitle")} S01E03 · AutoScene`,
      `      {/* i18n-ignore: a sample, never shown */}`,
      `      Lorem ipsum`,
      `      {name.title}`,
      `    </div>`,
      `  )`,
      `}`,
    ].join("\n")
    expect(compare(hitsOf("features/x.tsx", code), {}).added).toEqual([])
  })

  it("fails when a baseline lists a word the code no longer has, and counts each occurrence", () => {
    const code = `export const A = () => <><b>Cast</b><i>Cast</i></>`
    expect(compare(hitsOf("features/x.tsx", code), { "features/x.tsx": ["Cast", "Cast"] })).toEqual({ added: [], gone: [] })
    expect(compare(hitsOf("features/x.tsx", code), { "features/x.tsx": ["Cast"] }).added.map((h) => h.text)).toEqual(["Cast"])
    expect(compare(hitsOf("features/x.tsx", code), { "features/x.tsx": ["Cast", "Cast", "Exports"] }).gone).toEqual([
      { file: "features/x.tsx", text: "Exports" },
    ])
  })
})
