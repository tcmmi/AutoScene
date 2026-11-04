import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, HardwareInfo, Platform } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { HardwareGroup } from "./HardwareGroup"

const RTX = { key: "10de:24c9:NVIDIA GeForce RTX 3060 Ti", name: "NVIDIA GeForce RTX 3060 Ti" }
const IRIS = { key: "8086:a7a0:Intel Iris Xe Graphics", name: "Intel Iris Xe Graphics" }

const { state } = vi.hoisted(() => ({
  state: {
    platform: "windows" as Platform,
    settings: { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", gpu: "" } as AppSettings,
    hardware: null as unknown as HardwareInfo,
  },
}))

/** One card: Automatic is that card. */
function oneCard(over: Partial<HardwareInfo> = {}): HardwareInfo {
  return {
    gpus: [RTX],
    choice: "autoOrProcessor",
    selected: "auto",
    using: { kind: "gpu", name: RTX.name },
    missing: false,
    pending: false,
    fellBack: false,
    ...over,
  }
}

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    appInfo: vi.fn(() => Promise.resolve({ version: "1.1.0", platform: state.platform, arch: "x86_64", dataDir: "C:\\Data" })),
    settingsGet: vi.fn(() => Promise.resolve(state.settings)),
    settingsUpdate: vi.fn((s: AppSettings) => {
      state.settings = s
      return Promise.resolve(s)
    }),
    hardwareInfo: vi.fn(() => Promise.resolve(state.hardware)),
  },
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

const show = () => render(<HardwareGroup />, { wrapper })
const select = (name = "Graphics card") => screen.getByRole("combobox", { name })

async function options(user: ReturnType<typeof userEvent.setup>, name = "Graphics card") {
  select(name).focus()
  await user.keyboard("{Enter}")
  const list = await screen.findByRole("listbox")
  return within(list)
    .getAllByRole("option")
    .map((o) => o.textContent)
}

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  state.platform = "windows"
  state.settings = { ...state.settings, gpu: "" }
  state.hardware = oneCard()
})
afterEach(cleanup)

