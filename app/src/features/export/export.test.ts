import { afterEach, describe, expect, it, vi } from "vitest"
import type { AppSettings } from "@/bindings"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { rememberPreset } from "./export"

vi.mock("@/lib/ipc", () => ({
  commands: { settingsUpdate: vi.fn() },
  errorMessage: (e: unknown) => String(e),
}))

const settings = { libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng" } as AppSettings

afterEach(() => {
  queryClient.clear()
  vi.restoreAllMocks()
  vi.clearAllMocks()
})

describe("remembering the export format", () => {
  it("saves a format other than the remembered one", async () => {
    vi.mocked(commands.settingsUpdate).mockImplementation((s) => Promise.resolve(s))
    rememberPreset(settings, "prores-hq")
    rememberPreset(undefined, "h264")
    expect(commands.settingsUpdate).not.toHaveBeenCalled()
    rememberPreset(settings, "h264")
    expect(commands.settingsUpdate).toHaveBeenCalledWith({ ...settings, exportPresetId: "h264" })
    await vi.waitFor(() => expect(queryClient.getQueryData(qk.settings)).toMatchObject({ exportPresetId: "h264" }))
  })

  it("notes a failure in the log instead of leaving it unhandled", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {})
    vi.mocked(commands.settingsUpdate).mockRejectedValue("library drive unplugged")
    rememberPreset(settings, "h264")
    await vi.waitFor(() => expect(warn).toHaveBeenCalledWith("remembering the export format:", "library drive unplugged"))
    expect(queryClient.getQueryData(qk.settings)).toBeUndefined()
  })
})

describe("rememberPreset's options", () => {
  it("saves options that changed and leaves alone what didn't", async () => {
    const settings = { exportPresetId: "h264", exportOptions: { removeBars: false, matchLoudness: false } } as AppSettings
    const update = vi.mocked(commands.settingsUpdate)
    update.mockClear()
    update.mockResolvedValue(settings)
    rememberPreset(settings, "h264", { removeBars: false, matchLoudness: false })
    expect(update).not.toHaveBeenCalled()
    rememberPreset(settings, "h264", { removeBars: true, matchLoudness: false })
    expect(update).toHaveBeenCalledWith({ ...settings, exportOptions: { removeBars: true, matchLoudness: false } })
  })
})
