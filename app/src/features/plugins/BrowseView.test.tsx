import { QueryClientProvider } from "@tanstack/react-query"
import { Outlet, RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import type * as React from "react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, CatalogView, PluginInfo } from "@/bindings"
import { ContextArea } from "@/components/ui/context-area"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { queryClient } from "@/lib/queries"
import { usePluginReview } from "@/state/plugin-review"
import { catalogPlugin, catalogReview, catalogView, pluginInfo } from "@/test/plugins"
import { useCatalogCheck } from "./catalog-actions"
import { InstallReviewDialog } from "./InstallReviewDialog"
import { PluginsPage } from "./PluginsPage"

const { state, toast, checkUpdates } = vi.hoisted(() => ({
  state: { plugins: [] as PluginInfo[], catalog: null as CatalogView | null, daily: true },
  toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }),
  checkUpdates: vi.fn(),
}))

const settings = (): AppSettings => ({
  libraryRoot: "C:\\Library",
  exportPresetId: "prores-hq",
  audioLanguage: "eng",
  allowUnreviewedPlugins: false,
  pluginCatalogChecks: state.daily,
})

vi.mock("sonner", () => ({ toast }))
vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    settingsGet: vi.fn(() => Promise.resolve(settings())),
    settingsUpdate: vi.fn((s: AppSettings) => Promise.resolve(s)),
    pluginsList: vi.fn(() => Promise.resolve(state.plugins)),
    pluginBanner: vi.fn(() => Promise.resolve(null)),
    pluginInstall: vi.fn(),
    pluginDiscard: vi.fn(() => Promise.resolve(null)),
    pluginOpenSource: vi.fn(() => Promise.resolve(true)),
    catalogList: vi.fn(() => Promise.resolve(state.catalog)),
    catalogRefresh: vi.fn(() => Promise.resolve(state.catalog)),
    catalogGet: vi.fn(),
    catalogCancel: vi.fn(() => Promise.resolve(null)),
  },
}))
vi.mock("@/lib/actions", () => ({
  useAppActions: () => ({ goPlugin: vi.fn(), goPlugins: vi.fn(), goSettings: vi.fn(), checkUpdates }),
}))
vi.mock("@/components/ui/page", () => ({
  Page: ({ children, menu }: { children: React.ReactNode; menu?: React.ReactNode }) => (
    <ContextArea kind="page" menu={menu ?? null}>
      <main>{children}</main>
    </ContextArea>
  ),
}))

const ICON = "data:image/svg+xml;base64,PHN2Zy8+"

/** Plugins at `/plugins?view=browse`, with the window's one install review. */
function show() {
  const root = createRootRoute({
    component: () => (
      <>
        <PluginsPage />
        <InstallReviewDialog />
        <Outlet />
      </>
    ),
  })
  const plugins = createRoute({ getParentRoute: () => root, path: "/plugins", component: () => null })
  const plugin = createRoute({ getParentRoute: () => root, path: "/plugins/$id", component: () => null })
  const router = createRouter({
    routeTree: root.addChildren([plugins, plugin]),
    history: createMemoryHistory({ initialEntries: ["/plugins?view=browse"] }),
  })
  render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )
  return router
}

const card = (name: string) => screen.findByRole("group", { name })
const names = () => screen.queryAllByRole("group").map((g) => g.getAttribute("aria-label"))

const poster = catalogPlugin({
  id: "dev.autoscene.poster-maker",
  name: "Poster Maker",
  summary: "Makes a poster of each character from their best frames.",
  permissions: ["project:read", "exports:write"],
  gpu: true,
  icon: ICON,
})
const notify = catalogPlugin({
  id: "dev.autoscene.discord-notify",
  name: "Discord Notify",
  summary: "Posts to a Discord channel when an export is done.",
  author: "Example Dev",
  permissions: ["exports:read", "network"],
  hosts: ["discord.com"],
  runtime: "wasm",
})

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  vi.mocked(commands.catalogGet).mockReset()
  vi.mocked(commands.pluginInstall).mockReset()
  usePluginReview.setState({ review: null, loading: false, queue: [], getting: null })
  useCatalogCheck.setState({ checking: false })
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  state.plugins = []
  state.catalog = catalogView([poster, notify])
  state.daily = true
})
afterEach(cleanup)