describe("Settings → General → Graphics card", () => {
  it("one card: Automatic or the processor only, and the card named once", async () => {
    const user = userEvent.setup()
    show()
    expect(await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")).toBeTruthy()
    expect(screen.getByText("Hardware acceleration")).toBeTruthy()
    expect(select().textContent).toBe("Automatic")
    expect(await options(user)).toEqual(["Automatic", "Processor only"])
  })

  it("two cards: each by name, and a choice is saved and shown", async () => {
    const user = userEvent.setup()
    state.hardware = oneCard({ gpus: [RTX, IRIS], choice: "cards" })
    vi.mocked(commands.settingsUpdate).mockImplementationOnce((s: AppSettings) => {
      state.settings = s
      state.hardware = oneCard({ gpus: [RTX, IRIS], choice: "cards", selected: IRIS.key, using: { kind: "gpu", name: IRIS.name } })
      return Promise.resolve(s)
    })
    const invalidate = vi.spyOn(queryClient, "invalidateQueries")
    show()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")
    expect(await options(user)).toEqual(["Automatic", RTX.name, IRIS.name, "Processor only"])
    await user.click(screen.getByRole("option", { name: IRIS.name }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ gpu: IRIS.key })))
    expect(await screen.findByText("Analysis runs on the Intel Iris Xe Graphics.")).toBeTruthy()
    expect(select().textContent).toBe(IRIS.name)
    // Fast Scan's line follows the card.
    expect(invalidate).toHaveBeenCalledWith({ queryKey: qk.scanDefaults })
    // Back to Automatic stores nothing.
    await options(user)
    await user.click(screen.getByRole("option", { name: "Automatic" }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenLastCalledWith(expect.objectContaining({ gpu: "" })))
  })

  it("the processor only says it is much slower", async () => {
    const user = userEvent.setup()
    vi.mocked(commands.settingsUpdate).mockImplementationOnce((s: AppSettings) => {
      state.settings = s
      state.hardware = oneCard({ selected: "cpu", using: { kind: "processor" } })
      return Promise.resolve(s)
    })
    show()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")
    await options(user)
    await user.click(screen.getByRole("option", { name: "Processor only" }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ gpu: "cpu" })))
    expect(await screen.findByText("Analysis runs on the processor. That is much slower.")).toBeTruthy()
  })

  it("nothing to choose: no control, and where the analysis runs", async () => {
    state.hardware = oneCard({ gpus: [], choice: "fixed", using: { kind: "processor" } })
    show()
    expect(await screen.findByText("Analysis runs on the processor.")).toBeTruthy()
    expect(screen.queryByRole("combobox")).toBeNull()
    cleanup()
    // Linux has none, not even while it looks.
    queryClient.clear()
    state.platform = "linux"
    vi.mocked(commands.hardwareInfo).mockReturnValueOnce(new Promise(() => {}))
    show()
    expect(await screen.findByText("Looking for graphics cards…")).toBeTruthy()
    await waitFor(() => expect(queryClient.getQueryData(qk.appInfo)).toBeTruthy())
    expect(screen.queryByRole("combobox")).toBeNull()
  })

  it("Apple silicon: the chip, Automatic or the processor only", async () => {
    const user = userEvent.setup()
    state.platform = "macos"
    state.hardware = oneCard({ gpus: [], using: { kind: "chip", name: "Apple M2" } })
    show()
    expect(await screen.findByText("Analysis runs on the Apple M2.")).toBeTruthy()
    expect(screen.getByText("Chip")).toBeTruthy()
    expect(await options(user, "Chip")).toEqual(["Automatic", "Processor only"])
  })

  it("Apple silicon: looking and failing speak of the chip, not graphics cards", async () => {
    state.platform = "macos"
    await queryClient.fetchQuery({ queryKey: qk.appInfo, queryFn: commands.appInfo })
    vi.mocked(commands.hardwareInfo).mockReturnValueOnce(new Promise(() => {}))
    show()
    expect(await screen.findByText("Checking the chip…")).toBeTruthy()
    expect(screen.queryByText("Looking for graphics cards…")).toBeNull()
    cleanup()
    queryClient.clear()
    await queryClient.fetchQuery({ queryKey: qk.appInfo, queryFn: commands.appInfo })
    vi.mocked(commands.hardwareInfo).mockRejectedValueOnce({ code: "internal", message: "DXCore failed" })
    show()
    expect(await screen.findByText("Couldn't check the chip.")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy()
  })

  it("while it looks, the choice waits", async () => {
    vi.mocked(commands.hardwareInfo).mockReturnValueOnce(new Promise(() => {}))
    show()
    expect(await screen.findByText("Looking for graphics cards…")).toBeTruthy()
    const waiting = await screen.findByRole("combobox", { name: "Graphics card" })
    expect(waiting.hasAttribute("disabled") || waiting.hasAttribute("data-disabled")).toBe(true)
  })

  it("a failed look says so, and Try again asks again", async () => {
    const user = userEvent.setup()
    vi.mocked(commands.hardwareInfo).mockRejectedValueOnce({ code: "internal", message: "Couldn't check the graphics cards." })
    show()
    expect(await screen.findByText("Couldn't check the graphics cards.")).toBeTruthy()
    expect(screen.queryByRole("combobox")).toBeNull()
    await user.click(screen.getByRole("button", { name: "Try again" }))
    expect(await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")).toBeTruthy()
    expect(commands.hardwareInfo).toHaveBeenCalledTimes(2)
  })

  it("a failed save says so", async () => {
    const user = userEvent.setup()
    vi.mocked(commands.settingsUpdate).mockRejectedValueOnce(new Error("The library database is locked."))
    show()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")
    await options(user)
    await user.click(screen.getByRole("option", { name: "Processor only" }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Couldn't save the setting", { description: "The library database is locked." }),
    )
    expect(select().textContent).toBe("Automatic")
  })

  it("says when a change waits, a card is missing, or the last analysis fell back", async () => {
    // First that applies wins.
    const cases: [Partial<HardwareInfo>, string][] = [
      [{ pending: true, missing: true, fellBack: true }, "Applies from the next analysis."],
      [
        { missing: true, fellBack: true },
        "The graphics card you chose isn't connected, so analysis uses the NVIDIA GeForce RTX 3060 Ti.",
      ],
      [{ fellBack: true }, "In the last analysis some steps ran on the processor, which is slower."],
      [{ using: { kind: "gpu", name: "" } }, "Analysis runs on this computer's graphics card."],
    ]
    for (const [over, line] of cases) {
      queryClient.clear()
      state.hardware = oneCard(over)
      show()
      expect(await screen.findByText(line)).toBeTruthy()
      // The choice stays where it was: a missing card shows Automatic.
      expect(select().textContent).toBe("Automatic")
      cleanup()
    }
  })
})
