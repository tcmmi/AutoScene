import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, PluginReview } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { useInstallFlow } from "@/features/plugins/install-plugin"
import { InstallReviewDialog } from "@/features/plugins/InstallReviewDialog"
import { commands } from "@/lib/ipc"
import { pickFolder, pickPluginFile } from "@/lib/media"
import { queryClient } from "@/lib/queries"
import { usePluginReview } from "@/state/plugin-review"
import { useUi } from "@/state/ui"
import { pluginInfo } from "@/test/plugins"
import { AppMenu } from "./AppMenu"
import { Rail } from "./Rail"

const { state } = vi.hoisted(() => ({ state: { allow: false } }))

const settings = (): AppSettings => ({
  libraryRoot: "C:\\Library",
  exportPresetId: "prores-hq",
  audioLanguage: "eng",
  allowUnreviewedPlugins: state.allow,
})

vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return {
    ...ipc,
    commands: {
      ...ipc.commands,
      settingsGet: vi.fn(() => Promise.resolve(settings())),
      projectsList: vi.fn(() => Promise.resolve([])),
      pluginsList: vi.fn(() => Promise.resolve([])),
      pluginInspect: vi.fn(),
      pluginInspectFolder: vi.fn(),
    },
  }
})
vi.mock("@/lib/media", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/media")>()),
  pickFolder: vi.fn(),
  pickPluginFile: vi.fn(),
}))

/** The rail in the window's one menu, hidden as the window hides it, and the window's one install review. */
function show() {
  const root = createRootRoute({
    component: function Window() {
      const railHidden = useUi((s) => s.sidebarCollapsed)
      return (
        <AppMenu>
          <div>
            {!railHidden && <Rail />}
            <InstallReviewDialog />
          </div>
        </AppMenu>
      )
    },
  })
  const plugins = createRoute({ getParentRoute: () => root, path: "/plugins", component: () => null })
  const router = createRouter({ routeTree: root.addChildren([plugins]), history: createMemoryHistory({ initialEntries: ["/plugins"] }) })
  render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )
}

/** What a right-click on the Plugins tile lists. */
async function tileMenu() {
  fireEvent.contextMenu(await screen.findByRole("link", { name: "Plugins" }), { clientX: 40, clientY: 40 })
  return screen.findByRole("menu")
}
const names = (menu: HTMLElement) => within(menu).getAllByRole("menuitem").map((i) => i.textContent?.trim())

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  state.allow = false
  useUi.setState({ sidebarCollapsed: false })
  useInstallFlow.setState({ checking: null, busy: false })
  usePluginReview.setState({ review: null, loading: false, queue: [] })
})

const review = (): PluginReview => ({
  ticketId: "t",
  plugin: pluginInfo({ id: "dev.a.one", name: "One" }),
  update: null,
  needsUnreviewed: false,
  allowed: false,
  uvMissing: false,
  uvBytes: 15236317,
  resourceBytes: 0,
  resourceHosts: [],
  catalog: null,
})
// Under an open dialog too, which hides the rest from a screen reader.
const tile = () => screen.getByRole("link", { name: "Plugins", hidden: true })
afterEach(cleanup)

