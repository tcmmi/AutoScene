import type * as React from "react"
import {
  Outlet,
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { PageMount, PageReply, PageSpec, PluginInfo } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { NotFound } from "@/features/not-found"
import { followPlaces } from "@/lib/places"
import { pluginInfo } from "@/test/plugins"
import { PluginPageView } from "./PluginPageView"

const { list, pluginPageOpen, pluginPageClose, pluginPageCall } = vi.hoisted(() => ({
  list: { plugins: [] as PluginInfo[], error: null as unknown },
  pluginPageOpen: vi.fn(),
  pluginPageClose: vi.fn(async () => null),
  pluginPageCall: vi.fn(),
}))

// Not found is a page that scrolls, which jsdom can't.
vi.mock("@/components/ui/page", () => ({
  Page: ({ topbar, children }: { topbar: React.ReactNode; children: React.ReactNode }) => (
    <main>
      {topbar}
      {children}
    </main>
  ),
}))
vi.mock("@/lib/queries", () => ({
  usePlugins: () => ({ data: list.plugins, isPending: false, error: list.error }),
  useProject: () => ({ data: { id: "p1", title: "Show", kind: "series" } }),
  useAppInfo: () => ({ data: { version: "1.0.3-BETA" } }),
}))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return {
    ...ipc,
    commands: { ...ipc.commands, pluginPageOpen, pluginPageClose, pluginPageCall },
    events: { ...ipc.events, pluginPageEvent: { listen: vi.fn(async () => () => {}) } },
  }
})

const SESSION = "0123456789abcdef0123456789abcdef"

const page = (id: string, o: Partial<PageSpec> = {}): PageSpec => ({
  id,
  title: "Main",
  description: null,
  entry: `ui/${id}.html`,
  scope: "global",
  rail: true,
  ...o,
})

const hello = (o: Partial<PluginInfo> = {}) =>
  pluginInfo({
    id: "dev.a.page",
    name: "Hello page",
    runtime: "none",
    protocol: 2,
    pages: [page("main"), page("other", { title: "Other" }), page("clips", { title: "Clips", scope: "project", rail: false })],
    ...o,
  })

function mounted(): PageMount {
  return {
    session: SESSION,
    src: `http://asp.localhost/${SESSION}/ui/main.html`,
    plugin: { id: "dev.a.page", name: "Hello page", version: "1.0.0", trust: "reviewed", source: "package", icon: null },
    page: page("main"),
    projectId: null,
    permissions: [],
    hosts: [],
    service: false,
    protocol: 2,
  }
}

/** The app's routes for plugin pages, at `path`. */
function show(path: string) {
  const root = createRootRoute({ component: Outlet })
  const library = createRoute({ getParentRoute: () => root, path: "/", component: () => <p>Library</p> })
  const settings = createRoute({ getParentRoute: () => root, path: "/plugins/$id", component: () => <p>Its settings</p> })
  const own = createRoute({ getParentRoute: () => root, path: "/x/$plugin/$page/$", component: PluginPageView })
  const project = createRoute({ getParentRoute: () => root, path: "/p/$pid", component: Outlet })
  const projectPage = createRoute({ getParentRoute: () => project, path: "/x/$plugin/$page/$", component: PluginPageView })
  const router = createRouter({
    routeTree: root.addChildren([library, settings, own, project.addChildren([projectPage])]),
    history: createMemoryHistory({ initialEntries: [path] }),
    defaultNotFoundComponent: NotFound,
  })
  followPlaces(router.history)
  render(
    <TooltipProvider>
      <RouterProvider router={router} />
    </TooltipProvider>,
  )
  return router
}

/**
 * The page in `iframe` connects: its window says ready, and the host hands it a port. What it hears, but pings.
 *
 * A real page says ready on its frame's load, long after the host listens.
 * Here the frame never loads, and the host's listener comes in an effect
 * React may run after the frame is in the DOM: when its render took more
 * than one slice, the scheduler leaves the effect for later, and `findBy`
 * can see the frame before it. So the page says ready until the host hears
 * it; one nobody listened to did nothing.
 */
async function connect(iframe: HTMLIFrameElement) {
  let port: MessagePort | null = null
  vi.spyOn(iframe.contentWindow!, "postMessage").mockImplementation((_m: unknown, _o: unknown, transfer?: unknown) => {
    port = (transfer as MessagePort[])[0]
  })
  await waitFor(() => {
    const ready = new MessageEvent("message", { data: { type: "autoscene:ready", v: 1 } })
    Object.defineProperty(ready, "source", { value: iframe.contentWindow })
    window.dispatchEvent(ready)
    expect(port).not.toBeNull()
  })
  const heard: unknown[] = []
  port!.addEventListener("message", (e: MessageEvent<{ t: string }>) => void (e.data.t !== "ping" && heard.push(e.data)))
  port!.start()
  return { port: port! as MessagePort, heard }
}

