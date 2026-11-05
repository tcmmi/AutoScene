import { memo } from "react"
import { act, render, screen } from "@testing-library/react"
import { Trans } from "react-i18next"
import { describe, expect, it } from "vitest"
import { en } from "@/locales"
import { catalogProblems } from "@/locales/check"
import { inLanguage } from "@/test/i18n"
import { fmt, i18n, LanguageRoot, pseudo, PSEUDO, setLanguage, t } from "./i18n"

// Fixture keys outside the typed catalog.
const tx = (key: string, options?: Record<string, unknown>) => (i18n.t as unknown as (k: string, o?: object) => string)(key, options)

const fixtureEn = {
  faces_one: "{{count}} face",
  faces_other: "{{count}} faces",
  released: "Released in {{year}}",
  size: "{{width}}×{{height}}",
  grouped: "{{n, number}} frames",
  name: "Hello {{name}}",
  rich: "<b>{{count}}</b> frames · <go>Settings</go>",
}
const fixtureRo = {
  faces_one: "{{count}} față",
  faces_few: "{{count}} fețe",
  faces_other: "{{count}} de fețe",
  released: "Lansat în {{year}}",
  rich: "<b>{{count}}</b> cadre · <go>Setări</go>",
}

describe("the catalog", () => {
  it("speaks English synchronously, right after import", () => {
    expect(i18n.isInitialized).toBe(true)
    expect(t("common:action.cancel")).toBe("Cancel")
    expect(t("state.waiting")).toBe("Waiting…")
  })

  it("uses a language's words once switched to it, nothing gates the language", async () => {
    await inLanguage("ro", { common: { action: { cancel: "Anulează" } } })
    expect(t("common:action.cancel")).toBe("Anulează")
    expect(document.documentElement.lang).toBe("ro")
    // A key the language lacks falls back to English.
    expect(t("common:action.close")).toBe("Close")
    await setLanguage("en")
    expect(t("common:action.cancel")).toBe("Cancel")
    expect(document.documentElement.lang).toBe("en")
  })

  it("groups counts and `number` variables by the language, and leaves other numbers as written", async () => {
    i18n.addResourceBundle("en", "fixture", fixtureEn)
    expect(tx("fixture:faces", { count: 14_200 })).toBe("14,200 faces")
    expect(tx("fixture:grouped", { n: 14_200 })).toBe("14,200 frames")
    expect(tx("fixture:released", { year: 2023 })).toBe("Released in 2023")
    expect(tx("fixture:size", { width: 1920, height: 1080 })).toBe("1920×1080")
    await inLanguage("ro", { fixture: fixtureRo })
    expect(tx("fixture:faces", { count: 14_200 })).toBe("14.200 de fețe")
    expect(tx("fixture:grouped", { n: 14_200 })).toBe("14.200 frames")
    expect(tx("fixture:released", { year: 2023 })).toBe("Lansat în 2023")
    i18n.removeResourceBundle("en", "fixture")
  })

  it("picks English one/other and Romanian one/few/other plural forms", async () => {
    i18n.addResourceBundle("en", "fixture", fixtureEn)
    const counts = [0, 1, 2, 19, 20, 101, 119, 120]
    expect(counts.map((count) => tx("fixture:faces", { count }))).toEqual([
      "0 faces", "1 face", "2 faces", "19 faces", "20 faces", "101 faces", "119 faces", "120 faces",
    ])
    await inLanguage("ro", { fixture: fixtureRo })
    expect(counts.map((count) => tx("fixture:faces", { count }))).toEqual([
      "0 fețe", "1 față", "2 fețe", "19 fețe", "20 de fețe", "101 fețe", "119 fețe", "120 de fețe",
    ])
    i18n.removeResourceBundle("en", "fixture")
  })

  it("refuses an object as a value while developing, instead of reading [object Object]", () => {
    i18n.addResourceBundle("en", "fixture", fixtureEn)
    expect(() => tx("fixture:name", { name: { key: "x", text: "Walter" } })).toThrow(/\{\{name\}\} got an object/)
    expect(tx("fixture:name", { name: "Walter" })).toBe("Hello Walter")
    i18n.removeResourceBundle("en", "fixture")
  })

  it("fills <Trans> tags with elements", async () => {
    i18n.addResourceBundle("en", "fixture", fixtureEn)
    const view = () => (
      <p data-testid="rich">
        <Trans
          i18nKey={"fixture:rich" as never}
          values={{ count: 3 }}
          components={{ b: <strong />, go: <a href="#settings" /> }}
        />
      </p>
    )
    const { rerender } = render(view())
    expect(screen.getByTestId("rich").innerHTML).toBe('<strong>3</strong> frames · <a href="#settings">Settings</a>')
    await act(() => inLanguage("ro", { fixture: fixtureRo }))
    rerender(view())
    expect(screen.getByTestId("rich").innerHTML).toBe('<strong>3</strong> cadre · <a href="#settings">Setări</a>')
    i18n.removeResourceBundle("en", "fixture")
  })

  it("refuses tags <Trans> can't fill, and empty text", () => {
    expect(catalogProblems(en)).toEqual([])
    expect(catalogProblems({ a: "See <link>Settings</link>", b: "<go>ok</go>", c: " " })).toEqual([
      "a: <link> is not a Trans tag (b strong em mono key x go)",
      "a: <link> is not a Trans tag (b strong em mono key x go)",
      "c: empty",
    ])
  })

  it("keeps a plugin's toast behind fixed words and its name, in every language", () => {
    const toast = (pageToast: string) => catalogProblems({ backend: { plugin: { pageToast } } })
    const key = "backend.plugin.pageToast"
    expect(toast("Pluginul „{{name}}”: {{text}}")).toEqual([])
    expect(toast("{{text}} (din {{name}})")).toEqual([
      `${key}: starts with a plugin's words, not with fixed text`,
      `${key}: {{name}} must come before any other variable`,
    ])
    expect(toast("{{name}}: {{text}}")).toEqual([`${key}: starts with a plugin's words, not with fixed text`])
    expect(toast("„{{name}}”: {{text}}")).toEqual([`${key}: starts with a plugin's words, not with fixed text`])
    expect(toast("Pluginul: {{text}} ({{name}})")).toEqual([`${key}: {{name}} must come before any other variable`])
    const later = (toast: Record<string, unknown>) => catalogProblems({ plugins: { toast } })
    expect(later({ cantStart: { named: "Plugin “{{name}}”: {{action}} can't start", anonymous: "A plugin: {{action}} can't start" } })).toEqual([])
    expect(later({ cantStart: { named: "{{action}} can't start", anonymous: "{{action}} can't start" } })).toEqual([
      "plugins.toast.cantStart.named: starts with a plugin's words, not with fixed text",
      "plugins.toast.cantStart.named: {{name}} must come before any other variable",
      "plugins.toast.cantStart.anonymous: starts with a plugin's words, not with fixed text",
    ])
    // Elsewhere a sentence may open with a value.
    expect(catalogProblems({ backend: { plugin: { turnedOff: "{{name}} is turned off." } } })).toEqual([])
  })

  it("holds every other language's catalog to the same rules", () => {
    const files = import.meta.glob<Record<string, unknown>>(["../locales/*/*.json", "!../locales/en/*", "!../locales/*/meta.json"], { eager: true, import: "default" })
    const catalogs: Record<string, Record<string, unknown>> = {}
    for (const [file, words] of Object.entries(files)) {
      const [, lang, ns] = /([^/]+)\/([^/]+)\.json$/.exec(file) ?? []
      ;(catalogs[lang] ??= {})[ns] = words
    }
    expect(Object.entries(catalogs).flatMap(([lang, catalog]) => catalogProblems(catalog).map((p) => `${lang}: ${p}`))).toEqual([])
  })

  it("shows the pseudo-locale accented, longer and in brackets, keeping Trans tags", async () => {
    expect(pseudo("Hide 3 characters")).toBe("[Ĥîðé 3 çĥåŕåçţéŕš ······]")
    expect(pseudo("<b>3</b> frames")).toBe("[<b>3</b> ƒŕåɱéš ······]")
    expect(pseudo("about {{m}} min left $t(common:action.stop)")).toBe(
      "[åƀöûţ {{m}} ɱîñ ļéƒţ $t(common:action.stop) ················]",
    )
    await setLanguage(PSEUDO)
    expect(t("common:action.cancel")).toBe("[Çåñçéļ ···]")
    expect(document.documentElement.lang).toBe("en")
  })

  it("leaves the values of a pseudo-locale message plain, so a word the code hands in still shows", async () => {
    await setLanguage(PSEUDO)
    expect(t("common:format.eta.minutes", { m: "three" })).toBe("[åƀöûţ three ɱîñ ļéƒţ ·······]")
    expect(fmt.bytes(1536)).toBe("[1.5 ĶƁ ·····]")
  })

  it("ends in the language chosen last when two switches overlap", async () => {
    // Romanian still loads (a lazy import) when the pseudo-locale is chosen: the later choice wins.
    await Promise.all([setLanguage("ro"), setLanguage(PSEUDO)])
    expect(i18n.language).toBe(PSEUDO)
    await Promise.all([setLanguage(PSEUDO), setLanguage("en")])
    expect(i18n.language).toBe("en")
  })

  it("re-renders memoised components and helpers that use the global t after a switch", async () => {
    let renders = 0
    // A helper calling the global `t` during render, inside a memoised component that never re-renders by itself.
    const label = () => t("common:action.cancel")
    const Stale = memo(function Stale() {
      renders++
      return <span>{label()}</span>
    })
    render(
      <LanguageRoot>
        <Stale />
      </LanguageRoot>,
    )
    expect(screen.getByText("Cancel")).toBeTruthy()
    await act(() => inLanguage("ro", { common: { action: { cancel: "Anulează" } } }))
    expect(screen.getByText("Anulează")).toBeTruthy()
    expect(renders).toBeGreaterThanOrEqual(2)
  })
})

describe("fmt in another language", () => {
  it("writes numbers, lists and language names the language's way", async () => {
    expect(fmt.list(["“a”", "“b”", "“c”"])).toBe("“a”, “b” and “c”")
    expect(fmt.languageName("de")).toBe("German")
    expect(fmt.languageName("zz")).toBeUndefined()
    await inLanguage("ro")
    expect(fmt.count(14_200)).toBe("14.200")
    expect(fmt.decimal(7.46, 1)).toBe("7,5")
    expect(fmt.list(["A", "B", "C"])).toBe("A, B și C")
    expect(fmt.languageName("de")).toBe("Germană")
  })
})
