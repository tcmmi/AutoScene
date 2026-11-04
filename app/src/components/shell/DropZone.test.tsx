import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from "@tanstack/react-router"
import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import { useNewProject } from "@/state/new-project"
import { usePluginReview } from "@/state/plugin-review"
import { pluginInfo } from "@/test/plugins"
import { DropZone } from "./DropZone"

type Drag = { type: "enter" | "over" | "drop" | "leave"; paths: string[] }

const { dropped, mediaAdd, pluginInspect } = vi.hoisted(() => ({
  dropped: { handler: null as null | ((e: { payload: Drag }) => void) },
  mediaAdd: vi.fn(async () => 2),
  pluginInspect: vi.fn(),
}))

vi.mock("@/lib/platform", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/platform")>()), isTauri: true }))
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (e: { payload: Drag }) => void) => {
      dropped.handler = handler
      return Promise.resolve(() => {})
    },
  }),
}))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return { ...ipc, commands: { ...ipc.commands, mediaAdd, pluginInspect } }
})
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn(), loading: vi.fn(() => "tid"), dismiss: vi.fn() }) }))

/** The window at `path`, with its drop zone. */
async function at(path: string) {
  const root = createRootRoute({ component: DropZone })
  const router = createRouter({ routeTree: root, history: createMemoryHistory({ initialEntries: [path] }) })
  render(<RouterProvider router={router} />)
  await act(() => router.load())
}

const files = [String.raw`D:\Video\Show S01E04.mkv`]
const drag = (type: Drag["type"], paths = files) => act(() => dropped.handler!({ payload: { type, paths } }))

beforeEach(() => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  useNewProject.setState({ open: false, seed: [], dragging: false })
  usePluginReview.setState({ review: null, loading: false, queue: [] })
  pluginInspect.mockImplementation(async () => ({
    ticketId: "t1",
    plugin: pluginInfo({ id: "dev.a.notify", name: "Notify" }),
    update: null,
    needsUnreviewed: false,
    allowed: true,
    uvMissing: false,
  }))
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  dropped.handler = null
})

