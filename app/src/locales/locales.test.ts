import { describe, expect, it } from "vitest"
import native from "./en/native.json"
import { en } from "./index"
import { type Bundle, catalogProblems, type Meta, messageHash, messagesOf, pluralProblems, translationProblems } from "./check"

// Every language but English: its namespaces and its meta (docs/TRANSLATING.md).
const files = import.meta.glob<Record<string, unknown>>(["./*/*.json", "!./en/*", "!./*/meta.json"], { eager: true, import: "default" })
const metas = import.meta.glob<Meta>("./*/meta.json", { eager: true, import: "default" })
const languages: Record<string, { bundle: Bundle; meta: Meta }> = {}
for (const [file, words] of Object.entries(files)) {
  const [, lang, ns] = /\.\/([^/]+)\/([^/]+)\.json$/.exec(file) ?? []
  ;(languages[lang] ??= { bundle: {}, meta: {} }).bundle[ns] = words
}
for (const [file, meta] of Object.entries(metas)) {
  const lang = /\.\/([^/]+)\//.exec(file)?.[1] ?? ""
  if (lang !== "en") (languages[lang] ??= { bundle: {}, meta: {} }).meta = meta
}

// `native` is what Rust draws itself: it is no namespace of the UI, but a language has it too.
const english = { ...en, native } as unknown as Bundle
const msgs = (words: Record<string, string>) => new Map(Object.entries(words))

describe("the catalogs", () => {
  it("has Romanian next to English", () => {
    expect(Object.keys(languages)).toEqual(["ro"])
    expect(Object.keys(languages.ro.bundle).sort()).toEqual(Object.keys(english).sort())
  })

  it("write English with plural forms one and other", () => {
    expect(pluralProblems("en", english)).toEqual([])
  })

  it("write only text a message can render, in every language", () => {
    // Empty values, tags `<Trans>` can't fill (HTML void names among them), a plugin leading its own toast.
    expect(catalogProblems(english)).toEqual([])
    for (const [lang, { bundle }] of Object.entries(languages)) expect(catalogProblems(bundle), lang).toEqual([])
  })

  for (const [lang, { bundle, meta }] of Object.entries(languages)) {
    describe(lang, () => {
      it("has the plural forms the language has", () => {
        expect(pluralProblems(lang, bundle)).toEqual([])
      })

      it("has every message of the English, and no other, with its variables and tags", () => {
        // Keys, plural forms, `{{variables, formats}}`, `$t()` references, tags and forbidden letters, against the
        // English; and a meta entry made from the English as it is now.
        expect(translationProblems(lang, english, bundle, meta)).toEqual([])
      })

      it("has a state for every message, and none for a message it doesn't have, but stale ones", () => {
        const have = new Set(messagesOf(bundle).keys())
        const entries = Object.entries(meta)
        expect(entries.filter(([key, m]) => m.state !== "stale" && !have.has(key)).map(([key]) => key)).toEqual([])
        expect([...have].filter((key) => !meta[key])).toEqual([])
        expect(entries.every(([, m]) => ["draft", "reviewed", "stale"].includes(m.state))).toBe(true)
      })
    })
  }
})

