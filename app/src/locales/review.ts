// What a translation's reviewer works from, and what the review changes: the
// sheet of every message, `reviewed` and `stale` (docs/TRANSLATING.md). Shared
// by `npm run i18n` (scripts/i18n.ts) and review.test.ts, so keep it to
// erasable TypeScript: Node runs the script by stripping types. It reads and
// writes no file; the script does that.
import { parseSync } from "vite"
import { type Bundle, type Meta, type MetaEntry, messageHash, messagesOf } from "./check.ts"

/** A plural form's order in a cell and in a stale entry's `was`, a plain text last, as `messageHash` reads them. */
const FORM_ORDER = ["zero", "one", "two", "few", "many", "other", ""]
const FORM_SUFFIX = /_(zero|one|two|few|many|other)$/

/** Notes for the namespaces that are JSON and can hold no comment: the first prefix that fits a key is its note. */
const JSON_NOTES: [string, string][] = [
  ["backend:job.stage.", "A job's stage, after its title in Activity: work in progress."],
  ["backend:job.", "A job's title in Activity."],
  ["backend:", "What AutoScene's backend says: an error or a reason, shown in a toast, a dialog or a status line."],
  ["native:menu.", "An entry of the macOS menu."],
  ["native:dialog.", "A dialog the system draws for AutoScene."],
  ["native:file.", "Written into an exported file's name or chapters."],
]

/**
 * The translator's notes of an English catalog, by message: the comment right above a key (the first plural
 * form's stands for all of them), else the nearest one above its group.
 */
export function notesOf(ns: string, code: string): Map<string, string> {
  const { program, comments } = parseSync(`${ns}.ts`, code)
  const byEnd = new Map(comments.map((c) => [c.end, c.value]))
  const noteAt = (start: number): string | undefined => {
    let end = start
    while (end > 0 && /\s/.test(code[end - 1])) end--
    const text = byEnd.get(end)
    return text?.replace(/^\s*\*\s?/gm, "").replace(/\s+/g, " ").trim().replace(/^\*\s*/, "")
  }
  const notes = new Map<string, string>()
  // The parser's tree, as far as a catalog's `export default { … } as const` goes.
  type Node = { type: string; start: number; key?: { name?: string; value?: string }; value?: Node; properties?: Node[]; expression?: Node; declaration?: Node }
  const walk = (object: Node, path: string, above: string | undefined) => {
    for (const property of object.properties ?? []) {
      const name = property.key?.name ?? property.key?.value
      if (name === undefined || !property.value) continue
      const note = noteAt(property.start) ?? above
      const key = `${path}${name}`
      if (property.value.type === "ObjectExpression") walk(property.value, `${key}.`, note)
      else {
        const base = key.replace(FORM_SUFFIX, "")
        if (note && !notes.has(base)) notes.set(base, note)
      }
    }
  }
  const body = program.body as unknown as Node[]
  const exported = body.find((n) => n.type === "ExportDefaultDeclaration")?.declaration
  const root = exported?.type === "TSAsExpression" ? exported.expression : exported
  if (root) walk(root, `${ns}:`, undefined)
  return notes
}

/** The note of a message: its own, or for a namespace that has no comments, what its place in the app says. */
export function noteFor(key: string, notes: ReadonlyMap<string, string>): string {
  return notes.get(key) ?? JSON_NOTES.find(([prefix]) => key.startsWith(prefix))?.[1] ?? ""
}