describe("dropping files on the window", () => {
  it("a drop on a plugin page adds nothing", async () => {
    for (const path of ["/x/dev.a.page/main", "/p/p1/x/dev.a.page/clips/3"]) {
      await at(path)
      drag("enter")
      // Nothing says it would take them…
      expect(screen.queryByText("Add to this project")).toBeNull()
      expect(screen.queryByText("Create a project")).toBeNull()
      drag("drop")
      // …and nothing takes them: no project, no new one.
      expect(mediaAdd).not.toHaveBeenCalled()
      expect(useNewProject.getState().open).toBe(false)
      cleanup()
    }

    // Elsewhere in a project a drop adds to it, as always.
    await at("/p/p1/cast")
    drag("enter")
    expect(screen.getByText("Add to this project")).toBeTruthy()
    drag("drop")
    expect(mediaAdd).toHaveBeenCalledWith("p1", files)
  })

  it("a dropped package opens its review, not a new project", async () => {
    const pkg = [String.raw`D:\Downloads\Notify.ASP`]
    for (const path of ["/", "/p/p1/cast"]) {
      usePluginReview.setState({ review: null })
      pluginInspect.mockClear()
      await at(path)
      // The window says what the drop will do.
      drag("enter", pkg)
      expect(screen.getByText("Review a plugin")).toBeTruthy()
      expect(screen.queryByText("Create a project")).toBeNull()
      expect(screen.queryByText("Add to this project")).toBeNull()
      drag("drop", pkg)
      await waitFor(() => expect(usePluginReview.getState().review?.ticketId).toBe("t1"))
      // Read by its path, in any case of its ending; no project, no new one.
      expect(pluginInspect).toHaveBeenCalledExactlyOnceWith(pkg[0])
      expect(mediaAdd).not.toHaveBeenCalled()
      expect(useNewProject.getState().open).toBe(false)
      cleanup()
    }

    // Several: reviewed one after the other, and the window says so.
    usePluginReview.setState({ review: null })
    pluginInspect.mockClear()
    vi.mocked(toast.loading).mockClear()
    await at("/")
    drag("drop", [String.raw`D:\a.asp`, String.raw`D:\b.asp`])
    await waitFor(() => expect(usePluginReview.getState().review).not.toBeNull())
    expect(toast.loading).toHaveBeenCalledExactlyOnceWith("Reading a.asp\u2026 1 more wait their turn")
    expect(pluginInspect).toHaveBeenCalledExactlyOnceWith(String.raw`D:\a.asp`)
    act(() => usePluginReview.getState().close({ installed: true }))
    await waitFor(() => expect(pluginInspect).toHaveBeenCalledTimes(2))
    expect(pluginInspect).toHaveBeenLastCalledWith(String.raw`D:\b.asp`)
    expect(toast.dismiss).toHaveBeenCalledWith("tid")
  })

  it("a package says it is being read", async () => {
    await at("/")
    let read: (r: unknown) => void = () => {}
    pluginInspect.mockImplementationOnce(() => new Promise((resolve) => (read = resolve)))
    drag("drop", [String.raw`D:\Notify.asp`])
    expect(toast.loading).toHaveBeenCalledExactlyOnceWith("Reading Notify.asp\u2026")
    expect(toast.dismiss).not.toHaveBeenCalled()
    read({ ticketId: "t1", plugin: pluginInfo({ id: "dev.a.notify", name: "Notify" }), update: null, needsUnreviewed: false, allowed: true, uvMissing: false })
    await waitFor(() => expect(toast.dismiss).toHaveBeenCalledWith("tid"))
  })

  it("while a review is open nothing else comes up over it", async () => {
    await at("/p/p1/cast")
    await act(() => usePluginReview.getState().openReview(() => pluginInspect(), "a.asp"))
    expect(usePluginReview.getState().review).not.toBeNull()
    pluginInspect.mockClear()

    // No overlay for files or packages, no New Project, no files added.
    drag("enter")
    expect(screen.queryByText("Add to this project")).toBeNull()
    expect(useNewProject.getState().dragging).toBe(false)
    drag("drop")
    expect(mediaAdd).not.toHaveBeenCalled()
    expect(useNewProject.getState().open).toBe(false)
    expect(toast).toHaveBeenLastCalledWith("Finish the plugin review first.")

    // A package waits its turn, and says so.
    drag("enter", [String.raw`D:\b.asp`])
    expect(screen.queryByText("Review a plugin")).toBeNull()
    drag("drop", [String.raw`D:\b.asp`])
    expect(toast).toHaveBeenLastCalledWith("b.asp waits for the review open now")
    expect(pluginInspect).not.toHaveBeenCalled()
    expect(usePluginReview.getState().queue).toHaveLength(1)
  })

  it("a package dropped on the New Project dialog is not a source", async () => {
    await at("/")
    useNewProject.setState({ open: true, seed: [] })
    drag("drop", [String.raw`D:\Notify.asp`, ...files])
    expect(useNewProject.getState().seed).toEqual(files)
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin packages are no project files. Close this window and drop them on their own.")
    expect(pluginInspect).not.toHaveBeenCalled()
  })

  it("packages dropped with other files are not taken", async () => {
    await at("/p/p1/cast")
    const mixed = [String.raw`D:\Downloads\Notify.asp`, ...files]
    drag("enter", mixed)
    // Nothing takes them together, and the window says so before the drop.
    expect(screen.getByText("Drop plugin packages on their own")).toBeTruthy()
    expect(screen.queryByText("Add to this project")).toBeNull()
    drag("drop", mixed)
    expect(toast).toHaveBeenCalledExactlyOnceWith("Drop plugin packages on their own.")
    expect(pluginInspect).not.toHaveBeenCalled()
    expect(mediaAdd).not.toHaveBeenCalled()
    expect(useNewProject.getState().open).toBe(false)
    expect(usePluginReview.getState().review).toBeNull()
  })

  it("a package dropped on a plugin page is not reviewed either", async () => {
    await at("/x/dev.a.page/main")
    const pkg = [String.raw`D:\Downloads\Notify.asp`]
    drag("enter", pkg)
    expect(screen.queryByText("Review a plugin")).toBeNull()
    drag("drop", pkg)
    expect(pluginInspect).not.toHaveBeenCalled()
    expect(usePluginReview.getState().review).toBeNull()
    expect(usePluginReview.getState().loading).toBe(false)
  })
})
