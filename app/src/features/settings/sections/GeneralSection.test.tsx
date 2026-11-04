import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, GraphicsClass, HardwareInfo, Platform, ScanChoice, ScanDefaults } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { GeneralSection } from "./GeneralSection"

const { state } = vi.hoisted(() => ({
  state: {
    cls: "integrated" as GraphicsClass,
    choice: "auto" as ScanChoice,
    platform: "windows" as Platform,
    /** A hybrid laptop whose cards can be chosen (Graphics card), else one built-in GPU. */
    hybrid: false,
    settings: { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng", findBodies: false } as AppSettings,
  },
}))

/** The hybrid laptop's cards, Automatic's first, and how Fast Scan's default classes each. */
const CARDS = [
  { key: "10de:28e0:NVIDIA GeForce RTX 4060 Laptop GPU", name: "NVIDIA GeForce RTX 4060 Laptop GPU", cls: "forAi" as GraphicsClass },
  { key: "8086:a7a0:Intel Iris Xe Graphics", name: "Intel Iris Xe Graphics", cls: "integrated" as GraphicsClass },
]
const chosenCard = () => CARDS.find((c) => c.key === state.settings.gpu) ?? CARDS[0]

/** What `hardware_info` answers. */
function hardware(): HardwareInfo {
  const using = state.hybrid ? chosenCard().name : "Intel Iris Xe Graphics"
  return {
    gpus: state.hybrid ? CARDS.map(({ key, name }) => ({ key, name })) : [],
    choice: state.hybrid ? "cards" : "autoOrProcessor",
    selected: state.hybrid && state.settings.gpu ? chosenCard().key : "auto",
    using: { kind: "gpu", name: using },
    missing: false,
    pending: false,
    fellBack: false,
  }
}

/** What the library answers, like `as_project::scan::defaults`. */
function defaults(): ScanDefaults {
  const { choice } = state
  // The class of the card the analysis runs on: the one chosen.
  const cls = state.hybrid ? chosenCard().cls : state.cls
  const here = cls === "notForAi" || cls === "integrated" || cls === "processor"
  const fast = (content: string) =>
    choice === "auto" ? here && content !== "animation" && (content === "anime" || !state.settings.findBodies) : choice === "fast"
  return {
    computer: { class: cls, name: "Intel Iris Xe Graphics" },
    choice,
    footage: (["live", "mixed", "animation", "anime"] as const).map((content) => ({ content, fast: fast(content) })),
    notice: false,
  }
}

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    appInfo: vi.fn(() => Promise.resolve({ version: "1.1.0", platform: state.platform, arch: "x86_64", dataDir: "C:\\Data" })),
    settingsGet: vi.fn(() => Promise.resolve({ ...state.settings, fastScan: defaults().footage[0].fast })),
    settingsUpdate: vi.fn((s: AppSettings) => {
      state.settings = s
      return Promise.resolve({ ...s, fastScan: defaults().footage[0].fast })
    }),
    exportPresets: vi.fn(() => Promise.resolve([])),
    scanDefaultsGet: vi.fn(() => Promise.resolve(defaults())),
    hardwareInfo: vi.fn(() => Promise.resolve(hardware())),
    scanDefaultSet: vi.fn((choice: ScanChoice) => {
      state.choice = choice
      return Promise.resolve(defaults())
    }),
  },
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

const choice = (name: "Automatic" | "On" | "Off") => screen.getByRole("radio", { name })
const chosen = () => screen.getAllByRole("radio").find((r) => r.getAttribute("aria-checked") === "true")?.textContent
const show = () => render(<GeneralSection />, { wrapper })

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  Object.assign(state, { cls: "integrated", choice: "auto", platform: "windows", hybrid: false })
  state.settings = { ...state.settings, findBodies: false, gpu: "" }
})
afterEach(cleanup)

