import { afterEach, beforeAll, describe, expect, it, vi } from "vitest"
import { commands, type UiLocale } from "@/bindings"
import { installMockBackend } from "@/mocks/backend"
import { applySettings, i18n, PSEUDO, pseudo, startLanguage, t } from "./i18n"

const ASKED = "autoscene.mock.language"

describe("the language at the start", () => {
  beforeAll(() => installMockBackend())
  afterEach(() => {
    localStorage.removeItem(ASKED)
    vi.restoreAllMocks()
  })

  it("speaks what the backend resolves", async () => {
    localStorage.setItem(ASKED, "ro")
    await startLanguage()
    expect(i18n.language).toBe("ro")
    expect(document.documentElement.lang).toBe("ro")
  })

  it("is English on a system AutoScene doesn't speak to", async () => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["de-DE", "ro-RO"])
    await startLanguage()
    expect(i18n.language).toBe("en")
    expect(document.documentElement.lang).toBe("en")
    expect(await commands.uiLocale()).toEqual({ setting: "system", resolved: "en", automatic: "en" })
  })

  it("still starts in English when the backend can't say", async () => {
    vi.spyOn(commands, "uiLocale").mockRejectedValue({ code: "internal", message: "no" })
    const logged = vi.spyOn(console, "error").mockImplementation(() => {})
    await startLanguage()
    expect(i18n.language).toBe("en")
    expect(document.documentElement.lang).toBe("en")
    expect(t("common:action.cancel")).toBe("Cancel")
    expect(logged).toHaveBeenCalledOnce()
  })
})

describe("the language after a save", () => {
  beforeAll(() => installMockBackend())
  afterEach(() => {
    localStorage.removeItem(ASKED)
    vi.restoreAllMocks()
  })

  it("follows what the settings come to, at once", async () => {
    localStorage.setItem(ASKED, "ro")
    await applySettings()
    expect(i18n.language).toBe("ro")
    // English chosen beats the system, and Automatic follows it again.
    const settings = await commands.settingsGet()
    await commands.settingsUpdate({ ...settings, language: "en" })
    await applySettings()
    expect(i18n.language).toBe("en")
    await commands.settingsUpdate({ ...settings, language: "system" })
    await applySettings()
    expect(i18n.language).toBe("ro")
  })

  it("says what Automatic comes to while a language is chosen", async () => {
    localStorage.setItem(ASKED, "ro")
    const settings = await commands.settingsGet()
    await commands.settingsUpdate({ ...settings, language: "en" })
    expect(await commands.uiLocale()).toEqual({ setting: "en", resolved: "en", automatic: "ro" })
    await commands.settingsUpdate({ ...settings, language: "system" })
    expect(await commands.uiLocale()).toEqual({ setting: "system", resolved: "ro", automatic: "ro" })
  })

  it("keeps the language in use when the backend can't say", async () => {
    localStorage.setItem(ASKED, "ro")
    await applySettings()
    vi.spyOn(commands, "uiLocale").mockRejectedValue({ code: "internal", message: "no" })
    vi.spyOn(console, "error").mockImplementation(() => {})
    await applySettings()
    expect(i18n.language).toBe("ro")
  })
})

describe("a language asked for in the address while developing", () => {
  beforeAll(() => installMockBackend())
  afterEach(() => window.history.replaceState(null, "", "/"))

  it("shows the pseudo-locale, whatever the backend says", async () => {
    window.history.replaceState(null, "", "/?lang=pseudo")
    await startLanguage()
    expect(i18n.language).toBe(PSEUDO)
    expect(document.documentElement.lang).toBe("en")
    expect(t("common:action.cancel")).toBe(pseudo("Cancel"))
  })

  it("shows a language not offered yet", async () => {
    window.history.replaceState(null, "", "/?lang=ro")
    await startLanguage()
    expect(i18n.language).toBe("ro")
  })

  it("leaves anything else to the backend", async () => {
    window.history.replaceState(null, "", "/?lang=xx")
    await startLanguage()
    expect(i18n.language).toBe("en")
  })
})

describe("switches that cross", () => {
  // Romanian, which the bindings don't know until it is spoken.
  const EN = { setting: "en", resolved: "en", automatic: "ro" } as unknown as UiLocale
  const RO = { setting: "system", resolved: "ro", automatic: "ro" } as unknown as UiLocale

  /** A fresh i18n whose Romanian arrives when the test says so; setup.ts imported the shared one before any mock. */
  async function fresh() {
    let arrive = () => {}
    const held = new Promise<Record<string, never>>((resolve) => (arrive = () => resolve({})))
    vi.resetModules()
    vi.doMock("@/locales", async (original) => ({
      ...(await original<typeof import("@/locales")>()),
      loadLanguage: async (lng: string) => (lng === "ro" ? held : {}),
    }))
    const { applySettings, i18n } = await import("./i18n")
    const { commands } = await import("@/bindings")
    return { applySettings, i18n, arrive: () => arrive(), asked: vi.spyOn(commands, "uiLocale") }
  }

  afterEach(() => {
    vi.doUnmock("@/locales")
    vi.resetModules()
    vi.restoreAllMocks()
  })

  it("stays with the language saved last while the one before still loads", async () => {
    const { applySettings, i18n, arrive, asked } = await fresh()
    asked.mockResolvedValueOnce(RO).mockResolvedValueOnce(EN)
    const loading = applySettings()
    await vi.waitFor(() => expect(asked).toHaveBeenCalledOnce())
    await applySettings()
    arrive()
    await loading
    expect(i18n.language).toBe("en")
    expect(document.documentElement.lang).toBe("en")
  })

  it("takes the newest answer when the replies come in the wrong order", async () => {
    const { applySettings, i18n, arrive, asked } = await fresh()
    let reply = () => {}
    const late = new Promise<UiLocale>((resolve) => (reply = () => resolve(RO)))
    asked.mockReturnValueOnce(late).mockResolvedValueOnce(EN)
    const older = applySettings()
    await applySettings()
    reply()
    arrive()
    await older
    expect(i18n.language).toBe("en")
  })
})
