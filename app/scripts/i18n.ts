// npm run i18n -- <command>  (Node runs this file by stripping its types)
//
//   count                     every hard-coded string in app/src, by folder and by context (the inventory)
//   status [--compare <rev>]  how much the guard lets through, and the catalog's size; with --compare, the
//                             English that left the code since <rev> but is in no catalog, byte for byte
//   unguarded                 the hits the guard lets through (returns, variables, call arguments, label maps),
//                             by file and line
//
// A translation's review (docs/TRANSLATING.md); <lang> is its folder under src/locales, `ro`:
//
//   sheet <lang> [--out <file>]     every message as a Markdown table, for the reviewer: the language, its English,
//                                   its note and state; stale and draft first, drafts with a [check] word before
//                                   the others. Written to app/i18n-review-<lang>.md, which git ignores
//   reviewed <lang> <ns|keys>…      marks a namespace's messages, or `ns:key` ones, reviewed in <lang>/meta.json
//   stale <lang> <keys>…            for English that changed and can't be translated now: moves each message's
//                                   text from <lang>/<ns>.json into its meta entry, so the app shows English
import { execFileSync } from "node:child_process"
import { readdirSync, readFileSync, writeFileSync } from "node:fs"
import { join, relative } from "node:path"
import { pathToFileURL } from "node:url"
import type { Bundle, Meta } from "../src/locales/check.ts"
import { checkList, checkTerms, markReviewed, markStale, notesOf, type Outcome, reviewRows, reviewSheet } from "../src/locales/review.ts"
import { type Hit, inScope, scan } from "../src/locales/scan.ts"

const app = join(import.meta.dirname, "..")
const src = join(app, "src")

function walk(dir: string, out: string[] = []): string[] {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name)
    if (e.isDirectory()) walk(p, out)
    else out.push(relative(src, p).replaceAll("\\", "/"))
  }
  return out
}

function scanTree(): Record<string, Hit[]> {
  const hits: Record<string, Hit[]> = {}
  for (const file of walk(src).filter(inScope).sort()) hits[file] = scan(file, readFileSync(join(src, file), "utf8"))
  return hits
}

const words = (s: string) => s.replaceAll("{x}", " ").split(/\s+/).filter((w) => /[A-Za-z]/.test(w)).length

function count() {
  const hits = scanTree()
  const cols = ["files", "strings", "words", "guarded"] as const
  const folders = new Map<string, Record<(typeof cols)[number], number>>()
  const contexts = new Map<string, number>()
  for (const [file, list] of Object.entries(hits)) {
    const parts = file.split("/")
    const folder = parts.length > 2 ? `${parts[0]}/${parts[1]}` : parts.length === 2 ? parts[0] : "(root)"
    const row = folders.get(folder) ?? { files: 0, strings: 0, words: 0, guarded: 0 }
    row.files++
    for (const h of list.filter((x) => x.ctx !== "locale")) {
      row.strings++
      row.words += words(h.text)
      if (h.guarded) row.guarded++
      contexts.set(h.ctx, (contexts.get(h.ctx) ?? 0) + 1)
    }
    folders.set(folder, row)
  }
  const total = { files: 0, strings: 0, words: 0, guarded: 0 }
  console.log("folder".padEnd(24) + cols.map((c) => c.padStart(9)).join(""))
  for (const [folder, row] of [...folders].sort((a, b) => b[1].strings - a[1].strings)) {
    console.log(folder.padEnd(24) + cols.map((c) => String(row[c]).padStart(9)).join(""))
    for (const c of cols) total[c] += row[c]
  }
  console.log("total".padEnd(24) + cols.map((c) => String(total[c]).padStart(9)).join(""))
  const locale = Object.values(hits).flat().filter((h) => h.ctx === "locale").length
  console.log(`\nlocale-bound formatting outside lib/i18n.ts: ${locale}`)
  console.log(`contexts: ${[...contexts].sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k}=${v}`).join("  ")}`)
}

/** Every English leaf of the catalogs, `{{variables}}` written as `{x}`. */
async function englishCatalog(): Promise<Set<string>> {
  const dir = join(src, "locales", "en")
  const out = new Set<string>()
  const add = (v: unknown) => {
    if (typeof v === "string") out.add(v.replace(/\{\{[^}]*\}\}/g, "{x}"))
    else if (v && typeof v === "object") for (const x of Object.values(v)) add(x)
  }
  for (const f of readdirSync(dir)) {
    if (f.endsWith(".ts")) add((await import(pathToFileURL(join(dir, f)).href)).default)
    else if (f.endsWith(".json")) add(JSON.parse(readFileSync(join(dir, f), "utf8")))
  }
  return out
}

function textsAt(rev: string): Map<string, string[]> {
  const git = (...args: string[]) => execFileSync("git", args, { cwd: app, encoding: "utf8", maxBuffer: 1 << 28 })
  const files = git("ls-tree", "-r", "--name-only", rev, "--", "src").split("\n").filter(Boolean)
  const out = new Map<string, string[]>()
  for (const path of files) {
    const file = path.replace(/^src\//, "")
    if (!inScope(file)) continue
    const texts = scan(file, git("show", `${rev}:./${path}`))
      .filter((h) => h.ctx !== "locale")
      .map((h) => h.text)
    out.set(file, texts)
  }
  return out
}

async function status(rev: string | undefined) {
  const hits = scanTree()
  console.log(`Unguarded: ${unguardedHits(hits).length} texts the guard lets through (\`npm run i18n -- unguarded\` lists them)`)
  const catalog = await englishCatalog()
  console.log(`\nEnglish catalog: ${catalog.size} distinct texts`)
  if (!rev) return
  const before = textsAt(rev)
  const left = new Set(Object.values(hits).flat().map((h) => h.text))
  const lost: string[] = []
  for (const [file, texts] of before) {
    for (const text of new Set(texts)) {
      if (left.has(text) || catalog.has(text)) continue
      lost.push(`${file}: ${JSON.stringify(text)}`)
    }
  }
  console.log(
    lost.length === 0
      ? `Every text that left the code since ${rev} is in the catalog, byte for byte.`
      : `${lost.length} texts left the code since ${rev} and are in no catalog as they were (changed English, a plural or a whole-message rewrite: check each):\n  ${lost.join("\n  ")}`,
  )
}

