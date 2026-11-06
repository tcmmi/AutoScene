import { describe, expect, it } from "vitest"
import native from "./en/native.json"
import { en } from "./index"
import { type Bundle, type Meta, messageHash, messagesOf, translationProblems } from "./check"
import { checkList, checkTerms, markReviewed, markStale, noteFor, notesOf, reviewRows, reviewSheet } from "./review"

const E: Bundle = {
  cast: {
    title: "Cast",
    faces_one: "{{count}} face",
    faces_other: "{{count}} faces",
    hide: "Hide",
    hide_one: "Hide {{count}} character",
    hide_other: "Hide {{count}} characters",
    deep: { only: "Only | one\nline <b>x</b>" },
  },
  pack: { shots: "Shots" },
}
const RO = () =>
  ({
    cast: {
      title: "Distribuție",
      faces_one: "{{count}} față",
      faces_few: "{{count}} fețe",
      faces_other: "{{count}} de fețe",
      hide: "Ascunde",
      hide_one: "Ascunde {{count}} personaj",
      hide_few: "Ascunde {{count}} personaje",
      hide_other: "Ascunde {{count}} de personaje",
      deep: { only: "Doar <b>x</b>" },
    },
    pack: { shots: "Planuri" },
  }) as Bundle
const META = (state: "draft" | "reviewed" = "draft"): Meta =>
  Object.fromEntries([...messagesOf(E)].map(([key, forms]) => [key, { state, en: messageHash(forms) }]))

const CATALOG = `// The cast namespace.
export default {
  /** The page's title. “Quoted”, so an offset past a wide letter still lands. */
  title: "Cast",
  /** A number of things. */
  count: {
    /**
     * How many faces:
     * a badge.
     */
    faces_one: "{{count}} face",
    faces_other: "{{count}} faces",
    shots: "Shots",
  },
  untold: "No note",
  // A line of its own.
  line: "Line",
} as const
`

describe("a translator's notes", () => {
  const notes = notesOf("cast", CATALOG)

  it("are the comment above a key, one for all its plural forms", () => {
    expect(notes.get("cast:title")).toBe("The page's title. “Quoted”, so an offset past a wide letter still lands.")
    expect(notes.get("cast:count.faces")).toBe("How many faces: a badge.")
    expect(notes.get("cast:line")).toBe("A line of its own.")
  })

  it("fall back to the group's, and a key with neither has none", () => {
    expect(notes.get("cast:count.shots")).toBe("A number of things.")
    expect(notes.has("cast:untold")).toBe(false)
  })

  it("say where a message of the JSON catalogs shows", () => {
    expect(noteFor("native:menu.file.quit", new Map())).toContain("macOS menu")
    expect(noteFor("backend:job.stage.indexing", new Map())).toContain("stage")
    expect(noteFor("backend:job.indexing", new Map())).toContain("title")
    expect(noteFor("backend:db.full", new Map())).toContain("backend")
    expect(noteFor("cast:untold", new Map())).toBe("")
  })
})

describe("the glossary's [check] words", () => {
  const doc = `# Translating

| Not | the glossary **[check]** | here |
|---|---|---|

## Romanian

| English | Romanian (plural) | Note |
|---|---|---|
| shot | **plan** (planuri) | **[check]** the film term |
| in / out | **intrare / ieșire** | **[check]** |
| timeline | timeline (timeline-uri) | stays **[check]** |
| footage | material | Live action \`Cu actori reali\` **[check]**, Animation \`Animație\` |
| scene | scenă (scene) | |

Sizes, with badges **[check]**:

| English | Romanian | Badge |
|---|---|---|
| Close-up | Prim-plan | PP |

### For moongetsu to check first

1. **shot = plan.** Your ear decides.

### After it
`

  it("come from the Romanian section's marked rows and tables, and only from what the mark is on", () => {
    expect(checkTerms(doc)).toEqual(["plan", "planuri", "intrare", "ieșire", "timeline", "timeline-uri", "cu actori reali", "prim-plan"])
  })

  it("come with the list the reviewer decides first", () => {
    expect(checkList(doc)).toBe("1. **shot = plan.** Your ear decides.")
    expect(checkList("# No list")).toBe("")
  })
})

