import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AccelInfo, AccelRow, AppSettings, HardwareInfo, Platform } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { openExternalUrl } from "@/lib/os"
import { qk, queryClient } from "@/lib/queries"
import { inLanguage } from "@/test/i18n"
import { HardwareGroup } from "./HardwareGroup"

const named = (key: string, name: string) => ({ key, name: { key: "", text: name } })
const RTX = named("10de:24c9:NVIDIA GeForce RTX 3060 Ti", "NVIDIA GeForce RTX 3060 Ti")
const IRIS = named("8086:a7a0:Intel Iris Xe Graphics", "Intel Iris Xe Graphics")
const LICENCE = "https://docs.nvidia.com/deeplearning/tensorrt-rtx/latest/reference/sla.html"

const { state } = vi.hoisted(() => ({
  state: {
    platform: "windows" as Platform,
    settings: { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", gpu: "" } as AppSettings,
    hardware: null as unknown as HardwareInfo,
  },
}))

/** NVIDIA acceleration with `row`. */
function accel(row: AccelRow, over: Partial<AccelInfo> = {}): AccelInfo {
  return { row, installed: false, removable: false, active: false, updateStopped: false, downloadBytes: 110_866_443, licence: LICENCE, engine: "1.7.1", ...over }
}

/** One card: Automatic is that card. */
function oneCard(over: Partial<HardwareInfo> = {}): HardwareInfo {
  return {
    gpus: [RTX],
    choice: "autoOrProcessor",
    selected: "auto",
    using: { kind: "gpu", name: RTX.name.text },
    missing: false,
    pending: false,
    fellBack: false,
    runtime: "1.24.4",
    accel: accel({ state: "hidden" }),
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
    accelInstall: vi.fn(() => Promise.resolve()),
    accelCancel: vi.fn(() => Promise.resolve()),
    accelRemove: vi.fn(() => Promise.resolve()),
    accelTryAgain: vi.fn(() => Promise.resolve()),
  },
}))
vi.mock("@/lib/os", () => ({ openExternalUrl: vi.fn(() => Promise.resolve()) }))
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

  it("a card its driver gives no name is called so in the UI's language", async () => {
    const user = userEvent.setup()
    await inLanguage("ro", { backend: { hardware: { unnamedCard: "Placă video" } } })
    const unnamed = { key: "10de:24c9:Graphics card", name: { key: "hardware.unnamedCard", text: "Graphics card" } }
    state.hardware = oneCard({ gpus: [unnamed, IRIS], choice: "cards", using: { kind: "gpu", name: "" } })
    show()
    await screen.findByText("Analysis runs on this computer's graphics card.")
    expect(await options(user)).toEqual(["Automatic", "Placă video", IRIS.name.text, "Processor only"])
  })

  it("two cards: each by name, and a choice is saved and shown", async () => {
    const user = userEvent.setup()
    state.hardware = oneCard({ gpus: [RTX, IRIS], choice: "cards" })
    vi.mocked(commands.settingsUpdate).mockImplementationOnce((s: AppSettings) => {
      state.settings = s
      state.hardware = oneCard({ gpus: [RTX, IRIS], choice: "cards", selected: IRIS.key, using: { kind: "gpu", name: IRIS.name.text } })
      return Promise.resolve(s)
    })
    const invalidate = vi.spyOn(queryClient, "invalidateQueries")
    show()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")
    expect(await options(user)).toEqual(["Automatic", RTX.name.text, IRIS.name.text, "Processor only"])
    await user.click(screen.getByRole("option", { name: IRIS.name.text }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ gpu: IRIS.key })))
    expect(await screen.findByText("Analysis runs on the Intel Iris Xe Graphics.")).toBeTruthy()
    expect(select().textContent).toBe(IRIS.name.text)
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

