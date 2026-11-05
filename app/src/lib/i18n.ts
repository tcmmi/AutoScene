// The UI's words and how it writes numbers, dates and lists (docs/DESIGN.md, Copy).
//
// English is bundled and initialised synchronously on import, so the first
// paint and every test render have it with no provider and no flash. Another
// language loads its catalog on demand (`setLanguage`). Code outside React
// (toasts, palette data, lib/) calls `t` at the moment it shows the words,
// never at module scope.
import i18next, { type FormatterModule } from "i18next"
import { createElement, Fragment, type ReactNode, useSyncExternalStore } from "react"
import { initReactI18next } from "react-i18next"
import { commands, type Msg } from "@/bindings"
import { en, loadLanguage, NAMESPACES } from "@/locales"

/** What i18n can load. Settings offers `OFFERED`; the pseudo-locale is for development only. */
export const LANGUAGES = ["en", "ro"] as const
export type Lang = (typeof LANGUAGES)[number]
/** What Settings offers. Romanian joins once its catalog is complete (#17); i18n itself resolves every language it has. */
export const OFFERED: readonly Lang[] = ["en"]
/** Accented, 35 % longer English in brackets: whatever shows plain is still hardcoded (dev and the mock, `?lang=pseudo`). */
export const PSEUDO = "en-XA"

const STRICT = import.meta.env.DEV

// Dates and numbers follow the UI language, not the OS region: English
// writes them the way it always has (en-GB: "13 Oct, 22:13", "14,200").
function tagOf(lng: string | undefined): string {
  return lng === "ro" ? "ro-RO" : "en-GB"
}

const cache = new Map<string, unknown>()
function cached<T>(key: string, make: () => T): T {
  let v = cache.get(key) as T | undefined
  if (v === undefined) cache.set(key, (v = make()))
  return v
}
const numberFmt = (tag: string, o: Intl.NumberFormatOptions = {}) =>
  cached(`n|${tag}|${JSON.stringify(o)}`, () => new Intl.NumberFormat(tag, o))

// Every `{{variable}}` passes through here. Only `count` and variables written
// `{{x, number}}` are grouped by the language ("14,200" / "14.200"); a year, a
// resolution or a frame number keeps its digits. An object is a mistake (a
// `Msg` handed to `t()` past the types) and fails loudly while developing
// instead of reading "[object Object]".
const formatter: FormatterModule = {
  type: "formatter",
  init() {},
  add() {},
  addCached() {},
  format(value, format, lng, options) {
    if (STRICT && value !== null && typeof value === "object") {
      throw new TypeError(`i18n: {{${options?.interpolationkey ?? "?"}}} got an object; pass a string or a number`)
    }
    if (typeof value === "number" && (options?.interpolationkey === "count" || format?.startsWith("number"))) {
      return numberFmt(tagOf(lng)).format(value)
    }
    return value
  },
}

const ACCENTS: Record<string, string> = Object.fromEntries(
  [..."abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"].map((c, i) => [c, [..."åƀçðéƒĝĥîĵķļɱñöþǫŕšţûṽŵẋýžÅƁÇÐÉƑĜĤÎĴĶĻṀÑÖÞǪŔŠŢÛṼŴẊÝŽ"][i]]),
)

/**
 * "Hide {{count}} characters" → "[Ĥîðé {{count}} çĥåŕåçţéŕš ·········]": a catalog text
 * accented, longer and in brackets. `{{variables}}`, `$t(nesting)` and the
 * `<tags>` of `<Trans>` stay as they are.
 */
export function pseudo(text: string): string {
  const body = text
    .split(/(<[^>]*>|\{\{[^}]*\}\}|\$t\([^)]*\))/)
    .map((part, i) => (i % 2 === 1 ? part : [...part].map((c) => ACCENTS[c] ?? c).join("")))
    .join("")
  const pad = Math.ceil(text.length * 0.35)
  return `[${body}${pad > 0 ? ` ${"·".repeat(pad)}` : ""}]`
}

// The pseudo-locale is the English catalog, accented; built from the
// catalog, not from what t() returns, so the values a message gets (a
// character's name, a plugin's title, a fragment the code still builds) stay
// plain and show for what they are.
function pseudoBundle(v: unknown): unknown {
  if (typeof v === "string") return pseudo(v)
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, pseudoBundle(x)]))
  return v
}

