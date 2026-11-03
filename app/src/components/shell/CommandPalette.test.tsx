import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { PluginInfo } from "@/bindings"
import { useUi } from "@/state/ui"
import { pluginInfo } from "@/test/plugins"
import { CommandPalette } from "./CommandPalette"

const { at, pluginRun } = vi.hoisted(() => ({ at: { path: "/", plugins: [] as PluginInfo[] }, pluginRun: vi.fn() }))

vi.mock("@tanstack/react-router", () => ({
  useRouterState: ({ select }: { select: (s: { location: { pathname: string } }) => unknown }) => select({ location: { pathname: at.path } }),
  useNavigate: () => vi.fn(),
}))
vi.mock("@/lib/queries", () => ({
  useProjects: () => ({ data: [] }),
  usePlugins: () => ({ data: at.plugins }),
  useCast: () => ({ data: undefined }),
  useMediaList: () => ({ data: undefined }),
}))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings: vi.fn() }) }))
vi.mock("@/lib/shortcuts", () => ({ useShortcutKeys: () => () => [] }))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return { ...ipc, commands: { ...ipc.commands, pluginRun } }
})
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

const command = (id: string, needsProject: boolean) => ({ id, title: `Run ${id}`, description: null, needsProject })

function open(path: string) {
  at.path = path
  useUi.setState({ paletteOpen: true })
  render(<CommandPalette />)
}

beforeEach(() => {
  vi.clearAllMocks()
  pluginRun.mockResolvedValue("j1")
  at.plugins = [pluginInfo({ id: "dev.a.one", name: "One", commands: [command("anywhere", false), command("here", true)] })]
})
afterEach(cleanup)

describe("the command palette's plugin commands", () => {
  it("run in the open project", async () => {
    open("/p/p1/exports")
    fireEvent.click(screen.getByRole("option", { name: /Run here/ }))
    await waitFor(() => expect(pluginRun).toHaveBeenCalledExactlyOnceWith("dev.a.one", "here", "command", "p1", null))
    cleanup()

    // Outside one, only those that need none, with no project.
    open("/settings/plugins")
    expect(screen.queryByRole("option", { name: /Run here/ })).toBeNull()
    fireEvent.click(screen.getByRole("option", { name: /Run anywhere/ }))
    await waitFor(() => expect(pluginRun).toHaveBeenCalledTimes(2))
    expect(pluginRun).toHaveBeenLastCalledWith("dev.a.one", "anywhere", "command", null, null)
    expect(toast).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("a plugin command's toast names its plugin first", async () => {
    pluginRun.mockResolvedValue(null)
    open("/p/p1/exports")
    fireEvent.click(screen.getByRole("option", { name: /Run here/ }))
    // The command's title is the plugin's own words, so its plugin comes first.
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith("Plugin “One”: Run here is already waiting or running", { description: "It's in Activity." }),
    )
    cleanup()

    pluginRun.mockRejectedValue({ code: "invalid", message: "One is turned off." })
    open("/p/p1/exports")
    fireEvent.click(screen.getByRole("option", { name: /Run here/ }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Plugin “One”: Run here couldn't start", { description: "One is turned off." }),
    )
  })
})