describe("the rail", () => {
  it("the Plugins tile's menu installs a plugin, and loads a folder only while allowed", async () => {
    const failed = vi.spyOn(toast, "error")
    show()
    const menu = await tileMenu()
    // The tile's own list only: no app entries under it.
    expect(names(menu)).toEqual(["Install from a file…"])
    vi.mocked(pickPluginFile).mockResolvedValueOnce("D:\\Plugins\\one.asp")
    vi.mocked(commands.pluginInspect).mockRejectedValueOnce({ code: "invalid", message: "Not a plugin." })
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Install from a file…" }))
    await waitFor(() => expect(commands.pluginInspect).toHaveBeenCalledExactlyOnceWith("D:\\Plugins\\one.asp"))
    await waitFor(() => expect(failed).toHaveBeenCalledWith("Couldn't open one.asp", { description: "Not a plugin." }))
    cleanup()
    queryClient.clear()

    state.allow = true
    show()
    await waitFor(() => expect(commands.settingsGet).toHaveBeenCalled())
    await tileMenu()
    await waitFor(() => expect(names(screen.getByRole("menu"))).toEqual(["Install from a file…", "Load a folder…"]))
    vi.mocked(pickFolder).mockResolvedValueOnce("D:\\Plugins\\mine")
    vi.mocked(commands.pluginInspectFolder).mockRejectedValueOnce({ code: "invalid", message: "No plugin.json." })
    fireEvent.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: "Load a folder…" }))
    await waitFor(() => expect(commands.pluginInspectFolder).toHaveBeenCalledExactlyOnceWith("D:\\Plugins\\mine"))
  })

  it("the Plugins tile spins while it reads a plugin, waits, then shows the review", async () => {
    state.allow = true
    let read: (r: PluginReview) => void = () => {}
    vi.mocked(pickPluginFile).mockResolvedValueOnce("D:\\Plugins\\one.asp")
    vi.mocked(commands.pluginInspect).mockReturnValueOnce(new Promise((resolve) => (read = resolve)))
    show()
    await waitFor(() => expect(commands.settingsGet).toHaveBeenCalled())
    await tileMenu()
    await waitFor(() => expect(names(screen.getByRole("menu"))).toEqual(["Install from a file…", "Load a folder…"]))
    fireEvent.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: "Install from a file…" }))
    await waitFor(() => expect(commands.pluginInspect).toHaveBeenCalledExactlyOnceWith("D:\\Plugins\\one.asp"))
    // It shows that it is at work, and a second pick can't stack a second review on the first.
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    await waitFor(() => expect(tile().querySelector(".animate-spin")).not.toBeNull())
    const menu = await tileMenu()
    const entries = within(menu).getAllByRole("menuitem")
    expect(entries).toHaveLength(2)
    for (const entry of entries) expect(entry.getAttribute("aria-disabled")).toBe("true")
    fireEvent.keyDown(menu, { key: "Escape" })
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())

    act(() => read(review()))
    expect(await screen.findByRole("dialog", { name: "Install One?" })).toBeTruthy()
    expect(tile().querySelector(".animate-spin")).toBeNull()
  })

  it("the Plugins tile spins while a package dropped or opened from outside is read", async () => {
    show()
    await waitFor(() => expect(tile().querySelector(".animate-spin")).toBeNull())
    act(() => usePluginReview.setState({ loading: true }))
    await waitFor(() => expect(tile().querySelector(".animate-spin")).not.toBeNull())
    act(() => usePluginReview.setState({ loading: false }))
    await waitFor(() => expect(tile().querySelector(".animate-spin")).toBeNull())
  })

  it("opens one picker at a time", async () => {
    let picked: (path: string | null) => void = () => {}
    vi.mocked(pickPluginFile).mockReturnValueOnce(new Promise((resolve) => (picked = resolve)))
    show()
    fireEvent.click(within(await tileMenu()).getByRole("menuitem", { name: "Install from a file…" }))
    await waitFor(() => expect(pickPluginFile).toHaveBeenCalledOnce())
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    // Asked again while the first picker is open: nothing more opens.
    fireEvent.click(within(await tileMenu()).getByRole("menuitem", { name: "Install from a file…" }))
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
    expect(pickPluginFile).toHaveBeenCalledOnce()
    // Closed without a pick: the next ask opens one.
    await act(async () => picked(null))
    vi.mocked(pickPluginFile).mockResolvedValueOnce(null)
    fireEvent.click(within(await tileMenu()).getByRole("menuitem", { name: "Install from a file…" }))
    await waitFor(() => expect(pickPluginFile).toHaveBeenCalledTimes(2))
    expect(commands.pluginInspect).not.toHaveBeenCalled()
  })

  it("keeps the review when the rail is hidden while a plugin is read", async () => {
    let read: (r: PluginReview) => void = () => {}
    vi.mocked(pickPluginFile).mockResolvedValueOnce("D:\\Plugins\\one.asp")
    vi.mocked(commands.pluginInspect).mockReturnValueOnce(new Promise((resolve) => (read = resolve)))
    show()
    fireEvent.click(within(await tileMenu()).getByRole("menuitem", { name: "Install from a file…" }))
    await waitFor(() => expect(commands.pluginInspect).toHaveBeenCalledOnce())
    act(() => useUi.setState({ sidebarCollapsed: true }))
    expect(screen.queryByRole("link", { name: "Plugins" })).toBeNull()
    act(() => read(review()))
    expect(await screen.findByRole("dialog", { name: "Install One?" })).toBeTruthy()
  })
})
