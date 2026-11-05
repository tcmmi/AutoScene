import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { inLanguage } from "@/test/i18n"
import { NameCrumb, SettingsCrumb } from "./crumbs"
import { WindowTitle } from "./WindowTitle"

const { matches } = vi.hoisted(() => ({ matches: { list: [] as unknown[] } }))
vi.mock("@tanstack/react-router", () => ({ useMatches: () => matches.list }))
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ setTitle: () => Promise.resolve() }) }))
vi.mock("@/features/settings/sections", () => ({ settingsLabel: (s: string) => (s === "shortcuts" ? "Shortcuts" : "Settings") }))

afterEach(() => {
  cleanup()
  matches.list = []
})

const route = (id: string, crumb: unknown) => ({ id, params: {}, staticData: { crumb } })

describe("the window title", () => {
  it("is made of the names of the screens, most specific first", () => {
    matches.list = [route("root", undefined), route("cast", <NameCrumb name="cast" />), route("review", <NameCrumb name="review" />)]
    render(<WindowTitle />)
    expect(document.title).toBe("Cast · Review — Cast — AutoScene")
  })

  it("follows the language when it changes, with no reload", async () => {
    matches.list = [route("exports", <NameCrumb name="exports" />), route("settings", <SettingsCrumb section="shortcuts" />)]
    render(<WindowTitle />)
    expect(document.title).toBe("Settings · Shortcuts — Exports — AutoScene")
    await act(() => inLanguage("ro", { shell: { crumb: { exports: "Exporturi", settings: "Setări · {{section}}" } } }))
    await waitFor(() => expect(document.title).toBe("Setări · Shortcuts — Exporturi — AutoScene"))
  })

  it("names a screen in the language it is shown in", async () => {
    await inLanguage("ro", { shell: { crumb: { cast: "Distribuție" } } })
    render(<NameCrumb name="cast" />)
    expect(screen.getByText("Distribuție")).toBeTruthy()
  })
})