describe("Settings → General → Graphics card that is gone", () => {
  it("choosing Automatic clears the stored card, though the row already shows Automatic", async () => {
    const user = userEvent.setup()
    state.settings = { ...state.settings, gpu: IRIS.key }
    state.hardware = oneCard({ missing: true })
    vi.mocked(commands.settingsUpdate).mockImplementationOnce((s: AppSettings) => {
      state.settings = s
      state.hardware = oneCard()
      return Promise.resolve(s)
    })
    show()
    await screen.findByText(/isn't connected/)
    expect(select().textContent).toBe("Automatic")
    await options(user)
    await user.click(screen.getByRole("option", { name: "Automatic" }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ gpu: "" })))
    // The card is no longer chosen, so the line about it goes.
    expect(await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")).toBeTruthy()
    expect(screen.queryByText(/isn't connected/)).toBeNull()
  })

  it("a stored card with fewer than two cards left clears the same way", async () => {
    const user = userEvent.setup()
    state.settings = { ...state.settings, gpu: IRIS.key }
    state.hardware = oneCard()
    show()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")
    expect(select().textContent).toBe("Automatic")
    await options(user)
    await user.click(screen.getByRole("option", { name: "Automatic" }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ gpu: "" })))
  })

  it("choosing the processor while the card is gone still works", async () => {
    const user = userEvent.setup()
    state.settings = { ...state.settings, gpu: IRIS.key }
    state.hardware = oneCard({ missing: true })
    show()
    await screen.findByText(/isn't connected/)
    await options(user)
    await user.click(screen.getByRole("option", { name: "Processor only" }))
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ gpu: "cpu" })))
  })

  it("Automatic chosen again, with nothing stored, saves nothing", async () => {
    const user = userEvent.setup()
    show()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")
    await options(user)
    await user.click(screen.getByRole("option", { name: "Automatic" }))
    expect(commands.settingsUpdate).not.toHaveBeenCalled()
  })
})