export const i18n = i18next.createInstance()
void i18n
  .use(formatter)
  .use(initReactI18next)
  .init({
    lng: "en",
    fallbackLng: "en",
    // No `supportedLngs`: it would turn a loaded language back into English.
    resources: { en },
    ns: NAMESPACES,
    defaultNS: "common",
    initAsync: false,
    returnNull: false,
    // React escapes what it renders; the formatter decides per variable.
    interpolation: { escapeValue: false, alwaysFormat: true },
  })

/** The UI's words, for code outside components: call it when the words are shown. */
export const t: typeof i18n.t = i18n.t.bind(i18n) as typeof i18n.t

type Say = (key: string, options: object) => string

/**
 * What Rust said, in the UI's language: a `Msg` through the `backend` catalog,
 * its values said first. Words AutoScene didn't write (no key), a key the
 * catalog doesn't have and a plain string stay as they came.
 */
export function say(m: Msg | string): string {
  if (typeof m === "string") return m
  if (!m.key) return m.text
  const said = Object.entries(m.args ?? {}).map(([name, v]) => [name, v !== null && typeof v === "object" ? say(v) : v])
  const replace: Record<string, unknown> = Object.fromEntries(said)
  // In `replace`, a value can't be taken for an option of `t`, whatever it is called.
  const options = { replace, count: replace.count as number | undefined }
  const key = `backend:${m.key}`
  return i18n.exists(key, options) ? (i18n.t as unknown as Say)(key, options) : m.text
}

let switches = 0

/**
 * Loads the language's catalog, then switches every subscribed component and
 * `<html lang>`. When a newer switch starts while this one still loads, the
 * newer one wins and this one changes nothing.
 */
export async function setLanguage(lng: Lang | typeof PSEUDO): Promise<void> {
  const mine = ++switches
  const bundles = lng === PSEUDO ? (pseudoBundle(en) as Record<string, object>) : await loadLanguage(lng)
  if (mine !== switches) return
  // Modules load once (the import cache); adding them again is cheap, and
  // never overwrites: a test's fixture words, added before, win.
  for (const [ns, bundle] of Object.entries(bundles)) i18n.addResourceBundle(lng, ns, bundle, true, false)
  await i18n.changeLanguage(lng)
  if (mine !== switches) return
  document.documentElement.lang = lng === PSEUDO ? "en" : lng
}

/** `?lang=pseudo` (or `ro`) while developing; otherwise nothing. */
function askedInDevelopment(): Lang | typeof PSEUDO | null {
  if (!import.meta.env.DEV) return null
  const asked = new URLSearchParams(window.location.search).get("lang")
  if (asked === "pseudo") return PSEUDO
  return LANGUAGES.find((l) => l === asked) ?? null
}

let asks = 0

/**
 * Speaks what the saved settings come to, at once and with no toast: the
 * screen switching says it. On any error the language in use stays. Always
 * goes through `setLanguage`, also for the language in use: that is what
 * stops a language still loading. When replies cross, the newest ask counts.
 */
export async function applySettings(): Promise<void> {
  const mine = ++asks
  try {
    const lng = askedInDevelopment() ?? (await commands.uiLocale()).resolved
    if (mine === asks) await setLanguage(lng)
  } catch (e) {
    console.error("Couldn't load the language; staying with the one in use", e)
  }
}

/** Before the first render: the language to start in. An error doesn't stop the start: AutoScene stays English. */
export async function startLanguage(): Promise<void> {
  document.documentElement.lang = "en"
  await applySettings()
}

function subscribe(onChange: () => void) {
  i18n.on("languageChanged", onChange)
  return () => i18n.off("languageChanged", onChange)
}

/** The current language, re-rendering on a switch. */
export function useLanguage(): string {
  return useSyncExternalStore(subscribe, () => i18n.language)
}

/**
 * Remounts what it wraps when the language changes, so memoised components
 * and helpers that call the global `t` during render speak the new language
 * too. Wrapping the router keeps its location and the query cache; local
 * state (an open menu, a scroll position) resets, which is fine for a choice
 * made once, in Settings.
 */
export function LanguageRoot({ children }: { children: ReactNode }) {
  return createElement(Fragment, { key: useLanguage() }, children)
}

const BYTE_UNITS = ["b", "kb", "mb", "gb", "tb"] as const

