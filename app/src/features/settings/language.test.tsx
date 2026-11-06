import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider } from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest"
import { commands, type UiLanguage } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { applySettings, fmt, i18n, LanguageRoot, t } from "@/lib/i18n"
import { qk, queryClient } from "@/lib/queries"
import { installMockBackend } from "@/mocks/backend"
import { router } from "@/router"
import { useUi } from "@/state/ui"
import { inLanguage } from "@/test/i18n"

// The real window on the mock backend: the choice in Settings, and what it reaches.

const ASKED = "autoscene.mock.language"

function show() {
  render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <LanguageRoot>
          <RouterProvider router={router} />
        </LanguageRoot>
      </TooltipProvider>
    </QueryClientProvider>,
  )
}

const go = (href: string) => act(() => router.navigate({ href }))

async function choose(row: string, option: string) {
  const select = await screen.findByRole("combobox", { name: row })
  await waitFor(() => expect((select as HTMLButtonElement).disabled).toBe(false))
  fireEvent.keyDown(select, { key: "Enter" })
  fireEvent.click(await screen.findByRole("option", { name: option }))
}

beforeAll(() => {
  installMockBackend()
  Element.prototype.scrollTo ??= () => {}
})
beforeEach(async () => {
  queryClient.clear()
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  await commands.settingsUpdate({ ...(await commands.settingsGet()), language: "system" })
  router.history.replace("/settings/general")
})
afterEach(() => {
  cleanup()
  useUi.setState({ paletteOpen: false })
  localStorage.removeItem(ASKED)
  vi.restoreAllMocks()
})

describe("the language row", () => {
  it("is labelled App language, first on the page, and the audio row keeps its own name", async () => {
    show()
    const row = await screen.findByRole("combobox", { name: "App language" })
    expect(screen.getByRole("combobox", { name: "Preferred audio language" })).toBeTruthy()
    const groups = screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent)
    expect(groups[0]).toBe("Language")
    expect(screen.getByText("App language").closest("section")?.contains(row)).toBe(true)
    expect(screen.getByText("The words AutoScene uses, and how it writes dates and numbers.")).toBeTruthy()
  })

  it("names what Automatic comes to, not what the system speaks", async () => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["de-DE"])
    show()
    const row = await screen.findByRole("combobox", { name: "App language" })
    await waitFor(() => expect(row.textContent).toBe("Automatic (English)"))
  })

  it("names Romanian by its own name, in English too", async () => {
    localStorage.setItem(ASKED, "ro")
    await commands.settingsUpdate({ ...(await commands.settingsGet()), language: "en" })
    show()
    const row = await screen.findByRole("combobox", { name: "App language" })
    await waitFor(() => expect((row as HTMLButtonElement).disabled).toBe(false))
    fireEvent.keyDown(row, { key: "Enter" })
    const options = (await screen.findAllByRole("option")).map((o) => o.textContent)
    expect(options).toEqual(["Automatic (Română)", "English", "Română"])
  })

  it("offers the same names in Romanian, with only Automatic's word in the language", async () => {
    await commands.settingsUpdate({ ...(await commands.settingsGet()), language: "ro" })
    await inLanguage("ro")
    show()
    const row = await screen.findByRole("combobox", { name: "Limba aplicației" })
    await waitFor(() => expect((row as HTMLButtonElement).disabled).toBe(false))
    fireEvent.keyDown(row, { key: "Enter" })
    const options = (await screen.findAllByRole("option")).map((o) => o.textContent)
    expect(options).toEqual(["Automat (English)", "English", "Română"])
  })

  it("applies with no toast and stays chosen", async () => {
    show()
    await choose("App language", "Română")
    await waitFor(() => expect(i18n.language).toBe("ro"))
    expect(document.documentElement.lang).toBe("ro")
    expect((await commands.settingsGet()).language).toBe("ro")
    expect((await screen.findByRole("combobox", { name: "Limba aplicației" })).textContent).toBe("Română")
    expect(document.querySelector("[data-sonner-toast]")).toBeNull()
  })

  it("has a pack's plan made again: the names of its files are the backend's words, in the language it spoke", async () => {
    const plan = [...qk.packPlans("p", "c"), {}, {}]
    queryClient.setQueryData(plan, { clips: [] })
    show()
    await choose("App language", "Română")
    await waitFor(() => expect(i18n.language).toBe("ro"))
    await waitFor(() => expect(queryClient.getQueryState(plan)?.isInvalidated).toBe(true))
  })

  it("keeps focus on the row after the choice: the page is made again in the new language", async () => {
    show()
    await choose("App language", "Română")
    const row = await screen.findByRole("combobox", { name: "Limba aplicației" })
    await waitFor(() => expect(document.activeElement).toBe(row))
    // And only then: leaving Settings and coming back doesn't pull focus to it.
    await go("/")
    await go("/settings/general")
    await screen.findByRole("combobox", { name: "Limba aplicației" })
    expect(document.activeElement).not.toBe(await screen.findByRole("combobox", { name: "Limba aplicației" }))
  })
})