describe("Settings → General → NVIDIA acceleration", () => {
  /** The row in state `row`. */
  async function row(r: AccelRow, over: Partial<AccelInfo> = {}, card: Partial<HardwareInfo> = {}) {
    state.hardware = oneCard({ accel: accel(r, over), ...card })
    show()
    return screen.findByText("NVIDIA acceleration")
  }
  const installed = { installed: true, removable: true }
  const remove = () => screen.queryByRole("button", { name: "Remove NVIDIA's files" })

  it("isn't there where this computer can't use it and has none of its files", async () => {
    show()
    await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti.")
    expect(screen.queryByText("NVIDIA acceleration")).toBeNull()
  })

  it("available: what it does, what travels, whose licence, and the person agrees by downloading", async () => {
    const user = userEvent.setup()
    await row({ state: "available" })
    const line = screen.getByText(/NVIDIA's engine for GeForce RTX cards/)
    expect(line.textContent).toContain("so anime analysis runs faster")
    expect(line.textContent).toContain("(about 111 MB) from the Python Package Index, which sees your IP address")
    expect(line.textContent).toContain("By downloading you agree to NVIDIA's licence.")
    await user.click(screen.getByRole("button", { name: "NVIDIA's licence" }))
    expect(openExternalUrl).toHaveBeenCalledWith(LICENCE)
    expect(commands.accelInstall).not.toHaveBeenCalled()
    await user.click(screen.getByRole("button", { name: "Agree and download" }))
    expect(commands.accelInstall).toHaveBeenCalledOnce()
    expect(remove()).toBeNull()
  })

  it("available with files here: a new licence or half a download, and they can go without agreeing", async () => {
    await row({ state: "available" }, { removable: true })
    expect(screen.getByRole("button", { name: "Agree and download" })).toBeTruthy()
    expect(remove()).toBeTruthy()
  })

  it("downloading: the bar, how far, and Cancel", async () => {
    const user = userEvent.setup()
    await row({ state: "downloading", done: 45_000_000, total: 110_866_443 })
    expect(screen.getByText("Downloading NVIDIA's files.")).toBeTruthy()
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("41")
    expect(screen.getByText("45 of 111 MB")).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Cancel" }))
    expect(commands.accelCancel).toHaveBeenCalledOnce()
    expect(remove()).toBeNull()
  })

  it("downloading: a screen reader hears it start and end, not every megabyte", async () => {
    await row({ state: "downloading", done: 45_000_000, total: 110_866_443 })
    const heard = [...document.querySelectorAll("[aria-live]")].map((e) => e.textContent).join(" ")
    expect(heard).toContain("Downloading NVIDIA's files.")
    expect(heard).not.toContain("45 of 111 MB")
  })

  it("downloading an update: the files that are here can't go meanwhile", async () => {
    await row({ state: "downloading", done: 45_000_000, total: 110_866_443 }, installed)
    expect(remove()).toBeNull()
  })

  it("download failed: the reason, and Try again downloads again", async () => {
    const user = userEvent.setup()
    await row({ state: "failed", message: { key: "", text: "NVIDIA's files couldn't be downloaded. Check the internet connection and try again." } })
    expect(screen.getByText(/couldn't be downloaded/)).toBeTruthy()
    const group = screen.getByText("NVIDIA acceleration").closest("div")?.parentElement?.parentElement as HTMLElement
    await user.click(within(group).getByRole("button", { name: "Try again" }))
    expect(commands.accelInstall).toHaveBeenCalledOnce()
    expect(remove()).toBeNull()
  })

  it("an update the person stopped can be taken up from the row, and only then", async () => {
    const user = userEvent.setup()
    const offer = () => screen.queryByRole("button", { name: /Download the update/ })
    await row({ state: "ready" }, { ...installed, updateStopped: true, downloadBytes: 111_000_000 })
    expect(offer()?.textContent).toBe("Download the update (about 111 MB)")
    expect(screen.getByRole("switch", { name: "NVIDIA acceleration" })).toBeTruthy()
    await user.click(offer() as HTMLElement)
    expect(commands.accelInstall).toHaveBeenCalledOnce()
    cleanup()
    queryClient.clear()
    await row({ state: "ready" }, installed)
    expect(offer()).toBeNull()
  })

  it("download failed: the reason is in the UI's language", async () => {
    await inLanguage("ro", { backend: { accel: { noSpace: "Eliberează {{mb}} MB." } } })
    await row({ state: "failed", message: { key: "accel.noSpace", args: { mb: 124 }, text: "Free up 124 MB." } })
    expect(screen.getByText("Eliberează 124 MB.")).toBeTruthy()
  })

  it("download failed with files here: they can go too", async () => {
    await row({ state: "failed", message: { key: "", text: "The download was cancelled." } }, { removable: true })
    expect(remove()).toBeTruthy()
  })

  it("on and off: the switch saves the setting, and the graphics line says when it is in use", async () => {
    const user = userEvent.setup()
    await row({ state: "ready" }, { ...installed, active: true })
    expect(await screen.findByText("Analysis runs on the NVIDIA GeForce RTX 3060 Ti, with NVIDIA acceleration.")).toBeTruthy()
    const on = screen.getByRole("switch", { name: "NVIDIA acceleration" })
    expect(on.getAttribute("aria-checked")).toBe("true")
    await user.click(on)
    await waitFor(() =>
      expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ nvidiaAcceleration: false })),
    )
    await waitFor(() => expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("false"))
    expect(remove()).toBeTruthy()
    state.settings = { ...state.settings, nvidiaAcceleration: true }
  })

  it("remove asks first, and only Remove removes", async () => {
    const user = userEvent.setup()
    await row({ state: "ready" }, installed)
    await user.click(remove() as HTMLElement)
    const dialog = await screen.findByRole("dialog", { name: "Remove NVIDIA acceleration?" })
    expect(
      within(dialog).getByText("Analysis keeps running on this graphics card, more slowly. You can download it again any time."),
    ).toBeTruthy()
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }))
    expect(commands.accelRemove).not.toHaveBeenCalled()
    await user.click(remove() as HTMLElement)
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Remove" }))
    expect(commands.accelRemove).toHaveBeenCalledOnce()
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  })

  it("remove says what it costs only where analysis uses the files", async () => {
    const user = userEvent.setup()
    const says = async () => {
      await user.click(remove() as HTMLElement)
      const line = (await screen.findByRole("dialog")).textContent
      cleanup()
      queryClient.clear()
      return line
    }
    // The processor, another card, a pack that stopped, and the switch off.
    await row({ state: "notUsed" }, installed, { selected: "cpu", using: { kind: "processor" } })
    expect(await says()).not.toMatch(/graphics card|slowly/)
    await row({ state: "notUsed" }, installed)
    expect(await says()).not.toMatch(/graphics card|slowly/)
    await row({ state: "stopped" }, installed)
    expect(await says()).not.toMatch(/graphics card|slowly/)
    state.settings = { ...state.settings, nvidiaAcceleration: false }
    await row({ state: "ready" }, installed)
    await screen.findByRole("switch", { name: "NVIDIA acceleration", checked: false })
    expect(await says()).not.toMatch(/graphics card|slowly/)
    state.settings = { ...state.settings, nvidiaAcceleration: true }
    await row({ state: "available" }, { removable: true })
    expect(await says()).toContain("You can download it again any time.")
  })

  it("a removal that runs can't be dismissed: Esc, Cancel and the x do nothing until it ends", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(commands.accelRemove).mockImplementationOnce(() => new Promise((done) => (finish = () => done(null as never))))
    await row({ state: "ready" }, installed)
    await user.click(remove() as HTMLElement)
    const dialog = await screen.findByRole("dialog")
    await user.click(within(dialog).getByRole("button", { name: "Remove" }))
    await waitFor(() => expect(within(dialog).getByRole("button", { name: "Remove" }).getAttribute("aria-busy")).toBe("true"))
    await user.keyboard("{Escape}")
    expect(screen.getByRole("dialog")).toBeTruthy()
    // A press outside, on the overlay.
    const overlay = document.querySelector(".fixed.inset-0")
    expect(overlay).toBeTruthy()
    fireEvent.pointerDown(overlay as Element)
    fireEvent.pointerUp(overlay as Element)
    fireEvent.click(overlay as Element)
    expect(screen.getByRole("dialog")).toBeTruthy()
    expect(within(dialog).getByRole("button", { name: "Cancel" }).hasAttribute("disabled")).toBe(true)
    expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull()
    finish()
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  })

  it("a removal that fails says so", async () => {
    const user = userEvent.setup()
    vi.mocked(commands.accelRemove).mockRejectedValueOnce(new Error("NVIDIA's files couldn't be removed."))
    await row({ state: "ready" }, installed)
    await user.click(remove() as HTMLElement)
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Remove" }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Couldn't remove NVIDIA's files", {
        description: "NVIDIA's files couldn't be removed.",
      }),
    )
    expect(screen.getByRole("dialog")).toBeTruthy()
  })

  it("not used: says why, the switch waits, and the files can still go", async () => {
    await row({ state: "notUsed" }, installed)
    expect(screen.getByText(/^Not used: another graphics card is chosen\./)).toBeTruthy()
    const off = screen.getByRole("switch", { name: "NVIDIA acceleration" })
    expect(off.hasAttribute("disabled") || off.hasAttribute("data-disabled")).toBe(true)
    expect(remove()).toBeTruthy()
    cleanup()
    queryClient.clear()
    await row({ state: "notUsed" }, installed, { selected: "cpu", using: { kind: "processor" } })
    expect(screen.getByText(/^Not used: analysis runs on the processor\./)).toBeTruthy()
  })

  it("needs a newer driver: the soft line, and NVIDIA's driver page", async () => {
    const user = userEvent.setup()
    await row({ state: "needsDriver" }, installed)
    expect(screen.getByText(/^NVIDIA acceleration couldn't use this card\. A newer NVIDIA driver may help\./)).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Get the driver" }))
    expect(openExternalUrl).toHaveBeenCalledWith("https://www.nvidia.com/drivers/")
    expect(remove()).toBeTruthy()
  })

  it("stopped working: says analysis goes on without it, and Try again tries", async () => {
    const user = userEvent.setup()
    await row({ state: "stopped" }, installed)
    expect(screen.getByText(/^NVIDIA acceleration stopped working, so analysis runs without it\./)).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Try again" }))
    expect(commands.accelTryAgain).toHaveBeenCalledOnce()
    expect(commands.accelInstall).not.toHaveBeenCalled()
    expect(remove()).toBeTruthy()
  })

  it("stopped working: Try again waits for the check, and a second click doesn't start another", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(commands.accelTryAgain).mockImplementationOnce(() => new Promise((done) => (finish = () => done(null as never))))
    await row({ state: "stopped" }, installed)
    const button = screen.getByRole<HTMLButtonElement>("button", { name: "Try again" })
    await user.click(button)
    await waitFor(() => expect(button.getAttribute("aria-busy")).toBe("true"))
    await user.click(button)
    expect(commands.accelTryAgain).toHaveBeenCalledOnce()
    finish()
    await waitFor(() => expect(button.getAttribute("aria-busy")).toBeNull())
  })

  it("stopped working: a Try again that fails says so, and can be tried again", async () => {
    const user = userEvent.setup()
    vi.mocked(commands.accelTryAgain).mockRejectedValueOnce(new Error("No."))
    await row({ state: "stopped" }, installed)
    const button = screen.getByRole<HTMLButtonElement>("button", { name: "Try again" })
    await user.click(button)
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't try again", { description: "No." }))
    await waitFor(() => expect(button.disabled).toBe(false))
  })

  it("removing: says when, and offers nothing", async () => {
    await row({ state: "removing" }, installed)
    expect(screen.getByText("Removing after the current analysis.")).toBeTruthy()
    expect(screen.queryByRole("switch")).toBeNull()
    expect(remove()).toBeNull()
  })

  it("engine names stay in About", async () => {
    for (const r of [{ state: "available" }, { state: "ready" }, { state: "stopped" }] as const) {
      queryClient.clear()
      await row(r, installed)
      expect(document.body.textContent).not.toMatch(/TensorRT|DirectML|CUDA|ONNX/)
      cleanup()
    }
  })
})
