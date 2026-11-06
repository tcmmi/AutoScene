import { describe, expect, it } from "vitest"
import { inScope, refused, scan } from "./scan"

// The UI's own code, as text (scan.ts decides what is in scope).
const sources = Object.fromEntries(
  Object.entries(import.meta.glob<string>("/src/**/*.{ts,tsx}", { query: "?raw", import: "default", eager: true }))
    .map(([path, code]) => [path.replace(/^\/src\//, ""), code] as const)
    .filter(([file]) => inScope(file)),
)

const scanAll = (files: Record<string, string>) =>
  Object.fromEntries(Object.entries(files).map(([file, code]) => [file, scan(file, code)] as const))

describe("words written into the code", () => {
  it("are none: every word a person reads is in the catalog", () => {
    // New words go into src/locales/en/<namespace>.ts and reach the UI through t() (docs/DESIGN.md, Copy);
    // `// i18n-ignore: <why>` on the line before excuses a word that is never translated.
    expect(refused(scanAll(sources)).map((h) => `${h.file}:${h.line} ${JSON.stringify(h.text)}`), "words not in the catalog").toEqual([])
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
    expect(refused(hitsOf("features/x.tsx", code)).map((h) => [h.line, h.ctx, h.text])).toEqual([
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
      `      {\`\${n} faces\`} {n === 1 ? "face" : "faces"} {n} item{n !== 1 ? "s" : ""}`,
      `    </p>`,
      `  )`,
      `}`,
    ].join("\n")
    expect(refused(hitsOf("features/x.tsx", code)).map((h) => [h.line, h.ctx, h.text])).toEqual([
      [2, "plural", "episode"],
      [2, "plural", "episodes"],
      [4, "attr:aria-label", "{x} options"],
      [5, "jsx-expr", "{x} faces"],
      [5, "plural", "face"],
      [5, "plural", "faces"],
      [5, "jsx-text", "item"],
      [5, "plural", "s"],
    ])
  })

  it("refuses words in the options of a toast and in the values of t()", () => {
    const code = [
      `export function a(p: Promise<void>, name: string, title: string) {`,
      `  toast.promise(p, { loading: "Saving…", success: "Saved", error: "Couldn't save" })`,
      `  pluginToast(name, { text: \`\${title} couldn't start\`, tone: "error" })`,
      `  return t("project:remove", { what: "the whole project", context: "male", ns: "project" })`,
      `}`,
    ].join("\n")
    expect(refused(hitsOf("lib/x.ts", code)).map((h) => [h.line, h.ctx, h.text])).toEqual([
      [2, "toast", "Saving…"],
      [2, "toast", "Saved"],
      [2, "toast", "Couldn't save"],
      [3, "toast", "{x} couldn't start"],
      [4, "t-value", "the whole project"],
    ])
  })

  it("refuses an OS dialog's filter name", () => {
    const code = `export const f = () => open({ filters: [{ name: "Video", extensions: ["mp4"] }] })`
    expect(refused(hitsOf("lib/x.ts", code)).map((h) => h.text)).toEqual(["Video"])
  })

  it("refuses locale-bound formatting outside lib/i18n.ts", () => {
    const code = `export const a = (n: number) => [n.toLocaleString("en-GB"), new Intl.NumberFormat("en-GB").format(n), n.toFixed(3)]`
    expect(refused(hitsOf("lib/x.ts", code)).map((h) => h.text)).toEqual([
      "toLocaleString(",
      "en-GB",
      "Intl.NumberFormat(",
      "en-GB",
      "toFixed(",
    ])
    expect(refused(hitsOf("lib/i18n.ts", code))).toEqual([])
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
    expect(refused(hitsOf("features/x.tsx", code))).toEqual([])
  })

  it("reads a folder that is new like any other, and counts each occurrence", () => {
    const code = `export const A = () => <><b>Cast</b><i>Cast</i></>`
    expect(refused(hitsOf("newplace/x.tsx", code)).map((h) => [h.file, h.text])).toEqual([
      ["newplace/x.tsx", "Cast"],
      ["newplace/x.tsx", "Cast"],
    ])
  })
})