describe("Browse", () => {
  it("lists the catalog and finds a plugin by name", async () => {
    const user = userEvent.setup()
    show()
    expect(await card("Poster Maker")).toBeTruthy()
    expect(names()).toEqual(["Poster Maker", "Discord Notify"])
    // AutoScene's caption: its review, who it is by, what it does and may do.
    const p = within(await card("Poster Maker"))
    expect(p.getByText("Reviewed by AutoScene")).toBeTruthy()
    expect(p.getByText("By AutoScene · version 1.0.0")).toBeTruthy()
    expect(p.getByText("Makes a poster of each character from their best frames.")).toBeTruthy()
    expect(p.getByText("Reads the project: its episodes and characters · Changes or adds files beside the exported clips · Uses the graphics card")).toBeTruthy()
    // The network as the hosts it talks to.
    expect(within(await card("Discord Notify")).getByText("Reads the exported clips · Talks to discord.com")).toBeTruthy()
    // Which catalog, and when it was checked.
    expect(screen.getByText(/^Catalog of 3 November 2026\. Checked /)).toBeTruthy()

    // Browse has its search, though the catalog is short.
    const find = screen.getByRole("textbox", { name: "Find a plugin" })
    await user.type(find, "discord")
    expect(names()).toEqual(["Discord Notify"])
    // By its author too.
    await user.clear(find)
    await user.type(find, "example dev")
    expect(names()).toEqual(["Discord Notify"])
    await user.clear(find)
    await user.type(find, "nothing like it")
    expect(screen.getByText("Nothing matches")).toBeTruthy()
    fireEvent.click(screen.getByText("Clear the search", { selector: "button" }))
    expect(names()).toEqual(["Poster Maker", "Discord Notify"])
  })

  it("marks the catalog plugins that run in a sandbox, and no others", async () => {
    show()
    expect(within(await card("Discord Notify")).getByText("Sandboxed")).toBeTruthy()
    expect(within(await card("Poster Maker")).queryByText("Sandboxed")).toBeNull()
  })

  it.each(["", "none", "service", "WASM", "wasi", "process"])("does not mark a catalog plugin whose runtime is %j", async (runtime) => {
    state.catalog = catalogView([catalogPlugin({ ...poster, runtime })])
    show()
    expect(await card("Poster Maker")).toBeTruthy()
    expect(screen.queryByText("Sandboxed")).toBeNull()
  })

  it("installs from the catalog through the review", async () => {
    let got: (r: ReturnType<typeof catalogReview>) => void = () => {}
    vi.mocked(commands.catalogGet).mockReturnValue(new Promise((done) => (got = done)))
    const installed = pluginInfo({ id: poster.id, name: poster.name, author: "AutoScene", icon: ICON })
    vi.mocked(commands.pluginInstall).mockResolvedValue(installed)
    show()
    const button = within(await card("Poster Maker")).getByRole("button", { name: "Install…" }) as HTMLButtonElement
    fireEvent.click(button)
    expect(commands.catalogGet).toHaveBeenCalledWith("dev.autoscene.poster-maker")
    // While it comes, the dialog says so, and the button waits.
    expect(await screen.findByRole("dialog", { name: "Getting Poster Maker…" })).toBeTruthy()
    expect(button.disabled).toBe(true)

    got(catalogReview(installed))
    const dialog = await screen.findByRole("dialog", { name: "Install Poster Maker?" })
    expect(within(dialog).getByText(/^Built from/).textContent).toBe(
      "Built from github.com/tcmmi/AutoScene at 4f1c2a9, in examples/plugins/poster-maker. Catalog of 3 November 2026.",
    )
    // AutoScene reviewed it: nothing to type.
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }))
    await waitFor(() => expect(commands.pluginInstall).toHaveBeenCalledWith("t-dev.autoscene.poster-maker", false))
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  })

  it("shows Update… for a newer version", async () => {
    state.plugins = [pluginInfo({ id: notify.id, name: notify.name }), pluginInfo({ id: poster.id, name: poster.name })]
    state.catalog = catalogView([
      { ...poster, installed: "1.0.0" },
      { ...notify, version: "1.0.1", installed: "1.0.0", update: true },
    ])
    show()
    const n = within(await card("Discord Notify"))
    expect(n.getByRole("button", { name: "Update…" })).toBeTruthy()
    expect(n.queryByRole("button", { name: "Install…" })).toBeNull()
    // The installed version, nothing to get: it says so, and opens its settings.
    const p = within(await card("Poster Maker"))
    expect(p.getByText("Installed")).toBeTruthy()
    expect(p.queryByRole("button", { name: /Install…|Update…/ })).toBeNull()
    expect(p.getByRole("link", { name: "Poster Maker settings" }).getAttribute("href")).toBe("/plugins/dev.autoscene.poster-maker")
  })

  it("says when the catalog is out of date", async () => {
    state.catalog = catalogView([], { outOfDate: true, fetchedAt: Date.UTC(2026, 8, 24, 10), error: "The catalog ran out on 2026-10-24." })
    show()
    const banner = await screen.findByRole("status")
    expect(within(banner).getByText("The plugin catalog is out of date")).toBeTruthy()
    expect(within(banner).getByText(/AutoScene couldn't check it since 24 September 2026\./)).toBeTruthy()
    // Said once: the title's line doesn't call the failed try a check, the
    // catalog's row doesn't repeat it, and no search finds among no cards.
    expect(screen.getByText(/^Catalog of 3 November 2026\. Last check failed /)).toBeTruthy()
    expect(screen.queryByText("The catalog ran out on 2026-10-24.")).toBeNull()
    expect(screen.queryByRole("textbox", { name: "Find a plugin" })).toBeNull()
    expect(screen.queryByText("Nothing matches")).toBeNull()

    fireEvent.click(within(banner).getByRole("button", { name: "Try again" }))
    await waitFor(() => expect(commands.catalogRefresh).toHaveBeenCalledOnce())
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't check the plugin catalog", { description: "The catalog ran out on 2026-10-24." }))
  })

  it("offers nothing that needs a newer AutoScene", async () => {
    state.catalog = catalogView([
      { ...poster, needsNewerApp: true },
      { ...notify, runsHere: false },
    ])
    show()
    const p = within(await card("Poster Maker"))
    expect(p.getByText("Needs a newer AutoScene")).toBeTruthy()
    expect(p.queryByRole("button", { name: /Install/ })).toBeNull()
    const n = within(await card("Discord Notify"))
    expect(n.getByText("Not for this computer")).toBeTruthy()
    expect(n.queryByRole("button", { name: /Install/ })).toBeNull()
    cleanup()
    queryClient.clear()

    // A catalog this AutoScene is too old for shows no plugins at all.
    state.catalog = catalogView([], { ready: false, needsNewerApp: true })
    show()
    expect(await screen.findByText("Update AutoScene to use the plugin catalog")).toBeTruthy()
    // It says so once, below the title rather than in its line too.
    expect(screen.queryByText("This catalog needs a newer AutoScene.")).toBeNull()
    expect(screen.queryByRole("textbox", { name: "Find a plugin" })).toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }))
    expect(checkUpdates).toHaveBeenCalledOnce()
  })

  it("a card's menu offers no source until AutoScene can open the catalog's own link", async () => {
    show()
    fireEvent.pointerDown(await screen.findByRole("button", { name: "Discord Notify options" }), { button: 0, ctrlKey: false })
    const items = within(await screen.findByRole("menu")).getAllByRole("menuitem")
    expect(items.map((i) => i.textContent?.trim())).toEqual(["Install…", "Copy its id"])
  })

  it("checks the catalog now, and every day only while the switch is on", async () => {
    state.catalog = catalogView([{ ...notify, version: "1.0.1", installed: "1.0.0", update: true }])
    show()
    fireEvent.click(await screen.findByRole("button", { name: "Check now" }))
    await waitFor(() => expect(toast).toHaveBeenCalledWith("1 plugin update"))

    const daily = screen.getByRole("switch", { name: "Check the plugin catalog every day" })
    await waitFor(() => expect((daily as HTMLButtonElement).disabled).toBe(false))
    expect(daily.getAttribute("aria-checked")).toBe("true")
    fireEvent.click(daily)
    await waitFor(() => expect(commands.settingsUpdate).toHaveBeenCalledWith(expect.objectContaining({ pluginCatalogChecks: false })))
    expect(await screen.findByText("Off: AutoScene doesn't learn of withdrawn plugins.")).toBeTruthy()
  })

  it("Installed and Browse are one switch, and the address keeps which", async () => {
    const router = show()
    await card("Poster Maker")
    fireEvent.mouseDown(screen.getByRole("radio", { name: "Installed" }))
    fireEvent.click(screen.getByRole("radio", { name: "Installed" }))
    await waitFor(() => expect(router.state.location.search).toEqual({}))
    expect(await screen.findByText("No plugins yet")).toBeTruthy()
  })
})