describe("the sheet", () => {
  const notes = new Map([["cast:title", "The page's title."]])
  const rows = (meta = META(), bundle = RO()) => reviewRows(E, bundle, meta, notes, ["plan", "planuri"])
  const sheet = (meta = META(), bundle = RO()) => reviewSheet("ro", rows(meta, bundle), "1. **plan.**")
  const lines = (text: string) => text.split("\n")

  it("has one row of namespace, key, English, translation, note and state per message", () => {
    const text = sheet()
    expect(text).toContain("| Namespace | Key | English | Romanian | Note | State |")
    expect(text).toContain("| cast | title | Cast | Distribuție | The page's title. | draft |")
    expect(lines(text).filter((l) => l.startsWith("| cast |") || l.startsWith("| pack |"))).toHaveLength(messagesOf(E).size)
  })

  it("lists a message's forms in one cell, and keeps a cell one cell", () => {
    const text = sheet()
    expect(text).toContain("one: {{count}} face<br>other: {{count}} faces | one: {{count}} față<br>few: {{count}} fețe<br>other: {{count}} de fețe")
    expect(text).toContain("one: Hide {{count}} character<br>other: Hide {{count}} characters<br>Hide |")
    expect(text).toContain(String.raw`| Only \| one<br>line \<b>x\</b> | Doar \<b>x\</b> |`)
  })

  it("puts what to decide first on top, then stale, then drafts with a [check] word, then drafts, then reviewed", () => {
    const meta = META()
    meta["cast:title"].state = "reviewed"
    const ro = RO()
    const stale = { state: "stale", en: meta["cast:hide"].en, was: "Ascunde" } as const
    meta["cast:hide"] = stale
    for (const k of ["hide", "hide_one", "hide_few", "hide_other"]) delete ro.cast[k]
    const titles = lines(reviewSheet("ro", rows(meta, ro), "1. **plan.**")).filter((l) => l.startsWith("## "))
    expect(titles).toEqual(["## Check first", "## Stale (1)", "## Drafts with a [check] word (1)", "## Other drafts (2)", "## Reviewed (1)"])
    const text = reviewSheet("ro", rows(meta, ro), "")
    expect(text).toContain("| pack | shots | Shots | Planuri |  | draft [check: plan, planuri] |")
    // A stale message shows the words it had, and no English to hold them against but the new one.
    expect(text).toContain("<br>Hide | Ascunde |  | stale |")
    expect(text).not.toContain("## Check first")
  })

  it("finds a [check] word with its endings, at a word's start only", () => {
    const ro = RO()
    ro.pack.shots = "Planul și replanificare"
    expect(rows(META(), ro).find((r) => r.key === "shots")?.check).toEqual(["plan"])
    ro.pack.shots = "Replanificare"
    expect(rows(META(), ro).find((r) => r.key === "shots")?.check).toEqual([])
  })

  it("leaves out nothing of the real catalogs, and they are all drafts or better", () => {
    const english = { ...en, native } as unknown as Bundle
    const files = import.meta.glob<Record<string, unknown>>(["./ro/*.json", "!./ro/meta.json"], { eager: true, import: "default" })
    const ro = Object.fromEntries(Object.entries(files).map(([file, words]) => [file.replace(/^.*\/(.+)\.json$/, "$1"), words])) as Bundle
    const meta = import.meta.glob<Meta>("./ro/meta.json", { eager: true, import: "default" })["./ro/meta.json"]
    const list = reviewRows(english, ro, meta, new Map(), [])
    expect(list).toHaveLength(messagesOf(english).size)
    expect(list.filter((r) => r.translation === "")).toEqual([])
    // Without the English's comments, only the namespaces that can hold none have a note.
    expect(list.filter((r) => (r.note !== "") !== (r.ns === "backend" || r.ns === "native"))).toEqual([])
  })
})

