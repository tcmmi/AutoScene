// Checks a catalog's values can render, and that a translation is the English's
// translation: tags `<Trans>` can fill, no empty text, a plugin can't lead its
// own toast (`catalogProblems`), the plural forms a language has, the keys,
// variables and tags of the English, and the hash of the English each entry
// was made from (`translationProblems`; docs/DESIGN.md, Copy, and
// docs/TRANSLATING.md).

/** Tag names a message may use for `<Trans components>`. An HTML void name (`<link>`, `<br>`) would render empty. */
export const TRANS_TAGS = ["b", "strong", "em", "mono", "key", "x", "go"] as const

/**
 * Names a message may use only self-closed (`<what/>`), where the code places a word that is not its own to
 * translate and that may hold anything, `{{braces}}` included: a plugin's name, a repository's path. A
 * `{{value}}` would be filled in twice by `<Trans>`, and so change such a word.
 */
export const SLOT_TAGS = ["what", "repo", "commit", "path"] as const

/** Toasts that say a plugin's words: the key, with its namespace first. */
const PLUGIN_TOAST = /^(backend\.plugin\.pageToast$|plugins\.toast\.\w+\.)/

/**
 * What lets a plugin pass for AutoScene in the toast `value`. A plugin may be
 * called anything, so the toast opens with words it can't choose, then its
 * name before anything else of its own. An `anonymous` toast has no name.
 */
function toastProblems(value: string, key: string): string[] {
  const problems: string[] = []
  if (!/^[^{$]*\p{L}/u.test(value)) problems.push(`${key}: starts with a plugin's words, not with fixed text`)
  const [first] = [...value.matchAll(/\{\{\s*(\w+)/g)].map((m) => m[1])
  if (!key.includes(".anonymous") && first !== "name") problems.push(`${key}: {{name}} must come before any other variable`)
  return problems
}

/**
 * Every problem in a catalog's values, as `"key: what"`. Give it a language's
 * whole catalog, `{ backend: …, plugins: … }`, so a key has its namespace.
 */
export function catalogProblems(bundle: unknown, prefix = ""): string[] {
  if (typeof bundle === "string") {
    const problems: string[] = []
    if (bundle.trim() === "") problems.push(`${prefix}: empty`)
    for (const [tag, name] of bundle.matchAll(/<\/?([A-Za-z][\w-]*)[^>]*>/g)) {
      const slot = tag.endsWith("/>") && (SLOT_TAGS as readonly string[]).includes(name)
      if (!slot && !(TRANS_TAGS as readonly string[]).includes(name)) problems.push(`${prefix}: <${name}> is not a Trans tag (${TRANS_TAGS.join(" ")})`)
    }
    if (PLUGIN_TOAST.test(prefix)) problems.push(...toastProblems(bundle, prefix))
    return problems
  }
  if (bundle && typeof bundle === "object") {
    return Object.entries(bundle).flatMap(([k, v]) => catalogProblems(v, prefix ? `${prefix}.${k}` : k))
  }
  return [`${prefix}: not text`]
}

type Words = Record<string, unknown>
/** A language's catalog: `{ cast: { … }, backend: { … } }`. */
export type Bundle = Record<string, Words>

/** What `ro/meta.json` says of one message: who stands behind it, and the English it was made from. */
export interface MetaEntry {
  state: "draft" | "reviewed" | "stale"
  /** `messageHash` of the English when the translation was made or checked. */
  en: string
  /** A stale entry's old translation, kept for the translator; the catalog has no text for it, so the app shows English. */
  was?: string
}
export type Meta = Record<string, MetaEntry>

const FORMS = ["zero", "one", "two", "few", "many", "other"]
const FORM_SUFFIX = /_(zero|one|two|few|many|other)$/

/** Every message of a catalog by `ns:path`, its texts by form: `""` for a plain text, else the plural form's name. */
export function messagesOf(bundle: Bundle): Map<string, Map<string, string>> {
  const out = new Map<string, Map<string, string>>()
  const walk = (value: unknown, key: string) => {
    if (typeof value === "string") {
      const form = FORM_SUFFIX.exec(key)?.[1] ?? ""
      const base = form ? key.slice(0, -form.length - 1) : key
      if (!out.has(base)) out.set(base, new Map())
      out.get(base)?.set(form, value)
    } else if (value && typeof value === "object") {
      for (const [k, v] of Object.entries(value)) walk(v, key.endsWith(":") ? `${key}${k}` : `${key}.${k}`)
    }
  }
  for (const [ns, words] of Object.entries(bundle)) walk(words, `${ns}:`)
  return out
}

/**
 * The 8 letters a translation's meta entry holds of its English: FNV-1a over the message's texts, plural
 * forms in the order of the CLDR (`one`, then `other`), so an edit to any of them changes it.
 */
export function messageHash(forms: ReadonlyMap<string, string>): string {
  const text = FORMS.filter((f) => forms.has(f))
    .map((f) => forms.get(f))
    .concat(forms.get("") ?? [])
    .join("\n")
  let h = 0x811c9dc5
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193)
  return (h >>> 0).toString(16).padStart(8, "0")
}

/** Every `{{variable, format}}` and `$t(key)` of a text, written the same way for any spacing, in a fixed order. */
const placeholders = (text: string): string =>
  (text.match(/\{\{[^}]*\}\}|\$t\([^)]*\)/g) ?? [])
    .map((p) => p.replace(/\s+/g, ""))
    .sort()
    .join(" ")