beforeEach(() => {
  // The router puts each place at its top, which jsdom can't scroll.
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  list.plugins = [hello()]
  list.error = null
  pluginPageOpen.mockResolvedValue(mounted())
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe("a plugin's page", () => {
  it("a page of a plugin that is off shows why", async () => {
    list.plugins = [hello({ enabled: false, active: false })]
    show("/x/dev.a.page/main")
    expect(await screen.findByText("This page isn't available")).toBeTruthy()
    expect(screen.getByText("Hello page is turned off.")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Go to Plugins" })).toBeTruthy()
    cleanup()

    // Why it can't run, when that is something else.
    const broken = "Its files are missing. Install it again."
    list.plugins = [hello({ active: false, broken })]
    show("/x/dev.a.page/main")
    expect(await screen.findByText(broken)).toBeTruthy()
    expect(screen.queryByLabelText("Main")).toBeNull()
    expect(pluginPageOpen).not.toHaveBeenCalled()
  })

  it("a project page outside a project isn't found", async () => {
    show("/x/dev.a.page/clips")
    expect(await screen.findByText("Nothing here")).toBeTruthy()
    cleanup()
    // Nor a page of its own inside one, nor a link that names no plugin.
    for (const path of ["/p/p1/x/dev.a.page/main", "/x/Not.A-Plugin/main", "/x/dev.a.page/nope"]) {
      show(path)
      expect(await screen.findByText("Nothing here"), path).toBeTruthy()
      cleanup()
    }
    expect(pluginPageOpen).not.toHaveBeenCalled()

    // In a project it opens there, with the project's bar.
    show("/p/p1/x/dev.a.page/clips")
    await waitFor(() => expect(pluginPageOpen).toHaveBeenCalledWith("dev.a.page", "clips", "p1"))
    expect(screen.getByRole("navigation", { name: "Project" })).toBeTruthy()
  })

  // A plugin's settings are its own screen under Plugins, no longer a section of Settings.
  it("Its settings opens the plugin under Plugins", async () => {
    pluginPageOpen.mockReturnValue(new Promise(() => {}))
    const router = show("/x/dev.a.page/main")
    fireEvent.pointerDown(await screen.findByRole("button", { name: "Main options" }), { button: 0, ctrlKey: false })
    fireEvent.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Its settings" }))
    await waitFor(() => expect(router.state.location.pathname).toBe("/plugins/dev.a.page"))
    expect(await screen.findByText("Its settings")).toBeTruthy()
  })

  it("a page titled Settings still shows the Plugin badge", async () => {
    list.plugins = [hello({ pages: [page("main", { title: "Settings" })] })]
    show("/x/dev.a.page/main")
    const title = await screen.findByRole("heading", { level: 1 })
    expect(within(title).getByText("Settings")).toBeTruthy()
    expect(within(title).getByText("Plugin")).toBeTruthy()
  })

  it("back and forward reach the page as route messages", async () => {
    const router = show("/x/dev.a.page/main")
    const iframe = (await screen.findByLabelText("Main")) as HTMLIFrameElement
    const { port, heard } = await connect(iframe)
    const route = (path: string) => ({ t: "route", path })

    await act(() => router.history.push("/x/dev.a.page/main/clips/3"))
    await waitFor(() => expect(heard).toEqual([route("clips/3")]))
    await act(() => router.history.back())
    await waitFor(() => expect(heard).toEqual([route("clips/3"), route("")]))
    await act(() => router.history.forward())
    await waitFor(() => expect(heard).toEqual([route("clips/3"), route(""), route("clips/3")]))

    // The page's own move isn't told back to it.
    pluginPageCall.mockResolvedValueOnce({
      kind: "effect",
      effect: { type: "navigate", path: "clips/4", replace: false },
      result: null,
    } satisfies PageReply)
    heard.length = 0
    port.postMessage({ t: "call", id: 1, method: "ui.navigate", params: { path: "clips/4" } })
    await waitFor(() => expect(router.state.location.pathname).toBe("/x/dev.a.page/main/clips/4"))
    await waitFor(() => expect(heard).toEqual([{ t: "reply", id: 1, ok: true, result: null }]))
    expect(pluginPageCall).toHaveBeenCalledWith(SESSION, "ui.navigate", JSON.stringify({ path: "clips/4" }))

    // All along, one frame of one session.
    expect(screen.getByLabelText("Main")).toBe(iframe)
    expect(pluginPageOpen).toHaveBeenCalledTimes(1)
    port.close()
  })

  it("a page's move keeps its path as the page said it, inside the page", async () => {
    const router = show("/x/dev.a.page/main")
    const iframe = (await screen.findByLabelText("Main")) as HTMLIFrameElement
    const { port, heard } = await connect(iframe)
    // What Rust lets through, letters the router would read as its own among them.
    const paths = ["%2e%2e/%2e%2e/%2e%2e/settings", "..%2F..%2F..%2Fsettings", "a%20b/%25", "x:y@z~!&'()*+,;=-._", "clips/4", ""]
    for (const [id, path] of paths.entries()) {
      pluginPageCall.mockResolvedValueOnce({ kind: "effect", effect: { type: "navigate", path, replace: false }, result: null })
      heard.length = 0
      port.postMessage({ t: "call", id, method: "ui.navigate", params: { path } })
      await waitFor(() => expect(heard).toEqual([{ t: "reply", id, ok: true, result: null }]))
      const here = router.state.matches.at(-1)!
      expect(here.routeId, path).toBe("/x/$plugin/$page/$")
      expect(here.params, path).toMatchObject({ plugin: "dev.a.page", page: "main", _splat: path })
    }
    // Back walks them in turn, each told to the page as it was.
    heard.length = 0
    await act(() => router.history.back())
    await waitFor(() => expect(heard).toEqual([{ t: "route", path: "clips/4" }]))
    expect(screen.getByLabelText("Main")).toBe(iframe)
    expect(pluginPageOpen).toHaveBeenCalledTimes(1)
    port.close()
  })

  it("close after a move from one page to another goes back to the first", async () => {
    pluginPageOpen.mockRejectedValue({ code: "not_ready", message: "Hello page is being updated." })
    const router = show("/")
    await screen.findByText("Library")
    await act(() => router.history.push("/x/dev.a.page/main"))
    await act(() => router.history.push("/x/dev.a.page/other"))
    await screen.findByText("Hello page is being updated.")
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    await waitFor(() => expect(router.state.location.pathname).toBe("/x/dev.a.page/main"))
  })

  it("close on the second of two pages, then on the first, ends where the first was opened from", async () => {
    pluginPageOpen.mockRejectedValue({ code: "not_ready", message: "Hello page is being updated." })
    const router = show("/")
    await screen.findByText("Library")
    await act(() => router.history.push("/x/dev.a.page/main"))
    await act(() => router.history.push("/x/dev.a.page/other"))
    await screen.findByText("Hello page is being updated.")
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    await waitFor(() => expect(router.state.location.pathname).toBe("/x/dev.a.page/main"))
    // The first page again, opened anew: its Close never goes back to the second.
    await waitFor(() => expect(pluginPageOpen).toHaveBeenCalledTimes(3))
    await screen.findByText("Hello page is being updated.")
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    await waitFor(() => expect(router.state.location.pathname).toBe("/"))
    await screen.findByText("Library")
  })

  it("close leaves for where the page was opened from, however the person moved in it", async () => {
    pluginPageOpen.mockRejectedValue({ code: "not_ready", message: "Hello page is being updated." })
    const router = show("/")
    await screen.findByText("Library")
    await act(() => router.history.push("/x/dev.a.page/main"))
    await act(() => router.history.push("/x/dev.a.page/main/clips"))
    await act(() => router.history.push("/"))
    // Back into the page's deeper entry, then a step back inside it.
    await act(() => router.history.back())
    await screen.findByText("Hello page is being updated.")
    await act(() => router.history.back())
    await waitFor(() => expect(router.state.location.pathname).toBe("/x/dev.a.page/main"))
    fireEvent.click(screen.getByRole("button", { name: "Close" }))
    // Where it was opened from (the library it came back from), never a step through the history: no reload.
    await waitFor(() => expect(router.state.location.pathname).toBe("/"))
    await screen.findByText("Library")
  })

  it("a page whose plugins couldn't be read says why", async () => {
    list.plugins = undefined as unknown as PluginInfo[]
    list.error = { code: "internal", message: "Plugins couldn't start." }
    show("/x/dev.a.page/main")
    expect(await screen.findByText("Couldn't read the plugins")).toBeTruthy()
    expect(screen.getByText("Plugins couldn't start.")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Go to Plugins" })).toBeTruthy()
  })
})
