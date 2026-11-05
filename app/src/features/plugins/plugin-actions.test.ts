import { beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import type { PackNaming, PackSpec } from "@/bindings"
import { commands } from "@/lib/ipc"
import { takePackSave } from "./pack-saves"
import { qk, queryClient } from "@/lib/queries"
import { pluginInfo } from "@/test/plugins"
import { allowUnreviewed, runPackAction, setPluginEnabled } from "./plugin-actions"

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return { ...ipc, commands: { ...ipc.commands, packActionRun: vi.fn(), pluginSetEnabled: vi.fn(), pluginsAllowUnreviewed: vi.fn() } }
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

describe("turning a plugin on or off", () => {
  it("keeps the plugin as the backend gave it back", async () => {
    const one = pluginInfo({ id: "dev.a.one", name: "One", enabled: false })
    queryClient.setQueryData(qk.plugins, [one, pluginInfo({ id: "dev.a.two", name: "Two" })])
    vi.mocked(commands.pluginSetEnabled).mockResolvedValue({ ...one, enabled: true })
    await setPluginEnabled(one, true)
    expect(commands.pluginSetEnabled).toHaveBeenCalledExactlyOnceWith("dev.a.one", true)
    expect(queryClient.getQueryData<{ id: string; enabled: boolean }[]>(qk.plugins)?.map((p) => p.enabled)).toEqual([true, true])
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("says what it couldn't do, in its own words", async () => {
    vi.mocked(commands.pluginSetEnabled).mockRejectedValue({ code: "io", message: "The plugin's folder is locked." })
    await setPluginEnabled(pluginInfo({ id: "dev.a.one", name: "One" }), true)
    expect(toast.error).toHaveBeenLastCalledWith("Couldn't turn the plugin on", { description: "The plugin's folder is locked." })
    await setPluginEnabled(pluginInfo({ id: "dev.a.one", name: "One" }), false)
    expect(toast.error).toHaveBeenLastCalledWith("Couldn't turn the plugin off", { description: "The plugin's folder is locked." })
  })
})

describe("allowing plugins AutoScene hasn't reviewed", () => {
  const answer = (allowed: boolean) => ({ allowUnreviewedPlugins: allowed }) as never
  /** The system's dialog, open until `yes` answers it. */
  function dialog() {
    let yes!: (settings: never) => void
    const open = new Promise<never>((done) => (yes = done))
    return { open, yes }
  }

  it("opens one system dialog however often it is asked while that dialog is open", async () => {
    const { open, yes } = dialog()
    vi.mocked(commands.pluginsAllowUnreviewed).mockReturnValue(open)
    const first = allowUnreviewed(true)
    const second = allowUnreviewed(true)
    expect(commands.pluginsAllowUnreviewed).toHaveBeenCalledOnce()
    yes(answer(true))
    expect(await Promise.all([first, second])).toEqual([true, true])
  })

  it("asks again once the dialog is answered, and a failed ask doesn't stay in the way", async () => {
    vi.mocked(commands.pluginsAllowUnreviewed).mockRejectedValueOnce(new Error("no"))
    expect(await allowUnreviewed(true)).toBe(false)
    vi.mocked(commands.pluginsAllowUnreviewed).mockResolvedValue(answer(true))
    expect(await allowUnreviewed(true)).toBe(true)
    expect(commands.pluginsAllowUnreviewed).toHaveBeenCalledTimes(2)
  })

  it("turning them off never waits for a dialog", async () => {
    const { open, yes } = dialog()
    vi.mocked(commands.pluginsAllowUnreviewed).mockReturnValueOnce(open)
    const on = allowUnreviewed(true)
    vi.mocked(commands.pluginsAllowUnreviewed).mockResolvedValue(answer(false))
    expect(await allowUnreviewed(false)).toBe(false)
    yes(answer(false))
    await on
  })
})