/** The Romanian words the glossary (docs/TRANSLATING.md) marks **[check]**, from its tables, in lower case. */
export function checkTerms(doc: string): string[] {
  const terms = new Set<string>()
  const add = (cell: string) => {
    for (const term of cell.replace(/[*`]/g, "").split(/[,/()]/)) if (term.trim()) terms.add(term.trim().toLowerCase())
  }
  const cells = (row: string) => row.split("|").map((c) => c.trim())
  let sizes = false // A whole table can be marked, by the line above it.
  for (const line of doc.slice(Math.max(doc.indexOf("## Romanian"), 0)).split("\n")) {
    if (!line.startsWith("|")) {
      if (line.trim()) sizes = line.includes("[check]") && line.trimEnd().endsWith(":")
      continue
    }
    const romanian = cells(line)[2] ?? ""
    if (sizes) {
      if (romanian !== "Romanian" && !/^-+$/.test(romanian)) add(romanian)
    } else if (line.includes("[check]")) {
      // `Cu actori reali` **[check]** marks those words, not the row's own.
      const named = [...line.matchAll(/`([^`]+)`\s*\*\*\[check\]/g)].map((m) => m[1])
      for (const words of named.length > 0 ? named : [romanian]) add(words)
    }
  }
  return [...terms]
}

/** The doc's list of what the reviewer decides first, as it stands there. */
export function checkList(doc: string): string {
  const [, after = ""] = doc.split(/^### For moongetsu to check first\n/m)
  return after.split(/^#/m)[0].trim()
}

/** Which of `terms` a Romanian text uses: a term at a word's start, with whatever ending it took. */
function termsIn(text: string, terms: readonly string[]): string[] {
  const lower = text.toLowerCase()
  return terms.filter((term) => {
    for (let at = lower.indexOf(term); at >= 0; at = lower.indexOf(term, at + 1)) {
      if (at === 0 || !/[\p{L}\p{N}-]/u.test(lower[at - 1])) return true
    }
    return false
  })
}

/** One message of the sheet. */
export interface Row {
  ns: string
  /** The key without its namespace. */
  key: string
  english: string
  /** The translation; a stale message's old one. */
  translation: string
  note: string
  state: MetaEntry["state"]
  /** The glossary's [check] terms it uses. */
  check: string[]
}

const forms = (texts: ReadonlyMap<string, string>): [string, string][] =>
  FORM_ORDER.filter((form) => texts.has(form)).map((form) => [form, texts.get(form) ?? ""])

/** Text in a Markdown table's cell: nothing of it can end the cell or turn into markup. */
const cell = (text: string) => text.replace(/[\\|<*_`]/g, "\\$&").replace(/\r?\n/g, "<br>")

/** A message's texts for its cell: a form's name before each when it has several. */
const shown = (texts: ReadonlyMap<string, string>) =>
  forms(texts)
    .map(([form, text]) => (form ? `${form}: ${cell(text)}` : cell(text)))
    .join("<br>")

/** Every message of the English with what stands for it in `bundle` and `meta`, in the catalog's order. */
export function reviewRows(english: Bundle, bundle: Bundle, meta: Meta, notes: ReadonlyMap<string, string>, terms: readonly string[]): Row[] {
  const translated = messagesOf(bundle)
  const rows: Row[] = []
  for (const [key, source] of messagesOf(english)) {
    const entry = meta[key]
    const texts = translated.get(key)
    if (!entry || (!texts && entry.state !== "stale")) continue // the catalog test names these
    const [ns, ...path] = key.split(":")
    rows.push({
      ns,
      key: path.join(":"),
      english: shown(source),
      translation: texts ? shown(texts) : cell(entry.was ?? ""),
      note: cell(noteFor(key, notes)),
      state: entry.state,
      check: entry.state === "stale" ? [] : termsIn([...(texts?.values() ?? [])].join("\n"), terms),
    })
  }
  return rows
}

/** The review sheet: what to decide first, then every message, what waits for the reviewer before what doesn't. */
export function reviewSheet(lang: string, rows: readonly Row[], first: string): string {
  const name = new Intl.DisplayNames(["en"], { type: "language" }).of(lang) ?? lang
  const stateOf = (r: Row) => (r.state === "stale" ? "stale" : r.check.length > 0 && r.state === "draft" ? `draft [check: ${r.check.join(", ")}]` : r.state)
  const table = (title: string, list: Row[]) =>
    list.length === 0
      ? []
      : [
          `## ${title} (${list.length})`,
          "",
          `| Namespace | Key | English | ${name} | Note | State |`,
          "|---|---|---|---|---|---|",
          ...list.map((r) => `| ${r.ns} | ${cell(r.key)} | ${r.english} | ${r.translation} | ${r.note} | ${stateOf(r)} |`),
          "",
        ]
  const count = (state: Row["state"]) => rows.filter((r) => r.state === state).length
  const drafts = rows.filter((r) => r.state === "draft")
  return [
    `# ${name} review sheet`,
    "",
    `${rows.length} messages: ${count("draft")} draft, ${count("reviewed")} reviewed, ${count("stale")} stale.`,
    "",
    "Write a correction beside any line and send the sheet back to the owner. A message with several forms lists them; a `{{variable}}` and a `<tag>` stay as they are, but may move. Stale lines show the old translation of English that has since changed.",
    "",
    ...(first ? ["## Check first", "", first, ""] : []),
    ...table("Stale", rows.filter((r) => r.state === "stale")),
    ...table("Drafts with a [check] word", drafts.filter((r) => r.check.length > 0)),
    ...table("Other drafts", drafts.filter((r) => r.check.length === 0)),
    ...table("Reviewed", rows.filter((r) => r.state === "reviewed")),
  ].join("\n")
}

/** What a command changed, or why it changed nothing. */
export interface Outcome {
  /** Messages it changed. */
  done: string[]
  /** Messages a namespace named that it passed over, with why. */
  skipped: string[]
  /** Why nothing may be written: a target that isn't there, a message it can't do. */
  problems: string[]
}

/**
 * Marks `targets`, namespaces or `ns:key` messages, `reviewed`, in `meta`, and reports it. Only what the reviewer
 * saw can be: a stale message has no text, and one whose English changed was read against other words. A stale
 * message is passed over when a whole namespace is named, and refused when it is named itself.
 */
export function markReviewed(english: Bundle, meta: Meta, targets: readonly string[]): Outcome {
  const messages = messagesOf(english)
  const out: Outcome = { done: [], skipped: [], problems: [] }
  const mark = (key: string, whole: boolean) => {
    const entry = meta[key]
    const source = messages.get(key)
    if (!entry || !source) out.problems.push(`${key}: no such message`)
    else if (entry.state === "stale") {
      if (whole) out.skipped.push(`${key}: stale`)
      else out.problems.push(`${key}: stale, so it has no text to stand behind`)
    } else if (entry.en !== messageHash(source)) out.problems.push(`${key}: the English changed since it was translated`)
    else if (entry.state !== "reviewed") {
      entry.state = "reviewed"
      out.done.push(key)
    }
  }
  for (const target of targets) {
    if (target.includes(":")) mark(target, false)
    else if (target in english) {
      for (const key of messages.keys()) if (key.startsWith(`${target}:`)) mark(key, true)
    }
    else out.problems.push(`${target}: no such namespace`)
  }
  if (out.problems.length > 0) out.done = []
  return out
}

/** Takes every text of `keys` out of a language's words, and the groups that leaves empty. */
function dropTexts(words: Record<string, unknown>, path: string, keys: ReadonlySet<string>) {
  for (const [name, value] of Object.entries(words)) {
    const key = `${path}${name}`
    if (typeof value === "string") {
      if (keys.has(key.replace(FORM_SUFFIX, ""))) delete words[name]
    } else if (value && typeof value === "object") {
      dropTexts(value as Record<string, unknown>, `${key}.`, keys)
      if (Object.keys(value).length === 0) delete words[name]
    }
  }
}

/**
 * Marks the messages `keys` stale: their text leaves `bundle`, so the app shows English, and `meta` keeps it as
 * `was`, a plural message's forms one after another, with the hash of the English as it is now.
 */
export function markStale(english: Bundle, bundle: Bundle, meta: Meta, keys: readonly string[]): Outcome {
  const messages = messagesOf(english)
  const translated = messagesOf(bundle)
  const out: Outcome = { done: [], skipped: [], problems: [] }
  for (const key of keys) {
    const source = messages.get(key)
    const texts = translated.get(key)
    if (!source) out.problems.push(`${key}: no such message`)
    else if (meta[key]?.state === "stale") out.problems.push(`${key}: already stale`)
    else if (!texts) out.problems.push(`${key}: no translation to keep`)
    else out.done.push(key)
  }
  if (out.problems.length > 0) {
    out.done = []
    return out
  }
  for (const key of out.done) {
    const was = forms(translated.get(key) ?? new Map())
      .map(([, text]) => text)
      .join("\n")
    meta[key] = { state: "stale", en: messageHash(messages.get(key) ?? new Map()), was }
  }
  const doomed = new Set(out.done)
  for (const [ns, words] of Object.entries(bundle)) dropTexts(words, `${ns}:`, doomed)
  return out
}