/** Every tag of a text, `<b>`, `</b>`, `<what/>`, in a fixed order. */
const tagsOf = (text: string): string => (text.match(/<\/?[A-Za-z][\w-]*\s*\/?>/g) ?? []).sort().join(" ")

/**
 * Plural forms: a message with plural forms has exactly the language's (`Intl.PluralRules`: en `one`, `other`;
 * ro `one`, `few`, `other`). A plain text may sit beside them under the same key (`hide`, `hide_one`): it is
 * what `t()` reads without a count.
 */
export function pluralProblems(lang: string, bundle: Bundle): string[] {
  const want = new Intl.PluralRules(lang).resolvedOptions().pluralCategories.toSorted()
  const problems: string[] = []
  for (const [key, forms] of messagesOf(bundle)) {
    const have = [...forms.keys()].filter((f) => f !== "").toSorted()
    if (have.length > 0 && have.join() !== want.join()) problems.push(`${key}: plural forms ${have.join(" ")}, ${lang} has ${want.join(" ")}`)
  }
  return problems
}

/**
 * One English line said two ways, as `"English": one / other`: the rule of one thing, one word
 * (docs/TRANSLATING.md). Plural forms are held to their own form. `allowed` holds the English lines that are
 * two things in two places: a verb and a noun, a word that agrees with what it stands beside.
 */
export function sameEnglishProblems(english: Bundle, bundle: Bundle, allowed: ReadonlySet<string>): string[] {
  const translated = messagesOf(bundle)
  const said = new Map<string, Set<string>>()
  for (const [key, forms] of messagesOf(english)) {
    for (const [form, text] of forms) {
      const translation = translated.get(key)?.get(form)
      if (translation === undefined || allowed.has(text)) continue
      const id = JSON.stringify([form, text])
      if (!said.has(id)) said.set(id, new Set())
      said.get(id)?.add(translation)
    }
  }
  return [...said]
    .filter(([, translations]) => translations.size > 1)
    .map(([id, translations]) => `${JSON.stringify(JSON.parse(id)[1])}: ${[...translations].join(" / ")}`)
}

/** Letters a language never writes: Romanian has the comma below (ș ț), never the cedilla of old fonts (ş ţ). */
const FORBIDDEN: Record<string, { letters: RegExp; why: string }> = {
  ro: { letters: /[\u015e\u015f\u0162\u0163]/, why: "cedilla, Romanian writes the comma below (ș ț Ș Ț)" },
}

/**
 * What a translation breaks against the English it answers, as `"ns:key: what"`: a key the English has and it
 * hasn't (unless `meta` marks it stale, and then it has no text), a key the English hasn't, other variables,
 * formats or tags, forbidden letters, and a `meta` entry that is missing, unknown or made from other English.
 * A plural form is held to the English form of its name, `other` when the English has none (`few`).
 */
export function translationProblems(lang: string, english: Bundle, bundle: Bundle, meta: Meta): string[] {
  const en = messagesOf(english)
  const tr = messagesOf(bundle)
  const problems: string[] = []
  for (const [key, forms] of tr) {
    const source = en.get(key)
    if (!source) {
      problems.push(`${key}: not in the English`)
      continue
    }
    const shapes = (m: ReadonlyMap<string, string>) => `${m.has("")} ${[...m.keys()].some((f) => f !== "")}`
    if (shapes(forms) !== shapes(source)) {
      problems.push(`${key}: a text and plural forms, the English has other ones`)
      continue
    }
    for (const [form, text] of forms) {
      const english = source.get(form) ?? source.get("other") ?? ""
      const at = form ? `${key}_${form}` : key
      if (placeholders(text) !== placeholders(english)) problems.push(`${at}: variables are [${placeholders(text)}], the English's [${placeholders(english)}]`)
      if (tagsOf(text) !== tagsOf(english)) problems.push(`${at}: tags are [${tagsOf(text)}], the English's [${tagsOf(english)}]`)
      const bad = FORBIDDEN[lang]
      if (bad?.letters.test(text)) problems.push(`${at}: ${bad.why}`)
    }
  }
  for (const [key, source] of en) {
    const entry = meta[key]
    const has = tr.has(key)
    if (!has && entry?.state !== "stale") problems.push(`${key}: missing`)
    if (!entry) {
      if (has) problems.push(`${key}: no meta entry`)
      continue
    }
    if (entry.state === "stale") {
      if (has) problems.push(`${key}: stale, so it has no text`)
      if (typeof entry.was !== "string" || entry.was.trim() === "") problems.push(`${key}: stale without its old text (was)`)
    } else if (entry.state !== "draft" && entry.state !== "reviewed") {
      problems.push(`${key}: meta state ${JSON.stringify(entry.state)}`)
    } else if (entry.was !== undefined) problems.push(`${key}: ${entry.state} with an old text (was)`)
    const now = messageHash(source)
    if (entry.en !== now) problems.push(`${key}: the English changed (meta ${entry.en}, now ${now}): update the translation and its hash, or mark it stale`)
  }
  for (const key of Object.keys(meta)) if (!en.has(key)) problems.push(`${key}: meta entry for a key the English hasn't`)
  return problems
}
