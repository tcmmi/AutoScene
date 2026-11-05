import type * as React from "react"
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { t } from "@/lib/i18n"
import { quotedNames, slotLabel } from "@/lib/keymap"
import { queryClient } from "@/lib/queries"
import { DEFAULT_KEYMAP, setKeymap, shortcutOf } from "@/lib/shortcuts"
import { inLanguage } from "@/test/i18n"
import { settingsLabel } from "./sections"
import { ShortcutsSection } from "./sections/ShortcutsSection"
import { UpdatesSection } from "./sections/UpdatesSection"

// Settings and the shortcuts' words come from the catalog: this checks them through a fixture language
// (the real Romanian arrives later), and that a taken key names its shortcuts as one list.

const settings: AppSettings = { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" }

vi.mock("@/lib/ipc", () => ({
  commands: {
    settingsGet: vi.fn(() => Promise.resolve(settings)),
    settingsUpdate: vi.fn((s: AppSettings) => Promise.resolve(s)),
    menuSetKeys: vi.fn(() => Promise.resolve(null)),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))
vi.mock("@/lib/queries", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/queries")>()),
  useAppInfo: () => ({ data: { version: "1.2.3" } }),
}))

let updates = { phase: "current", auto: true, lastChecked: Date.now() - 5 * 60_000, status: null as null | object }
vi.mock("@/state/updates", () => ({
  useUpdates: () => ({ ...updates, check: vi.fn(), setAuto: vi.fn(), setDialogOpen: vi.fn() }),
  releaseNotesUrl: (v: string) => `https://example.test/${v}`,
}))

const wrapper = ({ children }: { children: React.ReactNode }) => <TooltipProvider>{children}</TooltipProvider>

beforeEach(() => {
  queryClient.clear()
  vi.mocked(commands.settingsUpdate).mockClear()
})
afterEach(() => {
  cleanup()
  setKeymap(DEFAULT_KEYMAP)
})

describe("Settings → Shortcuts words", () => {
  it("lists each shortcut by the name in the catalog, in a fixture language", async () => {
    await inLanguage("ro", {
      shortcuts: {
        group: { player: { title: "Player-ul", description: "Cât un episod e deschis." } },
        player: { mute: { name: "Fără sunet" }, frame: { name: "Cadrul anterior / următor", slots: { "0": "Cadrul anterior", "1": "Cadrul următor" } } },
      },
      settings: { keymap: { title: "Taste", noKey: "Fără tastă", slot: "{{name}}: {{keys}}. Schimbă" } },
    })
    render(<ShortcutsSection />, { wrapper })
    expect(screen.getByRole("heading", { name: "Player-ul" })).toBeTruthy()
    expect(screen.getByText("Cât un episod e deschis.")).toBeTruthy()
    expect(screen.getByText("Cadrul anterior / următor")).toBeTruthy()
    // The key button's name is what it does, in the language, then the key.
    expect(screen.getByRole("button", { name: "Fără sunet: M. Schimbă" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Cadrul următor: →. Schimbă" })).toBeTruthy()
    // What the language lacks reads in English.
    expect(screen.getByText("Command palette")).toBeTruthy()
  })

  it("counts the changes with the language's plural forms", async () => {
    await inLanguage("ro", {
      settings: {
        keymap: {
          differs_one: "O tastă diferă de cele din {{preset}}.",
          differs_few: "{{count}} taste diferă de cele din {{preset}}.",
          differs_other: "{{count}} de taste diferă de cele din {{preset}}.",
        },
      },
    })
    setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }], "player.zoomOut.0": [{ key: "u", shift: true }] } })
    render(<ShortcutsSection />, { wrapper })
    expect(screen.getByText("2 taste diferă de cele din AutoScene.")).toBeTruthy()
    act(() => setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }] } }))
    expect(screen.getByText("O tastă diferă de cele din AutoScene.")).toBeTruthy()
  })

  it("says the same English as before for one and for several changes", () => {
    setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }] } })
    render(<ShortcutsSection />, { wrapper })
    expect(screen.getByText("1 key differs from AutoScene's.")).toBeTruthy()
    act(() => setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }], "player.zoomOut.0": [{ key: "u", shift: true }] } }))
    expect(screen.getByText("2 keys differ from AutoScene's.")).toBeTruthy()
    act(() => setKeymap(DEFAULT_KEYMAP))
    expect(screen.getByText("None: every key is AutoScene's.")).toBeTruthy()
  })

  it("switching to another editor's keys shows its own description, and Back names its keys", () => {
    render(<ShortcutsSection />, { wrapper })
    fireEvent.click(screen.getByRole("radio", { name: "DaVinci Resolve" }))
    expect(screen.getByText("Zoom with Ctrl = and Ctrl −.")).toBeTruthy()
    // One key is one key, several are keys: the tooltip of a row's reset follows the number.
    act(() => setKeymap({ preset: "resolve", own: { "player.frame.0": [{ key: "u" }], "player.mute.0": [{ key: "u", shift: true }] } }))
    expect(screen.getByRole("button", { name: "Reset “Previous / next frame”" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Reset “Mute”" })).toBeTruthy()
  })
})

describe("a row's reset and Reset all", () => {
  it("name one key and several through the rendered row's tooltip", async () => {
    setKeymap({ preset: "autoscene", own: { "player.frame.0": [{ key: "u" }], "player.mute.0": [{ key: "u", shift: true }] } })
    render(<ShortcutsSection />, { wrapper })
    const tip = async (name: string) => {
      fireEvent.focus(screen.getByRole("button", { name }))
      return (await screen.findAllByText(/^Back to AutoScene's keys?$/))[0].textContent
    }
    expect(await tip("Reset “Mute”")).toBe("Back to AutoScene's key")
    cleanup()
    render(<ShortcutsSection />, { wrapper })
    expect(await tip("Reset “Previous / next frame”")).toBe("Back to AutoScene's keys")
  })

  it("Reset all puts the focus on the selected preset", () => {
    vi.useFakeTimers({ toFake: ["setTimeout"] })
    try {
      setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }] } })
      render(<ShortcutsSection />, { wrapper })
      fireEvent.click(screen.getByRole("button", { name: /^Reset all/ }))
      act(() => void vi.runAllTimers())
      const radio = screen.getByRole("radio", { name: "AutoScene" })
      expect(radio.getAttribute("aria-checked")).toBe("true")
      expect(document.activeElement).toBe(radio)
    } finally {
      vi.useRealTimers()
    }
  })
})

