// npm run i18n -- <command>  (Node runs this file by stripping its types)
//
//   count                     every hard-coded string in app/src, by folder and by context (the inventory)
//   baseline                  shrink src/locales/hardcoded/<group>.json to what the code still has; it never
//                             adds: a word the code gained is refused (exit 1) and goes into the catalog instead
//   baseline --init           write every group's file from the code, additions included: only when the guard
//                             itself changes or a join brings code from before it (phase 0), never in a sweep
//   status [--compare <rev>]  what each group still has to move (guarded, in its baseline) and what the guard
//                             lets through (unguarded, to look at), and the catalog's size; with --compare, the
//                             English that left the code since <rev> but is in no catalog, byte for byte
//   unguarded <group>         the group's hits the guard lets through (returns, variables, call arguments, label
//                             maps), by file and line: check each before deleting the group's baseline
//
// The Romanian commands (sheet, stale, reviewed) arrive with the Romanian catalog (docs/DESIGN.md, Copy).
import { execFileSync } from "node:child_process"
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { join, relative } from "node:path"
import { pathToFileURL } from "node:url"
import { type Baseline, baselines, GROUP_NAMES, groupOf, type Hit, inScope, scan } from "../src/locales/scan.ts"

const app = join(import.meta.dirname, "..")
const src = join(app, "src")
const hardcoded = join(src, "locales", "hardcoded")

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

function readBaselines(): Record<string, Record<string, string[]>> {
  if (!existsSync(hardcoded)) return {}
  const out: Record<string, Record<string, string[]>> = {}
  for (const f of readdirSync(hardcoded).filter((f) => f.endsWith(".json"))) {
    out[f.replace(/\.json$/, "")] = JSON.parse(readFileSync(join(hardcoded, f), "utf8"))
  }
  return out
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

const total = (b: Baseline | undefined) => (b ? Object.values(b).flat().length : 0)
const write = (path: string, b: Baseline) => writeFileSync(path, `${JSON.stringify(b, null, 2)}\n`)

/** What `next` holds beyond `listed`, per occurrence. */
function beyond(next: string[], listed: string[]): string[] {
  const left = new Map<string, number>()
  for (const t of listed) left.set(t, (left.get(t) ?? 0) + 1)
  return next.filter((t) => {
    const n = left.get(t) ?? 0
    if (n > 0) left.set(t, n - 1)
    return n === 0
  })
}

function baseline(init: boolean) {
  const next = baselines(scanTree())
  const files = readBaselines()
  mkdirSync(hardcoded, { recursive: true })
  const refused: string[] = []
  for (const group of GROUP_NAMES) {
    const path = join(hardcoded, `${group}.json`)
    const now = next[group] ?? {}
    if (init) {
      if (next[group]) {
        write(path, now)
        console.log(`${group}: ${total(now)} strings in ${Object.keys(now).length} files`)
      } else if (existsSync(path)) {
        rmSync(path)
        console.log(`${group}: nothing left, baseline removed`)
      }
      continue
    }
    // Subtract only: keep each listed word the code still has, add nothing.
    const listed = files[group] ?? {}
    const kept: Baseline = {}
    for (const [file, texts] of Object.entries(listed)) {
      const gone = beyond(texts, now[file] ?? [])
      const keep = beyond(texts, gone)
      if (keep.length > 0) kept[file] = keep
    }
    for (const [file, texts] of Object.entries(now)) {
      for (const text of beyond(texts, listed[file] ?? [])) refused.push(`${file}: ${JSON.stringify(text)}`)
    }
    if (!files[group]) continue
    if (Object.keys(kept).length === 0) {
      rmSync(path)
      console.log(`${group}: nothing left, baseline removed`)
    } else {
      write(path, kept)
      console.log(`${group}: ${total(kept)} strings in ${Object.keys(kept).length} files (was ${total(listed)})`)
    }
  }
  if (refused.length > 0) {
    console.error(
      `\n${refused.length} words are in the code but in no baseline. A baseline never grows: put them in the catalog (src/locales/en, t()), or mark one that is never translated \`// i18n-ignore: <why>\`.\n  ${refused.join("\n  ")}`,
    )
    process.exitCode = 1
  }
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
  const now = baselines(hits)
  const files = readBaselines()
  const loose = unguardedHits(hits)
  console.log("group    baseline   in code  unguarded")
  for (const g of GROUP_NAMES) {
    const listed = total(files[g])
    const live = total(now[g])
    const free = loose.filter((h) => h.group === g).length
    console.log(`${g.padEnd(9)}${String(files[g] ? listed : "—").padStart(8)}${String(live).padStart(10)}${String(free).padStart(11)}`)
  }
  console.log("(unguarded: words the guard lets through; `npm run i18n -- unguarded <group>` lists them)")
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

/** Hits the guard lets through (returns, variables, call arguments, label maps): a sweep group still checks each. */
function unguardedHits(hits: Record<string, Hit[]>): (Hit & { file: string; group: string | null })[] {
  return Object.entries(hits).flatMap(([file, list]) =>
    list.filter((h) => !h.guarded && h.ctx !== "locale").map((h) => ({ ...h, file, group: groupOf(file) })),
  )
}

function unguarded(group: string | undefined) {
  if (!group || !(GROUP_NAMES as readonly string[]).includes(group)) {
    console.error(`usage: npm run i18n -- unguarded <${GROUP_NAMES.join("|")}>`)
    process.exit(1)
  }
  const list = unguardedHits(scanTree()).filter((h) => h.group === group)
  for (const h of list) console.log(`${h.file}:${h.line}  ${h.ctx}  ${JSON.stringify(h.text)}`)
  console.log(`\n${list.length} unguarded texts in ${group}'s files: move each that a person reads, mark the rest \`// i18n-ignore: <why>\` or leave it`)
}

const [cmd, ...rest] = process.argv.slice(2)
const flag = (name: string) => {
  const i = rest.indexOf(name)
  return i >= 0 ? (rest[i + 1] ?? "") : undefined
}
if (cmd === "count") count()
else if (cmd === "baseline") baseline(rest.includes("--init"))
else if (cmd === "status") await status(flag("--compare"))
else if (cmd === "unguarded") unguarded(rest[0])
else {
  console.error("usage: npm run i18n -- count | baseline [--init] | status [--compare <rev>] | unguarded <group>")
  process.exit(1)
}
