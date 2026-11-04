import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { Outlet, RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter, useParams } from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, PluginInfo, PluginReview } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { usePluginReview } from "@/state/plugin-review"
import { pluginInfo } from "@/test/plugins"
import { InstallReviewDialog } from "./InstallReviewDialog"
import { PluginHero } from "./PluginSettingsPage"
import { PluginsPage } from "./PluginsPage"

const { state, reveal, openExternalUrl, goPlugin, copyText } = vi.hoisted(() => ({
  state: { plugins: [] as PluginInfo[], allow: false },
  reveal: vi.fn(),
  openExternalUrl: vi.fn(),
  goPlugin: vi.fn(),
  copyText: vi.fn(() => Promise.resolve(true)),
}))

const settings = (allow: boolean): AppSettings => ({
  libraryRoot: "C:\\Library",
  exportPresetId: "prores-hq",
  audioLanguage: "eng",
  allowUnreviewedPlugins: allow,
})

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    settingsGet: vi.fn(() => Promise.resolve(settings(state.allow))),
    pluginsList: vi.fn(() => Promise.resolve(state.plugins)),
    pluginsAllowUnreviewed: vi.fn((on: boolean) => Promise.resolve(settings(on))),
    pluginSetEnabled: vi.fn(),
    pluginUninstall: vi.fn(() => Promise.resolve(null)),
    pluginInspect: vi.fn(),
    pluginInspectFolder: vi.fn(),
    pluginDiscard: vi.fn(() => Promise.resolve(null)),
    pluginSettingsGet: vi.fn(() => Promise.resolve({})),
    pluginBanner: vi.fn(() => Promise.resolve(null)),
    pluginOpenSource: vi.fn(() => Promise.resolve(true)),
    revealPath: vi.fn(() => Promise.resolve()),
  },
}))
vi.mock("@/lib/media", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/media")>()),
  pickFolder: vi.fn(() => Promise.resolve("D:\\Plugins\\mine")),
  pickPluginFile: vi.fn(() => Promise.resolve("D:\\Downloads\\Notify.asp")),
}))
vi.mock("@/lib/os", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/os")>()), reveal, openExternalUrl, copyText }))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings: vi.fn(), goPlugin }) }))
// The window's menu, around the rows' own entries.
vi.mock("@/components/ui/page", () => ({ Page: ({ children }: { children: React.ReactNode }) => <main>{children}</main> }))

const BANNER = "data:image/jpeg;base64,/9j/4AAQ"
const ICON = "data:image/svg+xml;base64,PHN2Zy8+"

/** `ui` (Plugins, unless said otherwise) at `/plugins`, in the app's router: a plugin's screen says which it is. The shell's review dialog is there too. */
function show(ui: React.ReactNode = <PluginsPage />) {
  const root = createRootRoute({
    component: () => (
      <>
        {ui}
        <InstallReviewDialog />
        <Outlet />
      </>
    ),
  })
  const plugins = createRoute({ getParentRoute: () => root, path: "/plugins", component: () => null })
  const plugin = createRoute({
    getParentRoute: () => root,
    path: "/plugins/$id",
    component: function Screen() {
      const { id } = useParams({ strict: false })
      return <p>The screen of {id}</p>
    },
  })
  const router = createRouter({
    routeTree: root.addChildren([plugins, plugin]),
    history: createMemoryHistory({ initialEntries: ["/plugins"] }),
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )
  return { router, container: view.container, unmount: view.unmount }
}

const allowSwitch = () => screen.getByRole("switch", { name: "Allow plugins that AutoScene has not reviewed" })
const card = (name: string) => screen.findByRole("group", { name })
/** A card's art: the link to its plugin's screen. */
const art = async (name: string) => within(await card(name)).getByRole("link")
/** What a ⋯ menu lists. */
async function entries(name: string) {
  fireEvent.pointerDown(await screen.findByRole("button", { name: `${name} options` }), { button: 0, ctrlKey: false })
  const items = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" })
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
  return items
}
const six = () => [
  pluginInfo({ id: "dev.a.alpha", name: "Alpha Tool", author: "Alice", description: "Video helper" }),
  pluginInfo({ id: "dev.b.beta", name: "Beta Script", author: "Bob", description: "Audio cleaner" }),
  pluginInfo({ id: "dev.c.gamma", name: "Gamma", author: "Carol", description: "Writes notes" }),
  pluginInfo({ id: "dev.d.delta", name: "Delta", author: "Dan", description: "Posts to a channel" }),
  pluginInfo({ id: "dev.e.epsilon", name: "Epsilon", author: "Eve", description: "Checks frames" }),
  pluginInfo({ id: "dev.f.zeta", name: "Zeta", author: "Fay", description: "Counts words" }),
]
const names = () => screen.queryAllByRole("group").map((g) => g.getAttribute("aria-label"))

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  // A review a failed case left unasked must not answer the next one.
  vi.mocked(commands.pluginInspect).mockReset()
  vi.mocked(commands.pluginInspectFolder).mockReset()
  usePluginReview.setState({ review: null, loading: false, queue: [] })
  // The router puts each place at its top, which jsdom can't scroll.
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  state.plugins = []
  state.allow = false
})
afterEach(cleanup)