describe("the sentences with a count", () => {
  it("say one change and several, one key and several, as they always did", () => {
    expect(t("settings:keymap.letGo", { count: 1 })).toBe("Your change was let go.")
    expect(t("settings:keymap.letGo", { count: 3 })).toBe("Your 3 changes were let go.")
    expect(t("settings:keymap.backTo", { count: 1, preset: "AutoScene" })).toBe("Back to AutoScene's key")
    expect(t("settings:keymap.backTo", { count: 2, preset: "AutoScene" })).toBe("Back to AutoScene's keys")
  })

  it("tell the person, in a toast, how many of their changes another editor's keys let go", () => {
    setKeymap({ preset: "autoscene", own: { "player.mute.0": [{ key: "u" }], "player.zoomOut.0": [{ key: "u", shift: true }] } })
    render(<ShortcutsSection />, { wrapper })
    fireEvent.click(screen.getByRole("radio", { name: "Premiere Pro" }))
    expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Keys like in Premiere Pro")
    expect(vi.mocked(toast).mock.lastCall?.[1]).toMatchObject({ description: "Your 2 changes were let go." })
  })
})

describe("the shortcuts a key was taken from", () => {
  const uses = (...ids: ("player.mute" | "player.markShot" | "player.zoomOut")[]) => ids.map((id) => ({ s: shortcutOf(id), slot: 0 }))

  it("names two as one list, as before, and three the way a list is written", () => {
    expect(quotedNames(uses("player.mute", "player.markShot"))).toBe("“Mute” and “Mark the shot as the clip”")
    // The deliberate fix: three names were "“a” and “b” and “c”".
    expect(quotedNames(uses("player.mute", "player.markShot", "player.zoomOut"))).toBe(
      "“Mute”, “Mark the shot as the clip” and “Zoom the timeline out”",
    )
    // The same shortcut twice is named once.
    expect(quotedNames(uses("player.mute", "player.mute"))).toBe("“Mute”")
  })

  it("follows the language's list and quotation marks", async () => {
    await inLanguage("ro", { shortcuts: { quote: "„{{name}}”" } })
    expect(quotedNames(uses("player.mute", "player.markShot", "player.zoomOut"))).toMatch(/^„Mute”, „.*” și „.*”$/)
  })

  it("is said in the toast when recording a key takes it from two shortcuts", () => {
    render(<ShortcutsSection />, { wrapper })
    fireEvent.click(screen.getByRole("button", { name: "Command palette: Ctrl K. Change" }))
    act(() => void window.dispatchEvent(new KeyboardEvent("keydown", { key: "e", ctrlKey: true, bubbles: true, cancelable: true })))
    expect(vi.mocked(toast).mock.lastCall?.[0]).toBe("Took Ctrl E from “Export the marked clip” and “Export the pack”")
  })

  it("uses a slot's own name where a row does a few things", () => {
    expect(slotLabel({ s: shortcutOf("player.frame"), slot: 1 })).toBe("Next frame")
    expect(slotLabel({ s: shortcutOf("player.mute"), slot: 0 })).toBe("Mute")
  })
})