/** Numbers, sizes, dates, lists and times in the UI language. English output is what the app always wrote. */
export const fmt = {
  /** 14200 → "14,200" (ro: "14.200"). */
  count(n: number): string {
    return numberFmt(tagOf(i18n.language), { maximumFractionDigits: 0 }).format(n)
  },
  /** 7.46, 1 → "7.5" (ro: "7,5"); never grouped, as sizes and paces never were. `digits` exact unless `upTo`. */
  decimal(v: number, digits: number, upTo = false): string {
    const o = { minimumFractionDigits: upTo ? 0 : digits, maximumFractionDigits: digits, useGrouping: false }
    return numberFmt(tagOf(i18n.language), o).format(v)
  },
  /** 1536 → "1.5 KB"; nothing or less → "0 B". */
  bytes(bytes: number, digits = 1): string {
    if (!Number.isFinite(bytes) || bytes <= 0) return t("common:format.bytes.b", { value: fmt.decimal(0, 0) })
    const i = Math.min(BYTE_UNITS.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)))
    return t(`common:format.bytes.${BYTE_UNITS[i]}`, { value: fmt.decimal(bytes / 1024 ** i, i === 0 ? 0 : digits) })
  },
  /** Unix ms → "13 Oct, 19:42". */
  dateTime(ms: number): string {
    const tag = tagOf(i18n.language)
    const o: Intl.DateTimeFormatOptions = { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }
    return cached(`d|${tag}`, () => new Intl.DateTimeFormat(tag, o)).format(new Date(ms))
  },
  /** ["a", "b", "c"] → "a, b and c" (ro: "a, b și c"). */
  list(items: readonly string[]): string {
    const tag = tagOf(i18n.language)
    return cached(`l|${tag}`, () => new Intl.ListFormat(tag, { type: "conjunction" })).format(items)
  },
  /** 3725.4 → "1:02:05"; 65 → "1:05"; not a time → "–". The same digits in every language. */
  duration(sec: number): string {
    if (!Number.isFinite(sec) || sec < 0) return "–"
    const s = Math.round(sec)
    const h = Math.floor(s / 3600)
    const m = Math.floor((s % 3600) / 60)
    const r = String(s % 60).padStart(2, "0")
    return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${r}` : `${m}:${r}`
  },
  /** Remaining time for progress lines: "about 3 min left"; unknown → "". */
  eta(ms: number | null | undefined): string {
    if (ms == null || !Number.isFinite(ms)) return ""
    const s = Math.round(ms / 1000)
    if (s < 5) return t("common:format.eta.almostDone")
    if (s < 60) return t("common:format.eta.seconds", { s: fmt.count(s) })
    const m = Math.round(s / 60)
    if (m < 60) return t("common:format.eta.minutes", { m: fmt.count(m) })
    return t("common:format.eta.hours", { h: fmt.decimal(Math.round(m / 6) / 10, 1, true) })
  },
  /** 0.4 → "40%"; unknown → "". */
  percent(v: number | null | undefined): string {
    return v == null ? "" : t("common:format.percent", { value: String(Math.round(v * 100)) })
  },
  /** Screen time as people say it: "42 s", "12 min", "1 h 05 min". */
  screenTime(sec: number): string {
    if (!Number.isFinite(sec) || sec <= 0) return t("common:format.screenTime.seconds", { s: "0" })
    if (sec < 60) return t("common:format.screenTime.seconds", { s: String(Math.round(sec)) })
    const m = Math.round(sec / 60)
    if (m < 60) return t("common:format.screenTime.minutes", { m: String(m) })
    return t("common:format.screenTime.hours", { h: String(Math.floor(m / 60)), mm: String(m % 60).padStart(2, "0") })
  },
  /**
   * A language code's name in the UI language, capitalised as a list item
   * ("de" → "German", ro "Germană"); `undefined` when Intl has no name for it,
   * so the caller keeps its own fallback.
   */
  languageName(code: string): string | undefined {
    const tag = tagOf(i18n.language)
    let name: string | undefined
    try {
      name = cached(`ln|${tag}`, () => new Intl.DisplayNames(tag, { type: "language", fallback: "none" })).of(code)
    } catch {
      return undefined // not a well-formed code
    }
    return name && name.charAt(0).toLocaleUpperCase(tag) + name.slice(1)
  },
}