describe("what the checks refuse", () => {
  const E: Bundle = {
    cast: {
      faces_one: "{{count}} face",
      faces_other: "{{count}} faces",
      size: "{{n, number}} frames",
      rich: "<b>{{count}}</b> frames · <go>Settings</go>",
      hide: "Hide",
      hide_one: "Hide {{count}} character",
      hide_other: "Hide {{count}} characters",
      open: "Open $t(common:action.open)",
    },
  }
  const RO = {
    faces_one: "{{count}} față",
    faces_few: "{{count}} fețe",
    faces_other: "{{count}} de fețe",
    size: "{{n, number}} cadre",
    rich: "<b>{{count}}</b> cadre · <go>Setări</go>",
    hide: "Ascunde",
    hide_one: "Ascunde {{count}} personaj",
    hide_few: "Ascunde {{count}} personaje",
    hide_other: "Ascunde {{count}} de personaje",
    open: "Deschide $t(common:action.open)",
  }
  const metaFor = (state: "draft" | "reviewed" = "draft"): Meta =>
    Object.fromEntries([...messagesOf(E)].map(([key, forms]) => [key, { state, en: messageHash(forms) }]))
  const problems = (words: Record<string, string>, meta = metaFor()) => translationProblems("ro", E, { cast: words }, meta)

  it("take a whole translation", () => {
    expect(problems(RO)).toEqual([])
    expect(problems(RO, metaFor("reviewed"))).toEqual([])
    expect(pluralProblems("ro", { cast: RO })).toEqual([])
  })

  it("name a message the Romanian lacks, or the English hasn't", () => {
    const { size: _, ...without } = RO
    expect(problems(without)).toEqual(["cast:size: missing"])
    expect(problems({ ...RO, extra: "Ceva" })).toEqual(["cast:extra: not in the English"])
  })

  it("name a missing plural form and one the language doesn't have", () => {
    const { faces_few: _, ...noFew } = RO
    expect(pluralProblems("ro", { cast: noFew })).toEqual(["cast:faces: plural forms one other, ro has few one other"])
    expect(pluralProblems("en", { cast: RO })).toEqual([
      "cast:faces: plural forms few one other, en has one other",
      "cast:hide: plural forms few one other, en has one other",
    ])
    expect(pluralProblems("ro", { cast: { x_one: "{{count}} a", x_few: "{{count}} b", x_other: "{{count}} c", x_many: "d" } })).toEqual([
      "cast:x: plural forms few many one other, ro has few one other",
    ])
  })

  it("name another variable, a lost format and a lost reference", () => {
    expect(problems({ ...RO, faces_few: "{{total}} fețe" })).toEqual(["cast:faces_few: variables are [{{total}}], the English's [{{count}}]"])
    expect(problems({ ...RO, size: "{{n}} cadre" })).toEqual(["cast:size: variables are [{{n}}], the English's [{{n,number}}]"])
    expect(problems({ ...RO, size: "{{ n,  number }} cadre" })).toEqual([])
    expect(problems({ ...RO, open: "Deschide" })).toEqual(["cast:open: variables are [], the English's [$t(common:action.open)]"])
    // A plural form is read against the English form of its name, `other` when it has none.
    expect(problems({ ...RO, hide_one: "Ascunde personajul" })).toEqual(["cast:hide_one: variables are [], the English's [{{count}}]"])
  })

  it("name a lost, added or renamed tag", () => {
    expect(problems({ ...RO, rich: "{{count}} cadre · Setări" })).toEqual(["cast:rich: tags are [], the English's [</b> </go> <b> <go>]"])
    expect(problems({ ...RO, rich: "<b>{{count}}</b> cadre · <x>Setări</x>" })).toEqual([
      "cast:rich: tags are [</b> </x> <b> <x>], the English's [</b> </go> <b> <go>]",
    ])
  })

  it("refuse the cedilla and take the comma below", () => {
    // Written with escapes so no editor can turn them into each other.
    const cedilla = ["ş", "ţ", "Ş", "Ţ"]
    for (const c of cedilla) expect(problems({ ...RO, hide: `Ascunde ${c}` })).toEqual(["cast:hide: cedilla, Romanian writes the comma below (ș ț Ș Ț)"])
    expect(problems({ ...RO, hide: "Ascunde ș ț Ș Ț" })).toEqual([])
    // Only Romanian is held to it.
    expect(translationProblems("tr", E, { cast: { ...RO, hide: "Gizle ş" } }, metaFor())).toEqual([])
  })

  it("compare the hash of the English each entry was made from", () => {
    const meta = metaFor()
    meta["cast:size"] = { state: "draft", en: "00000000" }
    expect(problems(RO, meta)).toEqual([
      `cast:size: the English changed (meta 00000000, now ${messageHash(msgs({ "": "{{n, number}} frames" }))}): update the translation and its hash, or mark it stale`,
    ])
    // Plural forms: an edit to either English form changes the hash, and a new form does too.
    const [one, other] = [{ one: "{{count}} face", other: "{{count}} faces" }, { one: "{{count}} face", other: "{{count}} faces!" }].map(msgs)
    expect(messageHash(one)).not.toBe(messageHash(other))
    expect(messageHash(msgs({ one: "{{count}} face", other: "x" }))).not.toBe(messageHash(msgs({ one: "{{count}} face" })))
    expect(messageHash(msgs({ "": "a", other: "b" }))).not.toBe(messageHash(msgs({ "": "b", other: "a" })))
    // It is eight letters and the same for the same text, whatever order the forms were written in.
    expect(messageHash(msgs({ one: "a", other: "b" }))).toMatch(/^[0-9a-f]{8}$/)
    expect(messageHash(msgs({ one: "a", other: "b" }))).toBe(messageHash(msgs({ other: "b", one: "a" })))
  })

  it("know a message with no meta entry, an unknown one and a state that isn't one", () => {
    const { "cast:size": _, ...short } = metaFor()
    expect(problems(RO, short)).toEqual(["cast:size: no meta entry"])
    expect(problems(RO, { ...metaFor(), "cast:other": { state: "draft", en: "00000000" } })).toEqual(["cast:other: meta entry for a key the English hasn't"])
    expect(problems(RO, { ...metaFor(), "cast:size": { state: "final" as "draft", en: messageHash(msgs({ "": "{{n, number}} frames" })) } })).toEqual([
      'cast:size: meta state "final"',
    ])
  })

  it("let a stale message fall back to English, and only that", () => {
    const { size: _, ...without } = RO
    const stale = (was?: string): Meta => ({
      ...metaFor(),
      "cast:size": { state: "stale", en: messageHash(msgs({ "": "{{n, number}} frames" })), ...(was ? { was } : {}) },
    })
    expect(problems(without, stale("{{n, number}} cadre"))).toEqual([])
    expect(problems(without, stale())).toEqual(["cast:size: stale without its old text (was)"])
    expect(problems(RO, stale("{{n, number}} cadre"))).toEqual(["cast:size: stale, so it has no text"])
    expect(problems(without, { ...stale("x"), "cast:size": { state: "stale", en: "00000000", was: "x" } })).toEqual([
      `cast:size: the English changed (meta 00000000, now ${messageHash(msgs({ "": "{{n, number}} frames" }))}): update the translation and its hash, or mark it stale`,
    ])
    expect(problems(RO, { ...metaFor(), "cast:size": { state: "draft", en: messageHash(msgs({ "": "{{n, number}} frames" })), was: "x" } })).toEqual([
      "cast:size: draft with an old text (was)",
    ])
  })

  it("take a plain text beside plural forms, and refuse one without the other's twin", () => {
    const { hide: _, ...noPlain } = RO
    expect(problems(noPlain)).toEqual(["cast:hide: a text and plural forms, the English has other ones"])
    const { hide_one: _1, hide_few: _2, hide_other: _3, ...noPlural } = RO
    expect(problems(noPlural)).toEqual(["cast:hide: a text and plural forms, the English has other ones"])
  })
})