describe("PluginsPage", () => {
  it("lists plugins with their trust", async () => {
    state.plugins = [
      pluginInfo({ id: "dev.a.reviewed", name: "Reviewed One", description: "Says hello.", author: "AutoScene" }),
      pluginInfo({ id: "dev.a.pinned", name: "Pinned One", trust: "pinned", authorKeyId: "4B2D9C1E0F3A7E51" }),
      pluginInfo({
        id: "dev.a.unreviewed",
        name: "Unreviewed One",
        trust: "unreviewed",
        active: false,
        blockedReason: "Plugins that AutoScene has not reviewed are turned off. You can allow them in Plugins.",
      }),
      // A folder says so first, whatever its trust.
      pluginInfo({ id: "dev.a.folder", name: "Folder One", trust: "unreviewed", source: "folder", enabled: false, active: false }),
    ]
    show()
    const badges: [string, string][] = [
      ["Reviewed One", "Reviewed by AutoScene"],
      ["Pinned One", "Signed, not reviewed"],
      ["Unreviewed One", "Not reviewed"],
      ["Folder One", "Loaded from a folder"],
    ]
    for (const [name, badge] of badges) expect(within(await card(name)).getByText(badge)).toBeTruthy()
    // Visible words, not a hidden copy for screen readers.
    const facts = within(await card("Reviewed One")).getByText("By AutoScene · version 1.0.0")
    expect(facts.className).not.toMatch(/hidden|opacity-0|sr-only/)
    // Only AutoScene's review stands behind the name a plugin gives its author.
    expect(within(await card("Pinned One")).getByText("Says it is by Someone · version 1.0.0")).toBeTruthy()
    // Why one that is turned on doesn't run; nothing for one turned off.
    // In the card's own words: the backend's sends people to Plugins, where they are.
    expect(within(await card("Unreviewed One")).getByText("Doesn't run until the setting below allows it.")).toBeTruthy()
    expect(screen.queryByText(/You can allow them in Plugins/)).toBeNull()
    expect(within(await card("Folder One")).queryByText(/setting below/)).toBeNull()
    const folder = screen.getByRole("switch", { name: "Enable Folder One" })
    expect(folder.getAttribute("aria-checked")).toBe("false")
    expect(screen.getByRole("switch", { name: "Enable Reviewed One" }).getAttribute("aria-checked")).toBe("true")
    // What runs: the blocked one and the one turned off don't.
    expect(screen.getByText("4 plugins · 2 running")).toBeTruthy()

    // Turned on, the list shows what the backend gave back.
    vi.mocked(commands.pluginSetEnabled).mockResolvedValue({ ...state.plugins[3], enabled: true })
    fireEvent.click(folder)
    await waitFor(() => expect(screen.getByRole("switch", { name: "Enable Folder One" }).getAttribute("aria-checked")).toBe("true"))
    expect(commands.pluginSetEnabled).toHaveBeenCalledWith("dev.a.folder", true)
  })

  it("the trust badge and the facts sit under the art, not on it", async () => {
    let answer: (url: string | null) => void = () => {}
    vi.mocked(commands.pluginBanner).mockReturnValue(new Promise((done) => (answer = done)))
    state.plugins = [
      pluginInfo({ id: "dev.a.art", name: "Art", trust: "unreviewed", author: "someone", bannerHash: "a1", icon: ICON }),
      pluginInfo({ id: "dev.a.broken", name: "Broken", source: "folder", broken: "Its plugin.json can't be read." }),
    ]
    show()
    const link = await art("Art")
    const caption = [
      within(await card("Art")).getByText("Not reviewed"),
      within(await card("Art")).getByText("Says it is by someone · version 1.0.0"),
      screen.getByRole("switch", { name: "Enable Art" }),
    ]
    // Its icon shows until its banner is in hand, in the same place.
    expect(link.querySelector(`img[src="${ICON}"]`)).not.toBeNull()
    act(() => answer(BANNER))
    await waitFor(() => expect(link.querySelector(`img[src="${BANNER}"]`)).not.toBeNull())
    expect(await art("Art")).toBe(link)
    for (const el of caption) expect(link.contains(el)).toBe(false)
    // On the art only its name: nothing the host vouches for on the plugin's pixels.
    expect(link.textContent).toBe("Art")
    // Why one doesn't run is under it too.
    const why = within(await card("Broken")).getByText("Its plugin.json can't be read.")
    expect((await art("Broken")).contains(why)).toBe(false)
  })

  it("says so when no plugin is installed, with one Install a plugin…", async () => {
    show()
    expect(await screen.findByText("No plugins yet")).toBeTruthy()
    expect(screen.getAllByRole("button", { name: "Install a plugin…" })).toHaveLength(1)
    expect(screen.queryByRole("textbox", { name: "Find a plugin" })).toBeNull()
    // The setting it needs is still there, below.
    expect(allowSwitch()).toBeTruthy()
    cleanup()
    queryClient.clear()

    state.allow = true
    show()
    expect(await screen.findByText("No plugins yet")).toBeTruthy()
    await waitFor(() => expect(screen.getAllByRole("button", { name: "Load a folder…" })).toHaveLength(1))
    expect(screen.getAllByRole("button", { name: "Install a plugin…" })).toHaveLength(1)
  })

  it("finds a plugin by its name, author or description from six plugins", async () => {
    const user = userEvent.setup()
    state.plugins = six()
    show()
    const find = await screen.findByRole("textbox", { name: "Find a plugin" })
    expect(names()).toHaveLength(6)
    await user.type(find, "alpha")
    expect(names()).toEqual(["Alpha Tool"])
    await user.clear(find)
    await user.type(find, "BOB")
    expect(names()).toEqual(["Beta Script"])
    await user.clear(find)
    await user.type(find, "channel")
    expect(names()).toEqual(["Delta"])
    // Each word, wherever it is: an author and a word of what it does.
    await user.clear(find)
    await user.type(find, "carol notes")
    expect(names()).toEqual(["Gamma"])
    await user.clear(find)
    await user.type(find, "dev.f")
    expect(names()).toEqual(["Zeta"])
    // Escape clears it.
    await user.keyboard("{Escape}")
    expect((find as HTMLInputElement).value).toBe("")
    expect(names()).toHaveLength(6)
    // Nothing found says so, and clears the search.
    await user.type(find, "nothing like it")
    expect(names()).toEqual([])
    const nothing = screen.getByText("Nothing matches").parentElement!
    await user.click(within(nothing).getByRole("button", { name: "Clear the search" }))
    expect(names()).toHaveLength(6)
  })

  it("keeps the search while it narrows, though fewer than six are left", async () => {
    const user = userEvent.setup()
    state.plugins = six()
    show()
    const find = await screen.findByRole("textbox", { name: "Find a plugin" })
    await user.type(find, "a")
    // One is uninstalled meanwhile: five are left, and the search stays.
    state.plugins = six().slice(1)
    await act(() => queryClient.invalidateQueries({ queryKey: qk.plugins }))
    await waitFor(() => expect(names()).not.toContain("Alpha Tool"))
    expect(screen.getByRole("textbox", { name: "Find a plugin" })).toBe(find)
    expect((find as HTMLInputElement).value).toBe("a")
    // Cleared, it goes: there are fewer than six.
    await user.click(screen.getByRole("button", { name: "Clear the search" }))
    expect(screen.queryByRole("textbox", { name: "Find a plugin" })).toBeNull()
    expect(names()).toHaveLength(5)
  })

  it("shows no search under six plugins", async () => {
    state.plugins = six().slice(0, 5)
    show()
    expect(await card("Alpha Tool")).toBeTruthy()
    expect(names()).toHaveLength(5)
    expect(screen.queryByRole("textbox", { name: "Find a plugin" })).toBeNull()
    expect(screen.queryByRole("search")).toBeNull()
  })

  it("a card opens its plugin", async () => {
    const user = userEvent.setup()
    state.plugins = [
      pluginInfo({ id: "dev.a.alpha", name: "Alpha Tool" }),
      pluginInfo({ id: "dev.a.bare", name: "Bare" }),
    ]
    const { router } = show()
    await user.click(await art("Bare"))
    await waitFor(() => expect(router.state.location.pathname).toBe("/plugins/dev.a.bare"))
    expect(await screen.findByText("The screen of dev.a.bare")).toBeTruthy()
    // Its switch and its ⋯ are their own, not the card's.
    await act(() => router.navigate({ to: "/plugins" }))
    vi.mocked(commands.pluginSetEnabled).mockResolvedValueOnce({ ...state.plugins[0], enabled: false })
    await user.click(screen.getByRole("switch", { name: "Enable Alpha Tool" }))
    await waitFor(() => expect(commands.pluginSetEnabled).toHaveBeenCalledWith("dev.a.alpha", false))
    await user.click(screen.getByRole("button", { name: "Alpha Tool options" }))
    expect(await screen.findByRole("menu")).toBeTruthy()
    expect(router.state.location.pathname).toBe("/plugins")
    await user.click(screen.getByRole("menuitem", { name: "Its settings" }))
    expect(goPlugin).toHaveBeenCalledExactlyOnceWith("dev.a.alpha")
  })

  it("a plugin without a banner shows its icon as its artwork", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.icon", name: "Icon", icon: ICON })]
    show()
    const link = await art("Icon")
    const icons = [...link.querySelectorAll<HTMLImageElement>(`img[src="${ICON}"]`)]
    // Blurred into the backdrop, and sharp on glass beside the name.
    expect(icons).toHaveLength(2)
    expect(icons.filter((img) => img.closest("[aria-hidden]") && img.className.includes("blur"))).toHaveLength(1)
    expect(icons.filter((img) => !img.closest("[aria-hidden]"))).toHaveLength(1)
    expect(link.textContent).toBe("Icon")
    expect(commands.pluginBanner).not.toHaveBeenCalled()
  })

  it("a plugin without banner or icon shows its name as its artwork", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.plain", name: "A plain plugin with a longer name" })]
    show()
    const link = await art("A plain plugin with a longer name")
    // The name once, set large; no picture and no glyph.
    expect(within(link).getAllByText("A plain plugin with a longer name")).toHaveLength(1)
    expect(within(link).getByText("A plain plugin with a longer name").className).toMatch(/display/)
    expect(link.querySelector("img, svg")).toBeNull()
  })

  it("a banner that isn't a picture's data URL isn't drawn", async () => {
    const answers = new Map([
      ["dev.a.link", "https://example.com/banner.png"],
      ["dev.a.html", "data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg=="],
      // A banner is never an SVG: one could hold markup.
      ["dev.a.svg", ICON],
      ["dev.a.quoted", `${BANNER}"><script>alert(1)</script>`],
    ])
    vi.mocked(commands.pluginBanner).mockImplementation((id) => Promise.resolve(answers.get(id) ?? null))
    state.plugins = [...answers.keys()].map((id, i) => pluginInfo({ id, name: id, bannerHash: `h${i}` }))
    const { container } = show()
    for (const id of answers.keys())
      await waitFor(() => expect(queryClient.getQueryState(qk.pluginBanner(id, `h${[...answers.keys()].indexOf(id)}`))?.status).toBe("success"))
    await act(() => new Promise((done) => setTimeout(done, 0)))
    expect(container.querySelectorAll("img")).toHaveLength(0)
    expect(container.innerHTML).not.toContain("example.com")
    expect(container.innerHTML).not.toContain("alert")
    // Each shows its name as its artwork instead.
    for (const id of answers.keys()) expect((await art(id)).textContent).toBe(id)
  })

  it("a plugin turned off shows dimmed and its switch off", async () => {
    state.plugins = [
      pluginInfo({ id: "dev.a.on", name: "On", icon: ICON }),
      pluginInfo({ id: "dev.a.off", name: "Off", icon: ICON, enabled: false, active: false }),
    ]
    show()
    const dimmed = (link: HTMLElement) => link.querySelector(".opacity-60") != null
    expect(dimmed(await art("Off"))).toBe(true)
    expect(dimmed(await art("On"))).toBe(false)
    expect(screen.getByRole("switch", { name: "Enable Off" }).getAttribute("aria-checked")).toBe("false")
    expect(screen.getByRole("switch", { name: "Enable On" }).getAttribute("aria-checked")).toBe("true")
    expect(screen.getByText("2 plugins · 1 running")).toBeTruthy()
  })

  it("a working folder plugin's menu checks its folder for changes", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.me.mine", name: "Mine", trust: "unreviewed", source: "folder" })]
    show()
    expect(await entries("Mine")).toEqual([
      "Its settings",
      "Show its folder",
      "Show its log",
      "Copy its id",
      "Check its folder for changes",
      "Uninstall…",
    ])
    await waitFor(() => expect(commands.pluginsList).toHaveBeenCalledTimes(1))
    const done = vi.spyOn(toast, "success")
    const failed = vi.spyOn(toast, "error")
    const check = async () => {
      await user.click(screen.getByRole("button", { name: "Mine options" }))
      await user.click(await screen.findByRole("menuitem", { name: "Check its folder for changes" }))
    }
    // The list reads its plugin.json anew, and says so when nothing changed.
    await check()
    await waitFor(() => expect(done).toHaveBeenLastCalledWith("Its folder is as it was"))
    expect(commands.pluginsList).toHaveBeenCalledTimes(2)
    expect(commands.pluginInspectFolder).not.toHaveBeenCalled()
    // A change says it was read.
    state.plugins = [{ ...state.plugins[0], version: "1.1.0" }]
    await check()
    await waitFor(() => expect(done).toHaveBeenLastCalledWith("Read the changes in its folder"))
    // A list that can't be read says why, not nothing.
    const unreadable = { code: "io", message: "The plugins folder can't be read." }
    vi.mocked(commands.pluginsList).mockRejectedValueOnce(unreadable).mockRejectedValueOnce(unreadable)
    await check()
    await waitFor(
      () => expect(failed).toHaveBeenCalledExactlyOnceWith("Couldn't check its folder", { description: "The plugins folder can't be read." }),
      { timeout: 5000 },
    )
    expect(done).toHaveBeenCalledTimes(2)
  })

  it("a broken folder plugin's menu loads its folder again", async () => {
    const user = userEvent.setup()
    state.allow = true
    state.plugins = [
      pluginInfo({
        id: "dev.me.mine",
        name: "Mine",
        trust: "unreviewed",
        source: "folder",
        folder: "D:\\Plugins\\mine",
        broken: "Its program changed since it was loaded.",
      }),
    ]
    const review: PluginReview = {
      ticketId: "t",
      plugin: state.plugins[0],
      update: null,
      needsUnreviewed: true,
      allowed: true,
      uvMissing: false,
      catalog: null,
    }
    let read: (r: PluginReview) => void = () => {}
    vi.mocked(commands.pluginInspectFolder).mockReturnValueOnce(new Promise((resolve) => (read = resolve)))
    show()
    // Once the setting is in: the entry shows only while unreviewed plugins are allowed.
    await waitFor(() => expect(screen.getByRole("button", { name: "Load a folder…" })).toBeTruthy())
    const items = await entries("Mine")
    expect(items).toContain("Load its folder again…")
    expect(items).not.toContain("Check its folder for changes")
    await user.click(screen.getByRole("button", { name: "Mine options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Load its folder again…" }))
    // Through its review, the one way to agree to a changed program.
    await waitFor(() => expect(commands.pluginInspectFolder).toHaveBeenCalledExactlyOnceWith("D:\\Plugins\\mine"))
    // The header's buttons weren't pressed: they neither spin nor wait.
    const load = screen.getByRole("button", { name: "Load a folder…" })
    expect(load.querySelector(".animate-spin")).toBeNull()
    expect((load as HTMLButtonElement).disabled).toBe(false)
    act(() => read(review))
    expect(await screen.findByRole("dialog")).toBeTruthy()
    // A package that is broken has no folder to load.
    usePluginReview.getState().close()
    cleanup()
    state.plugins = [pluginInfo({ id: "dev.a.pkg", name: "Pkg", broken: "Needs a newer AutoScene." })]
    queryClient.clear()
    show()
    const pkg = await entries("Pkg")
    expect(pkg).not.toContain("Load its folder again…")
    expect(pkg).not.toContain("Check its folder for changes")
  })

  it("offers no Load its folder again… while unreviewed plugins are off", async () => {
    // The backend refuses a folder's review then: the entry could only fail.
    state.plugins = [
      pluginInfo({ id: "dev.me.mine", name: "Mine", trust: "unreviewed", source: "folder", broken: "Its program changed since it was loaded." }),
    ]
    show()
    await waitFor(() => expect((allowSwitch() as HTMLButtonElement).disabled).toBe(false))
    expect(await entries("Mine")).toEqual(["Its settings", "Show its folder", "Show its log", "Copy its id", "Uninstall…"])
    // The hero's menu agrees.
    cleanup()
    queryClient.clear()
    show(<PluginHero p={state.plugins[0]} />)
    // The menu reads the setting as it opens.
    await entries("Mine")
    await waitFor(() => expect(queryClient.getQueryState(qk.settings)?.status).toBe("success"))
    expect(await entries("Mine")).toEqual(["Show its folder", "Show its log", "Copy its id", "Uninstall…"])
  })

  it("a broken folder's hero loads its folder again through its review", async () => {
    const user = userEvent.setup()
    state.allow = true
    const p = pluginInfo({
      id: "dev.me.mine",
      name: "Mine",
      trust: "unreviewed",
      source: "folder",
      folder: "D:\\Plugins\\mine",
      broken: "Its program changed since it was loaded.",
    })
    state.plugins = [p]
    vi.mocked(commands.pluginInspectFolder).mockResolvedValueOnce({
      ticketId: "t",
      plugin: p,
      update: null,
      needsUnreviewed: true,
      allowed: true,
      uvMissing: false,
      catalog: null,
    })
    show(<PluginHero p={p} />)
    await entries("Mine")
    await waitFor(() => expect(queryClient.getQueryState(qk.settings)?.status).toBe("success"))
    expect(await entries("Mine")).toContain("Load its folder again…")
    await user.click(screen.getByRole("button", { name: "Mine options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Load its folder again…" }))
    await waitFor(() => expect(commands.pluginInspectFolder).toHaveBeenCalledExactlyOnceWith("D:\\Plugins\\mine"))
    expect(await screen.findByRole("dialog")).toBeTruthy()
  })

  it("copies a plugin's id", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    show()
    await user.click(await screen.findByRole("button", { name: "One options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Copy its id" }))
    expect(copyText).toHaveBeenCalledExactlyOnceWith("dev.a.one", "plugin id")
  })

  it("shows Try again when the plugins can't be read", async () => {
    const user = userEvent.setup()
    vi.mocked(commands.pluginsList).mockRejectedValueOnce({ code: "io", message: "The plugins folder can't be read." })
    vi.mocked(commands.pluginsList).mockRejectedValueOnce({ code: "io", message: "The plugins folder can't be read." })
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    show()
    expect(await screen.findByText("Couldn't read the plugins", undefined, { timeout: 5000 })).toBeTruthy()
    expect(screen.getByText("The plugins folder can't be read.")).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Try again" }))
    expect(await card("One")).toBeTruthy()
    expect(screen.queryByText("Couldn't read the plugins")).toBeNull()
  })

  it("leaves the question about unreviewed plugins to AutoScene's own dialog", async () => {
    show()
    await waitFor(() => expect((allowSwitch() as HTMLButtonElement).disabled).toBe(false))
    // The person said no there: it stays off.
    vi.mocked(commands.pluginsAllowUnreviewed).mockResolvedValueOnce(settings(false))
    fireEvent.click(allowSwitch())
    await waitFor(() => expect(commands.pluginsAllowUnreviewed).toHaveBeenCalledExactlyOnceWith(true))
    expect(screen.queryByRole("dialog")).toBeNull()
    expect(allowSwitch().getAttribute("aria-checked")).toBe("false")

    fireEvent.click(allowSwitch())
    await waitFor(() => expect(allowSwitch().getAttribute("aria-checked")).toBe("true"))
    // Through its own command, never a settings save.
    expect(commands.pluginsAllowUnreviewed).toHaveBeenLastCalledWith(true)
    expect(queryClient.getQueryData<AppSettings>(qk.settings)?.allowUnreviewedPlugins).toBe(true)

    // Off needs no question.
    fireEvent.click(allowSwitch())
    await waitFor(() => expect(commands.pluginsAllowUnreviewed).toHaveBeenLastCalledWith(false))
    expect(screen.queryByRole("dialog")).toBeNull()
    await waitFor(() => expect(allowSwitch().getAttribute("aria-checked")).toBe("false"))
  })

  it("offers a plugin's actions in its ⋯ menu too", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    show()
    expect(await entries("One")).toEqual(["Its settings", "Show its folder", "Show its log", "Copy its id", "Uninstall…"])
    fireEvent.pointerDown(await screen.findByRole("button", { name: "One options" }), { button: 0, ctrlKey: false })
    fireEvent.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Uninstall…" }))
    expect(await screen.findByRole("dialog", { name: "Uninstall One?" })).toBeTruthy()
  })

  it("stays open while it uninstalls", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    let done: (v: null) => void = () => {}
    vi.mocked(commands.pluginUninstall).mockReturnValueOnce(new Promise((resolve) => (done = resolve)))
    show()
    fireEvent.pointerDown(await screen.findByRole("button", { name: "One options" }), { button: 0, ctrlKey: false })
    fireEvent.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Uninstall…" }))
    const dialog = await screen.findByRole("dialog")
    fireEvent.click(within(dialog).getByRole("button", { name: "Uninstall" }))
    await waitFor(() => expect((within(dialog).getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled).toBe(true))
    fireEvent.keyDown(dialog, { key: "Escape" })
    fireEvent.click(within(dialog).getByRole("button", { name: "Close" }))
    expect(screen.getByRole("dialog")).toBeTruthy()
    done(null)
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
  })

  it("spins the button whose choice is being checked", async () => {
    state.allow = true
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    let checked: (r: PluginReview) => void = () => {}
    vi.mocked(commands.pluginInspectFolder).mockReturnValueOnce(new Promise((resolve) => (checked = resolve)))
    show()
    const load = await screen.findByRole("button", { name: /Load a folder/ })
    fireEvent.click(load)
    await waitFor(() => expect(load.querySelector(".animate-spin")).not.toBeNull())
    expect(screen.getByRole("button", { name: /Install a plugin/ }).querySelector(".animate-spin")).toBeNull()
    checked({ ticketId: "t", plugin: pluginInfo({ id: "dev.me.mine", name: "Mine" }), update: null, needsUnreviewed: true, allowed: true, uvMissing: false, catalog: null })
    await waitFor(() => expect(load.querySelector(".animate-spin")).toBeNull())
  })

  it("opens the review of the package it picked, in the shell's dialog", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    show()
    const install = await screen.findByRole("button", { name: /Install a plugin/ })
    // A package that can't be read is named by its file.
    vi.mocked(commands.pluginInspect).mockRejectedValueOnce({ code: "invalid", message: "Not a plugin package." })
    fireEvent.click(install)
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't open Notify.asp", { description: "Not a plugin package." }))
    expect(screen.queryByRole("dialog")).toBeNull()

    vi.mocked(commands.pluginInspect).mockResolvedValueOnce({
      ticketId: "t",
      plugin: pluginInfo({ id: "dev.me.notify", name: "Notify" }),
      update: null,
      needsUnreviewed: false,
      allowed: true,
      uvMissing: false,
      catalog: null,
    })
    fireEvent.click(install)
    expect(await screen.findByRole("heading", { name: "Install Notify?" })).toBeTruthy()
    expect(commands.pluginInspect).toHaveBeenLastCalledWith("D:\\Downloads\\Notify.asp")
  })

  it("uninstalls a plugin, and what it kept only when asked", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" }), pluginInfo({ id: "dev.a.two", name: "Two" })]
    show(
      <AppMenu>
        <div>
          <PluginsPage />
        </div>
      </AppMenu>,
    )
    const uninstall = async (name: string) => {
      fireEvent.contextMenu(await card(name), { clientX: 40, clientY: 40 })
      fireEvent.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Uninstall…" }))
      return screen.findByRole("dialog")
    }
    // What it kept between runs stays, unless the person says otherwise.
    let dialog = await uninstall("One")
    expect(within(dialog).getByRole("switch", { name: "Delete what it kept too" }).getAttribute("aria-checked")).toBe("false")
    fireEvent.click(within(dialog).getByRole("button", { name: "Uninstall" }))
    await waitFor(() => expect(commands.pluginUninstall).toHaveBeenCalledExactlyOnceWith("dev.a.one", false))
    await waitFor(() => expect(screen.queryByRole("group", { name: "One" })).toBeNull())

    dialog = await uninstall("Two")
    fireEvent.click(within(dialog).getByRole("switch", { name: "Delete what it kept too" }))
    fireEvent.click(within(dialog).getByRole("button", { name: "Uninstall" }))
    await waitFor(() => expect(commands.pluginUninstall).toHaveBeenLastCalledWith("dev.a.two", true))
  })

  it("hides Load a folder while unreviewed plugins are off", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    show()
    expect(await screen.findByRole("button", { name: "Install a plugin…" })).toBeTruthy()
    await waitFor(() => expect((allowSwitch() as HTMLButtonElement).disabled).toBe(false))
    expect(screen.queryByRole("button", { name: "Load a folder…" })).toBeNull()
    cleanup()
    queryClient.clear()

    state.allow = true
    show()
    expect(await screen.findByRole("button", { name: "Load a folder…" })).toBeTruthy()
  })

  it("opens plugin folder and log via ⋯ menu items", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.alpha", name: "Alpha Tool", folder: "/path/to/folder", logPath: "/path/to/log.txt" })]
    show()
    const trigger = await screen.findByRole("button", { name: "Alpha Tool options" })
    await user.click(trigger)
    await user.click(await screen.findByRole("menuitem", { name: "Show its folder" }))
    expect(reveal).toHaveBeenCalledWith("/path/to/folder")

    await user.click(trigger)
    await user.click(await screen.findByRole("menuitem", { name: "Show its log" }))
    expect(reveal).toHaveBeenCalledWith("/path/to/log.txt")
  })

  // A guard for good: a picture fetched from a link would tell its host who
  // looked, and when, and could make a claim look checked.
  it("no picture on the plugin screens comes from the internet", async () => {
    // Its plugin.json may name its author's GitHub page: only its own files show.
    state.plugins = [
      pluginInfo({
        id: "dev.a.art",
        name: "Art",
        author: "someone",
        repository: "https://github.com/someone/art",
        icon: ICON,
        bannerHash: "a1",
        settings: [{ key: "on", title: "On", description: null, kind: { type: "boolean", default: true } }],
      }),
      // A banner answered with a link isn't shown.
      pluginInfo({ id: "dev.a.link", name: "Link", author: "someone", repository: "https://github.com/someone/link", bannerHash: "b2" }),
    ]
    vi.mocked(commands.pluginBanner).mockImplementation((id) =>
      Promise.resolve(id === "dev.a.art" ? BANNER : "https://github.com/someone.png"),
    )
    const { container } = show(
      <>
        <PluginsPage />
        <PluginHero p={state.plugins[0]} />
      </>,
    )
    // The banner on its card, and blurred and framed in its settings' hero.
    await waitFor(() => expect(container.querySelectorAll(`img[src="${BANNER}"]`)).toHaveLength(3))
    await waitFor(() => expect(queryClient.getQueryState(qk.pluginBanner("dev.a.link", "b2"))?.status).toBe("success"))
    await act(() => new Promise((done) => setTimeout(done, 0)))
    const sources = [...container.querySelectorAll("img")].map((img) => img.getAttribute("src"))
    expect(sources.length).toBeGreaterThan(2)
    for (const src of sources) expect(src).toMatch(/^data:/)
    expect(container.innerHTML).not.toContain("someone.png")
  })

  // The page never opens a plugin's link itself: AutoScene reads it from the
  // plugin.json, and asks first unless it is a GitHub repository.
  it("Show the source it names asks AutoScene to open it", async () => {
    const user = userEvent.setup()
    const p = pluginInfo({ id: "dev.a.mine", name: "Mine", trust: "unreviewed", repository: "https://gitlab.com/a/b" })
    state.plugins = [p]
    const page = show()
    await user.click(await screen.findByRole("button", { name: "Mine options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Show the source it names" }))
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledWith("dev.a.mine"))
    page.unmount()
    // Its settings say the same, in their ⋯ (the card's list without
    // Its settings) and in their right-click.
    const settings = show(
      <AppMenu>
        <div>
          <PluginHero p={p} />
        </div>
      </AppMenu>,
    )
    await user.click(await screen.findByRole("button", { name: "Mine options" }))
    const items = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
    expect(items).toEqual(["Show the source it names", "Show its folder", "Show its log", "Copy its id", "Uninstall…"])
    await user.click(screen.getByRole("menuitem", { name: "Show the source it names" }))
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledTimes(2))
    fireEvent.contextMenu(screen.getByRole("heading", { name: "Mine" }), { clientX: 40, clientY: 40 })
    const right = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
    // The plugin's list first; the app's own entries follow it.
    expect(right.slice(0, items.length)).toEqual(items)
    fireEvent.click(within(await screen.findByRole("menu")).getByRole("menuitem", { name: "Show the source it names" }))
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledTimes(3))
    expect(vi.mocked(commands.pluginOpenSource).mock.calls).toEqual([["dev.a.mine"], ["dev.a.mine"], ["dev.a.mine"]])
    expect(openExternalUrl).not.toHaveBeenCalled()
    settings.unmount()
  })

  // A no says nothing more; a failure says why.
  it("Show its source says only what went wrong", async () => {
    const user = userEvent.setup()
    const error = vi.spyOn(toast, "error")
    state.plugins = [pluginInfo({ id: "dev.a.mine", name: "Mine", repository: "https://gitlab.com/a/b" })]
    show()
    const open = async () => {
      await user.click(await screen.findByRole("button", { name: "Mine options" }))
      await user.click(await screen.findByRole("menuitem", { name: "Show its source" }))
    }
    vi.mocked(commands.pluginOpenSource).mockResolvedValueOnce(false)
    await open()
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledTimes(1))
    await act(() => new Promise((done) => setTimeout(done, 0)))
    expect(error).not.toHaveBeenCalled()
    vi.mocked(commands.pluginOpenSource).mockRejectedValueOnce({ code: "io", message: "AutoScene couldn't open the link." })
    await open()
    await waitFor(() =>
      expect(error).toHaveBeenCalledExactlyOnceWith("Couldn't open the link", { description: "AutoScene couldn't open the link." }),
    )
    error.mockRestore()
  })

  it("a reviewed package says Show its source", async () => {
    const user = userEvent.setup()
    const repository = "https://github.com/a/b"
    state.plugins = [
      pluginInfo({ id: "dev.a.reviewed", name: "Reviewed", repository }),
      // Signed is not reviewed, and a loaded folder never is.
      pluginInfo({ id: "dev.a.pinned", name: "Pinned", trust: "pinned", repository }),
      pluginInfo({ id: "dev.a.folder", name: "Folder", source: "folder", repository }),
    ]
    show()
    expect(await entries("Reviewed")).toEqual(["Its settings", "Show its source", "Show its folder", "Show its log", "Copy its id", "Uninstall…"])
    expect((await entries("Pinned"))[1]).toBe("Show the source it names")
    expect((await entries("Folder"))[1]).toBe("Show the source it names")
    await user.click(await screen.findByRole("button", { name: "Reviewed options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Show its source" }))
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledWith("dev.a.reviewed"))
    expect(openExternalUrl).not.toHaveBeenCalled()
  })

  it("no source entry without a repository", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.plain", name: "Plain", trust: "unreviewed" })]
    const page = show()
    expect(await entries("Plain")).toEqual(["Its settings", "Show its folder", "Show its log", "Copy its id", "Uninstall…"])
    page.unmount()
    // Nor does the ⋯ of its settings.
    show(<PluginHero p={state.plugins[0]} />)
    await user.click(await screen.findByRole("button", { name: "Plain options" }))
    const ownItems = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
    expect(ownItems).toEqual(["Show its folder", "Show its log", "Copy its id", "Uninstall…"])
    expect(screen.queryByText(/source/)).toBeNull()
    expect(commands.pluginOpenSource).not.toHaveBeenCalled()
  })
})