describe("Settings' words", () => {
  it("points the switch's line at the button by the button's own label", async () => {
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText("At launch and every few hours. Off, AutoScene only looks when you click Check for updates.")).toBeTruthy()
    cleanup()
    await inLanguage("ro", {
      settings: { updates: { check: "Caută actualizări", auto: { description: "Doar când apeși {{check}}." } } },
    })
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText("Doar când apeși Caută actualizări.")).toBeTruthy()
  })

  it("an update with no version says so without a gap", () => {
    updates = { ...updates, phase: "idle", status: { state: "available" } }
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText("An update is ready to install.")).toBeTruthy()
    cleanup()
    updates = { ...updates, status: { state: "available", version: "1.2.4" } }
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText("Version 1.2.4 is ready to install.")).toBeTruthy()
    updates = { ...updates, phase: "current", status: null }
  })

  it("name a section from its route in the language, and an unknown one Settings", async () => {
    expect(settingsLabel("about")).toBe("About")
    expect(settingsLabel("nope")).toBe("Settings")
    await inLanguage("ro", { settings: { section: { about: "Despre" }, page: { title: "Setări" } } })
    expect(settingsLabel("about")).toBe("Despre")
    expect(settingsLabel("nope")).toBe("Setări")
  })

  it("says when the last update check ended as one phrase per distance", async () => {
    updates = { ...updates, lastChecked: Date.now() - 5 * 60_000 }
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText(/· checked 5 min ago/)).toBeTruthy()
    cleanup()
    updates = { ...updates, lastChecked: Date.now() - 10_000 }
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText(/· checked just now/)).toBeTruthy()
    cleanup()
    updates = { ...updates, lastChecked: Date.now() - 3 * 3600_000 }
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText(/· checked 3 h ago/)).toBeTruthy()
    cleanup()
    updates = { ...updates, lastChecked: Date.now() - 60_000 }
    render(<UpdatesSection />, { wrapper })
    expect(screen.getByText(/· checked 1 min ago/)).toBeTruthy()
    cleanup()
    // Each distance picks the language's plural form: Romanian says "acum 5 minute" but "acum 20 de minute".
    await inLanguage("ro", {
      settings: {
        updates: {
          checked: {
            minutes_one: "verificat acum un minut",
            minutes_few: "verificat acum {{count}} minute",
            minutes_other: "verificat acum {{count}} de minute",
            hours_one: "verificat acum o oră",
            hours_few: "verificat acum {{count}} ore",
            hours_other: "verificat acum {{count}} de ore",
          },
        },
      },
    })
    for (const [ms, said] of [
      [60_000, /· verificat acum un minut/],
      [5 * 60_000, /· verificat acum 5 minute/],
      [20 * 60_000, /· verificat acum 20 de minute/],
      [3600_000, /· verificat acum o oră/],
      [3 * 3600_000, /· verificat acum 3 ore/],
      [20 * 3600_000, /· verificat acum 20 de ore/],
    ] as const) {
      updates = { ...updates, lastChecked: Date.now() - ms }
      render(<UpdatesSection />, { wrapper })
      expect(screen.getByText(said)).toBeTruthy()
      cleanup()
    }
    updates = { ...updates, lastChecked: Date.now() - 5 * 60_000 }
    render(<UpdatesSection />, { wrapper })
    // The version is a value inside a translated sentence.
    expect(screen.getByText("What changed in AutoScene 1.2.3, on GitHub.")).toBeTruthy()
  })
})
