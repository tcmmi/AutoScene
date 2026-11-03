import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, GraphicsClass, Platform, ScanChoice, ScanDefaults } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { GeneralSection } from "./GeneralSection"

const { state } = vi.hoisted(() => ({
  state: {
    cls: "integrated" as GraphicsClass,
    choice: "auto" as ScanChoice,
    platform: "windows" as Platform,
    settings: { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng", findBodies: false } as AppSettings,
  },
}))

/** What the library answers, like `as_project::scan::defaults`. */
function defaults(): ScanDefaults {
  const { cls, choice } = state
  const here = cls === "notForAi" || cls === "integrated" || cls === "processor"
  const fast = (content: string) =>
    choice === "auto" ? here && content !== "animation" && (content === "anime" || !state.settings.findBodies) : choice === "fast"
  return {
    computer: { class: cls, name: "Intel Iris Xe Graphics" },
    choice,
    footage: (["live", "mixed", "animation", "anime"] as const).map((content) => ({ content, fast: fast(content) })),
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
  Object.assign(state, { cls: "integrated", choice: "auto", platform: "windows" })
  state.settings = { ...state.settings, findBodies: false }
})
afterEach(cleanup)

describe("Settings → General → Fast Scan", () => {
  it("Automatic says what it means here", async () => {
    show()
    expect(await screen.findByText(/^Here, Automatic means Fast Scan: this computer analyses video on its built-in graphics\./)).toBeTruthy()
    expect(screen.getByRole("radiogroup", { name: "Fast Scan for new projects" })).toBeTruthy()
    expect(chosen()).toBe("Automatic")
    // The device isn't named here: Settings has one place for that.
    expect(screen.queryByText(/Iris Xe/)).toBeNull()
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
