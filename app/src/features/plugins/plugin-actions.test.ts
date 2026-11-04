import { beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import type { PackNaming, PackSpec } from "@/bindings"
import { commands } from "@/lib/ipc"
import { takePackSave } from "./pack-saves"
import { runPackAction } from "./plugin-actions"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return { ...ipc, commands: { ...ipc.commands, packActionRun: vi.fn() } }
})

const run = { pluginId: "dev.example.fcpxml", pluginName: "FCPXML", actionId: "fcpxml", title: "Save as FCPXML…" }
const spec = { filters: [] } as unknown as PackSpec
const naming = { template: "{name}" } as unknown as PackNaming
const click = () => runPackAction(run, "p1", "c1", spec, naming, "stamp-1")

beforeEach(() => vi.clearAllMocks())

describe("saving a pack with a plugin's action", () => {
  it("asks for the pack the grid showed, by the plugin and action", async () => {
    vi.mocked(commands.packActionRun).mockResolvedValue({ kind: "queued", job: "j1" as never, path: "C:\\Walter.fcpxml" })
    await click()
    expect(commands.packActionRun).toHaveBeenCalledExactlyOnceWith("p1", "c1", "dev.example.fcpxml/fcpxml", spec, naming, "stamp-1")
    // The run shows in Activity and says when it ends: nothing more now.
    expect(toast).not.toHaveBeenCalled()
    // Where its file goes is kept for that end, which offers to show it.
    expect(takePackSave("j1")).toBe("C:\\Walter.fcpxml")
    expect(takePackSave("j1")).toBeUndefined()
  })

  it("keeps no place for a save that did not start", async () => {
    vi.mocked(commands.packActionRun).mockResolvedValue({ kind: "waits" })
    await click()
    vi.mocked(commands.packActionRun).mockResolvedValue({ kind: "cancelled" })
    await click()
    expect(takePackSave("j1")).toBeUndefined()
  })

  it("says nothing when the save dialog was closed", async () => {
    vi.mocked(commands.packActionRun).mockResolvedValue({ kind: "cancelled" })
    await click()
    expect(toast).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("says the same save waits or runs already, named by its plugin", async () => {
    vi.mocked(commands.packActionRun).mockResolvedValue({ kind: "waits" })
    await click()
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “FCPXML”: Save as FCPXML is already waiting or running", {
      description: "It's in Activity.",
    })
  })

  it("says why it couldn't start, and a changed pack is the backend's own words", async () => {
    vi.mocked(commands.packActionRun).mockRejectedValue({ code: "invalid", message: "The pack changed meanwhile. Check it and export again." })
    await click()
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin “FCPXML”: Save as FCPXML couldn't start", {
      description: "The pack changed meanwhile. Check it and export again.",
    })
  })
})