/** Hits the guard lets through (returns, variables, call arguments, label maps). */
function unguardedHits(hits: Record<string, Hit[]>): (Hit & { file: string })[] {
  return Object.entries(hits).flatMap(([file, list]) => list.filter((h) => !h.guarded && h.ctx !== "locale").map((h) => ({ ...h, file })))
}

function unguarded() {
  const list = unguardedHits(scanTree())
  for (const h of list) console.log(`${h.file}:${h.line}  ${h.ctx}  ${JSON.stringify(h.text)}`)
  console.log(`\n${list.length} unguarded texts: move each that a person reads, mark the rest \`// i18n-ignore: <why>\` or leave it`)
}

/** The English of every namespace, as the app has it. */
async function english(): Promise<Bundle> {
  const dir = join(src, "locales", "en")
  const out: Bundle = {}
  for (const f of readdirSync(dir).sort()) {
    const ns = f.replace(/\.\w+$/, "")
    if (f.endsWith(".ts")) out[ns] = (await import(pathToFileURL(join(dir, f)).href)).default
    else if (f.endsWith(".json")) out[ns] = JSON.parse(readFileSync(join(dir, f), "utf8"))
  }
  return out
}

/** A language's folder, or the usage line and out. */
function folderOf(lang: string | undefined): string {
  const dir = join(src, "locales", lang ?? "")
  if (!lang || lang === "en" || !existsSync(join(dir, "meta.json"))) {
    console.error("usage: npm run i18n -- sheet <lang> [--out <file>] | reviewed <lang> <ns|keys>… | stale <lang> <keys>…")
    process.exit(1)
  }
  return dir
}

const readJson = <T>(path: string): T => JSON.parse(readFileSync(path, "utf8"))
const writeJson = (path: string, value: unknown) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`)

function translation(dir: string): { bundle: Bundle; meta: Meta } {
  const bundle: Bundle = {}
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".json") && f !== "meta.json")) bundle[f.replace(/\.json$/, "")] = readJson(join(dir, f))
  return { bundle, meta: readJson(join(dir, "meta.json")) }
}

async function sheet(lang: string | undefined, out: string | undefined) {
  const dir = folderOf(lang)
  const words = await english()
  const { bundle, meta } = translation(dir)
  const notes = new Map<string, string>()
  for (const f of readdirSync(join(src, "locales", "en")).filter((f) => f.endsWith(".ts"))) {
    const code = readFileSync(join(src, "locales", "en", f), "utf8")
    for (const [key, note] of notesOf(f.replace(/\.ts$/, ""), code)) notes.set(key, note)
  }
  const doc = readFileSync(join(app, "..", "docs", "TRANSLATING.md"), "utf8")
  const rows = reviewRows(words, bundle, meta, notes, lang === "ro" ? checkTerms(doc) : [])
  const path = out || join(app, `i18n-review-${lang}.md`)
  writeFileSync(path, reviewSheet(lang as string, rows, lang === "ro" ? checkList(doc) : ""))
  console.log(`${rows.length} messages in ${path}`)
}

/** Says what a command did, or fails without having written anything. */
function report(outcome: Outcome, did: string): boolean {
  for (const skipped of outcome.skipped) console.log(`skipped ${skipped}`)
  if (outcome.problems.length > 0) {
    console.error(`Nothing changed:\n  ${outcome.problems.join("\n  ")}`)
    process.exitCode = 1
    return false
  }
  console.log(`${outcome.done.length} messages ${did}`)
  return true
}

async function reviewed(lang: string | undefined, targets: string[]) {
  const dir = folderOf(lang)
  if (targets.length === 0) folderOf(undefined)
  const { meta } = translation(dir)
  if (report(markReviewed(await english(), meta, targets), "marked reviewed")) writeJson(join(dir, "meta.json"), meta)
}

async function stale(lang: string | undefined, keys: string[]) {
  const dir = folderOf(lang)
  if (keys.length === 0) folderOf(undefined)
  const { bundle, meta } = translation(dir)
  const outcome = markStale(await english(), bundle, meta, keys)
  if (!report(outcome, "marked stale")) return
  writeJson(join(dir, "meta.json"), meta)
  for (const ns of new Set(outcome.done.map((key) => key.split(":")[0]))) writeJson(join(dir, `${ns}.json`), bundle[ns])
}

const [cmd, ...rest] = process.argv.slice(2)
const flag = (name: string) => {
  const i = rest.indexOf(name)
  return i >= 0 ? (rest[i + 1] ?? "") : undefined
}
if (cmd === "count") count()
else if (cmd === "status") await status(flag("--compare"))
else if (cmd === "unguarded") unguarded()
else if (cmd === "sheet") await sheet(rest[0], flag("--out"))
else if (cmd === "reviewed") await reviewed(rest[0], rest.slice(1))
else if (cmd === "stale") await stale(rest[0], rest.slice(1))
else {
  console.error(
    "usage: npm run i18n -- count | status [--compare <rev>] | unguarded | sheet <lang> [--out <file>] | reviewed <lang> <ns|keys>… | stale <lang> <keys>…",
  )
  process.exit(1)
}