describe("Settings → General → Fast Scan", () => {
  it("Automatic says what it means here", async () => {
    show()
    const line = await screen.findByText(/^Here, Automatic means Fast Scan: this computer analyses video on its built-in graphics\./)
    expect(screen.getByRole("radiogroup", { name: "Fast Scan for new projects" })).toBeTruthy()
    expect(chosen()).toBe("Automatic")
    // The device isn't named here: Settings has one place for that, the
    // Graphics card row.
    expect(line.textContent).not.toMatch(/Iris Xe/)
    expect(await screen.findByText("Analysis runs on the Intel Iris Xe Graphics.")).toBeTruthy()
    expect(screen.getAllByText(/Iris Xe/)).toHaveLength(1)
    cleanup()

    state.cls = "forAi"
    queryClient.clear()
    show()
    expect(await screen.findByText("Here, Automatic means the full scan: this computer's graphics card is built for AI work.")).toBeTruthy()
    cleanup()

    Object.assign(state, { cls: "integrated", platform: "macos" })
    queryClient.clear()
    show()
    expect(await screen.findByText(/^Here, Automatic means Fast Scan: this Mac analyses video on its built-in graphics\./)).toBeTruthy()
  })

  it("On and Off say they apply to every new project", async () => {
    state.choice = "fast"
    show()
    expect(await screen.findByText("Every new project starts with Fast Scan, 3D animation too.")).toBeTruthy()
    expect(chosen()).toBe("On")
    cleanup()

    Object.assign(state, { cls: "forAi", choice: "full" })
    queryClient.clear()
    show()
    expect(await screen.findByText("Every new project starts with the full scan.")).toBeTruthy()
    expect(chosen()).toBe("Off")
  })

  it("a choice calls scanDefaultSet, shows its answer and refreshes the settings", async () => {
    show()
    await screen.findByText(/^Here, Automatic means Fast Scan/)
    await waitFor(() => expect(commands.settingsGet).toHaveBeenCalledTimes(1))
    fireEvent.click(choice("Off"))
    await waitFor(() => expect(commands.scanDefaultSet).toHaveBeenCalledWith("full"))
    expect(await screen.findByText("Every new project starts with the full scan.")).toBeTruthy()
    expect(chosen()).toBe("Off")
    // The settings' read-only `fastScan` is read again, never sent.
    await waitFor(() => expect(commands.settingsGet).toHaveBeenCalledTimes(2))
    expect(commands.settingsUpdate).not.toHaveBeenCalled()
    fireEvent.click(choice("Automatic"))
    await waitFor(() => expect(commands.scanDefaultSet).toHaveBeenLastCalledWith("auto"))
    expect(await screen.findByText(/^Here, Automatic means Fast Scan/)).toBeTruthy()
  })

  it("a failed choice says so", async () => {
    vi.mocked(commands.scanDefaultSet).mockRejectedValueOnce(new Error("The library database is locked."))
    show()
    await screen.findByText(/^Here, Automatic means Fast Scan/)
    fireEvent.click(choice("On"))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Couldn't save the setting", { description: "The library database is locked." }),
    )
    expect(chosen()).toBe("Automatic")
    expect(screen.getByText(/^Here, Automatic means Fast Scan/)).toBeTruthy()
  })

  it("the Fast Scan line follows the chosen card", async () => {
    const user = userEvent.setup()
    state.hybrid = true
    show()
    // Automatic: the RTX, built for AI work.
    expect(await screen.findByText("Here, Automatic means the full scan: this computer's graphics card is built for AI work.")).toBeTruthy()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 4060 Laptop GPU.")
    // Its built-in graphics chosen: Fast Scan on Automatic, said without naming them.
    screen.getByRole("combobox", { name: "Graphics card" }).focus()
    await user.keyboard("{Enter}")
    await user.click(await screen.findByRole("option", { name: "Intel Iris Xe Graphics" }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ gpu: CARDS[1].key })))
    expect(await screen.findByText(/^Here, Automatic means Fast Scan: this computer analyses video on its built-in graphics\./)).toBeTruthy()
    expect(await screen.findByText("Analysis runs on the Intel Iris Xe Graphics.")).toBeTruthy()
    expect(commands.scanDefaultSet).not.toHaveBeenCalled()
  })

  it("turning faceless people on refreshes the scan defaults", async () => {
    show()
    await screen.findByText(/^Here, Automatic means Fast Scan.* 3D animation keeps the full scan\.$/)
    fireEvent.click(screen.getByRole("switch", { name: "Find characters without a visible face" }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ findBodies: true })))
    expect(
      await screen.findByText(/and so does live action while Find characters without a visible face is on\.$/),
    ).toBeTruthy()
    expect(commands.scanDefaultsGet).toHaveBeenCalledTimes(2)
  })
})