describe("switching the language", () => {
  it("switching to Română turns the rail, top bar, Settings, the Player, Cast and the palette Romanian without a reload, and back", async () => {
    const [project] = await commands.projectsList()
    const media = (await commands.mediaList(project!.id)).find((m) => m.status === "ready")!
    const cast = await commands.castGet(project!.id)
    const character = cast.characters[0]!
    const loaded = vi.fn()
    window.addEventListener("beforeunload", loaded)
    show()

    const rail = () => screen.findByRole("navigation", { name: t("shell:rail.label") })
    const at = async (place: string, words: () => void) => {
      await go(place)
      await waitFor(words, { timeout: 5000 })
    }
    const pages = async (ro: boolean) => {
      expect(within(await rail()).getByRole("link", { name: ro ? "Bibliotecă" : "Library" })).toBeTruthy()
      await at("/settings/general", () => {
        expect(screen.getByRole("navigation", { name: ro ? "Setări" : "Settings" })).toBeTruthy()
        expect(screen.getByRole("heading", { level: 3, name: ro ? "Stocare" : "Storage" })).toBeTruthy()
      })
      await at(`/p/${project!.id}/cast`, () => {
        // The project's places are the top bar.
        expect(screen.getByRole("link", { name: ro ? "Episoade" : "Episodes" })).toBeTruthy()
        expect(document.body.textContent).toMatch(ro ? /\d+ (de )?(plan|planuri)/ : /\d+ shots?/)
      })
      // A face card is memoised; it is opened after the switch here ("a page that stays open" below switches under it).
      await at(`/p/${project!.id}/cast/${character.id}`, () => {
        expect(screen.getAllByRole("button", { name: ro ? /^Față/ : /^Face/ }).length).toBeGreaterThan(0)
      })
      await at(`/p/${project!.id}/timeline/${media.id}`, () => {
        expect(screen.getByRole("button", { name: ro ? "Redă" : "Play" })).toBeTruthy()
      })
      act(() => useUi.setState({ paletteOpen: true }))
      expect(await screen.findByPlaceholderText(ro ? /Caută/ : /Search/)).toBeTruthy()
      act(() => useUi.setState({ paletteOpen: false }))
    }

    await pages(false)
    await go("/settings/general")
    await choose("App language", "Română")
    await waitFor(() => expect(i18n.language).toBe("ro"))
    await pages(true)
    await go("/settings/general")
    await choose("Limba aplicației", "English")
    await waitFor(() => expect(i18n.language).toBe("en"))
    await pages(false)

    expect(loaded).not.toHaveBeenCalled()
    window.removeEventListener("beforeunload", loaded)
  }, 60_000)
})

describe("switching the language on a page that stays open", () => {
  // The pages above are opened after each switch, so they are made in the new language whatever the switch does.
  // Here nothing is opened again: what is on screen has to change by itself. (That the whole tree is made again,
  // for components that never re-render by themselves, is held by lib/i18n.test.tsx and PluginFrame.test.tsx.)
  it("turns the face cards of the open page Romanian, and back, with no navigation", async () => {
    const [project] = await commands.projectsList()
    const cast = await commands.castGet(project!.id)
    show()
    await go(`/p/${project!.id}/cast/${cast.characters[0]!.id}`)
    await waitFor(() => expect(screen.getAllByRole("button", { name: /^Face/ }).length).toBeGreaterThan(0), { timeout: 5000 })
    const switchTo = async (language: UiLanguage) => {
      await commands.settingsUpdate({ ...(await commands.settingsGet()), language })
      await act(() => applySettings())
    }
    await switchTo("ro")
    await waitFor(() => expect(screen.getAllByRole("button", { name: /^Față/ }).length).toBeGreaterThan(0), { timeout: 5000 })
    expect(screen.queryAllByRole("button", { name: /^Face/ })).toEqual([])
    await switchTo("en")
    await waitFor(() => expect(screen.getAllByRole("button", { name: /^Face/ }).length).toBeGreaterThan(0), { timeout: 5000 })
    expect(screen.queryAllByRole("button", { name: /^Față/ })).toEqual([])
  }, 30_000)
})

describe("Romanian, as it is written", () => {
  it("counts faces with the language's three forms", async () => {
    await inLanguage("ro")
    const faces = (count: number) => t("cast:count.faces", { count })
    expect([1, 2, 20, 101, 120].map(faces)).toEqual(["1 față", "2 fețe", "20 de fețe", "101 fețe", "120 de fețe"])
  })

  it("writes a date and a size the Romanian way", async () => {
    await inLanguage("ro")
    expect(fmt.longDate(Date.UTC(2026, 10, 3, 12))).toBe("3 noiembrie 2026")
    expect(fmt.dateTime(Date.UTC(2026, 9, 13, 12))).toMatch(/^13 oct\.?, \d\d:\d\d$/)
    expect(fmt.bytes(1536)).toBe("1,5\u00a0KB")
  })
})
