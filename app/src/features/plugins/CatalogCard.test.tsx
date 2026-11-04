import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { cleanup, render, screen } from "@testing-library/react"
import type * as React from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { CatalogPlugin, PluginInfo } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { queryClient } from "@/lib/queries"
import { usePluginReview } from "@/state/plugin-review"
import { catalogPlugin, pluginInfo } from "@/test/plugins"
import { CatalogCard } from "./CatalogCard"

const { state } = vi.hoisted(() => ({ state: { plugins: [] as PluginInfo[] } }))

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    settingsGet: vi.fn(() => Promise.resolve({ allowUnreviewedPlugins: false })),
    pluginsList: vi.fn(() => Promise.resolve(state.plugins)),
    catalogGet: vi.fn(),
  },
}))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goPlugin: vi.fn() }) }))

/** One catalog card, as Browse draws it. */
function show(p: CatalogPlugin) {
  const root = createRootRoute({ component: () => <CatalogCard p={p} /> })
  const plugin = createRoute({ getParentRoute: () => root, path: "/plugins/$id", component: () => null })
  const router = createRouter({ routeTree: root.addChildren([plugin]), history: createMemoryHistory({ initialEntries: ["/"] }) })
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>{children}</TooltipProvider>
    </QueryClientProvider>
  )
  render(<RouterProvider router={router} />, { wrapper })
}

const model = catalogPlugin({ id: "dev.example.big-model", name: "Big Model", resourceBytes: 327_155_712 })

beforeEach(() => {
  queryClient.clear()
  state.plugins = []
  usePluginReview.setState({ review: null, loading: false, queue: [], getting: null })
})
afterEach(cleanup)

describe("a catalog card", () => {
  it("a catalog plugin with large files says so", async () => {
    show(model)
    expect(await screen.findByText("Downloads 312 MB at its first run")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Install…" })).toBeTruthy()
    cleanup()

    // A service gets them as it first starts.
    show({ ...model, runtime: "service" })
    expect(await screen.findByText("Downloads 312 MB at its first start")).toBeTruthy()
  })

  it("a plugin that names none says nothing of downloads", async () => {
    show(catalogPlugin({ id: "dev.a.plain", name: "Plain" }))
    await screen.findByText("By AutoScene · version 1.0.0")
    expect(screen.queryByText(/Downloads/)).toBeNull()
    cleanup()
    show({ ...model, resourceBytes: 0 })
    await screen.findByText("By AutoScene · version 1.0.0")
    expect(screen.queryByText(/Downloads/)).toBeNull()
  })

  it("an update says so only when its files come from a host the installed version has none from", async () => {
    state.plugins = [pluginInfo({ id: model.id, name: model.name })]
    // From known hosts the files may be here already: the review states the size.
    show({ ...model, installed: "0.9.0", update: true })
    await screen.findByRole("button", { name: "Update…" })
    expect(screen.queryByText(/Downloads/)).toBeNull()
    cleanup()

    // From a new host it says that, not a size: the files it names may be here already.
    show({ ...model, installed: "0.9.0", update: true, addsResourceHosts: ["huggingface.co"] })
    expect(await screen.findByText("Downloads from a new host: huggingface.co")).toBeTruthy()
    expect(screen.queryByText(/312 MB/)).toBeNull()
    cleanup()

    // A plugin that needs a newer AutoScene cannot be installed: it advertises no download.
    show({ ...model, needsNewerApp: true })
    await screen.findByText("Needs a newer AutoScene")
    expect(screen.queryByText(/Downloads/)).toBeNull()
    cleanup()

    // Installed and up to date, or not for this computer: nothing is going to be downloaded.
    show({ ...model, installed: "1.0.0" })
    await screen.findByText("Installed")
    expect(screen.queryByText(/Downloads/)).toBeNull()
    cleanup()
    show({ ...model, runsHere: false })
    await screen.findByText("Not for this computer")
    expect(screen.queryByText(/Downloads/)).toBeNull()
  })

  it("a withdrawn plugin says nothing of downloads", async () => {
    state.plugins = [pluginInfo({ id: model.id, name: model.name, withdrawn: "It sent export paths to a server." })]
    show({ ...model, installed: "1.0.0", update: true, addsResourceHosts: ["huggingface.co"] })
    await screen.findByText("Withdrawn")
    expect(screen.queryByText(/Downloads/)).toBeNull()
  })
})