describe("marking messages reviewed", () => {
  it("takes a namespace and a message, and what stays a draft stays one", () => {
    const meta = META()
    const out = markReviewed(E, meta, ["pack", "cast:faces"])
    expect(out).toEqual({ done: ["pack:shots", "cast:faces"], skipped: [], problems: [] })
    expect(meta["pack:shots"].state).toBe("reviewed")
    expect(meta["cast:faces"].state).toBe("reviewed")
    expect(meta["cast:title"].state).toBe("draft")
    expect(translationProblems("ro", E, RO(), meta)).toEqual([])
  })

  it("changes nothing to what already is", () => {
    expect(markReviewed(E, META("reviewed"), ["cast"]).done).toEqual([])
  })

  it("refuses what isn't there, and what was read against other English", () => {
    expect(markReviewed(E, META(), ["cast:nope"]).problems).toEqual(["cast:nope: no such message"])
    expect(markReviewed(E, META(), ["nope"]).problems).toEqual(["nope: no such namespace"])
    const meta = META()
    meta["cast:title"].en = "00000000"
    const out = markReviewed(E, meta, ["pack", "cast:title"])
    expect(out.problems).toEqual(["cast:title: the English changed since it was translated"])
    // A refusal is the whole command's: the script writes nothing.
    expect(out.done).toEqual([])
  })

  it("passes a stale message over in a namespace, and refuses it named", () => {
    const meta = META()
    meta["cast:title"] = { ...meta["cast:title"], state: "stale", was: "Distribuție" }
    const whole = markReviewed(E, meta, ["cast"])
    expect(whole.skipped).toEqual(["cast:title: stale"])
    expect(whole.problems).toEqual([])
    expect(meta["cast:title"].state).toBe("stale")
    expect(markReviewed(E, meta, ["cast:title"]).problems).toEqual(["cast:title: stale, so it has no text to stand behind"])
  })
})

describe("marking messages stale", () => {
  const changed = { ...E, cast: { ...E.cast, faces_other: "{{count}} other faces" } }

  it("moves the text into the entry, every form and the plain one, and the catalog test takes it", () => {
    const ro = RO()
    const meta = META()
    const out = markStale(changed, ro, meta, ["cast:faces", "cast:hide"])
    expect(out).toEqual({ done: ["cast:faces", "cast:hide"], skipped: [], problems: [] })
    expect(Object.keys(ro.cast)).toEqual(["title", "deep"])
    expect(meta["cast:faces"]).toEqual({
      state: "stale",
      en: messageHash(messagesOf(changed).get("cast:faces") ?? new Map()),
      was: "{{count}} față\n{{count}} fețe\n{{count}} de fețe",
    })
    expect(meta["cast:hide"].was).toBe("Ascunde {{count}} personaj\nAscunde {{count}} personaje\nAscunde {{count}} de personaje\nAscunde")
    expect(translationProblems("ro", changed, ro, meta)).toEqual([])
  })

  it("leaves no empty group behind", () => {
    const ro = RO()
    markStale(E, ro, META(), ["cast:deep.only", "pack:shots"])
    expect(ro.cast.deep).toBeUndefined()
    expect(ro.pack).toEqual({})
  })

  it("refuses what it can't keep, and then touches nothing", () => {
    const ro = RO()
    const meta = META()
    meta["pack:shots"] = { ...meta["pack:shots"], state: "stale", was: "Planuri" }
    const { shots: _, ...left } = ro.pack
    ro.pack = left
    const out = markStale(E, ro, meta, ["cast:title", "cast:nope", "pack:shots"])
    expect(out.problems).toEqual(["cast:nope: no such message", "pack:shots: already stale"])
    expect(out.done).toEqual([])
    expect(ro.cast.title).toBe("Distribuție")
    expect(meta["cast:title"].state).toBe("draft")
    const bare = RO()
    delete bare.cast.title
    expect(markStale(E, bare, META(), ["cast:title"]).problems).toEqual(["cast:title: no translation to keep"])
  })
})
