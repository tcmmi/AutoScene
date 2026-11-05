import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter, Outlet } from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, PluginInfo } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { pluginInfo } from "@/test/plugins"
import { PluginSettingsPage } from "./PluginSettingsPage"

vi.mock("sonner", async (original) => {
  const real = await original<typeof import("sonner")>()
  return { ...real, toast: Object.assign(vi.fn(), real.toast) }
})

const { state } = vi.hoisted(() => ({ state: { plugins: [] as PluginInfo[], allow: true } }))

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
    pluginSettingsGet: vi.fn(() => Promise.resolve({})),
    pluginBanner: vi.fn(() => Promise.resolve(null)),
    pluginSetEnabled: vi.fn(),
    pluginsAllowUnreviewed: vi.fn((on: boolean) => Promise.resolve(settings(on))),
    pluginInspectFolder: vi.fn(() => new Promise(() => {})),
    pluginOpenSource: vi.fn(() => Promise.resolve(true)),
    pluginSettingsSet: vi.fn((_id: string, v: Record<string, unknown>) => Promise.resolve(v)),
    pluginUninstall: vi.fn(() => Promise.resolve(null)),
    catalogList: vi.fn(() => Promise.resolve({ status: { ready: false }, plugins: [] })),
  },
}))
// A page scrolls, which jsdom can't.
vi.mock("@/components/ui/page", () => ({
  Page: ({ topbar, children }: { topbar: React.ReactNode; children: React.ReactNode }) => (
    <main>
      {topbar}
      {children}
    </main>
  ),
}))

const BANNER = "data:image/jpeg;base64,/9j/4AAQ"
const ICON = "data:image/svg+xml;base64,PHN2Zy8+"
const BLOCKED = "Plugins that AutoScene has not reviewed are turned off. You can allow them in Plugins."
const NOT_REVIEWED = "AutoScene hasn't reviewed this plugin, so it doesn't run."

/** The app's Plugins and a plugin's settings, at `/plugins/<id>`. */
function show(id: string) {
  const root = createRootRoute({ component: Outlet })
  const plugins = createRoute({ getParentRoute: () => root, path: "/plugins", component: () => <p>All plugins</p> })
  const plugin = createRoute({ getParentRoute: () => root, path: "/plugins/$id", component: PluginSettingsPage })
  const router = createRouter({
    routeTree: root.addChildren([plugins, plugin]),
    history: createMemoryHistory({ initialEntries: [`/plugins/${id}`] }),
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <RouterProvider router={router} />
      </TooltipProvider>
    </QueryClientProvider>,
  )
  return { router, container: view.container }
}

/** The plugin's name in its hero (the window's top bar names it too). */
async function heroName(name: string) {
  const all = await screen.findAllByRole("heading", { level: 1, name })
  return all.find((h) => h.className.includes("display"))!
}

/** Each place `src` is drawn: blurred in the hero's backdrop, or sharp and framed beside the name. */
function drawn(container: HTMLElement, src: string) {
  const imgs = [...container.querySelectorAll<HTMLImageElement>(`img[src="${src}"]`)]
  const backdrop = imgs.filter((img) => img.closest("[aria-hidden]"))
  const framed = imgs.filter((img) => !img.closest("[aria-hidden]"))
  return { backdrop, framed }
}

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
  // The router puts each place at its top, which jsdom can't scroll.
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  state.plugins = []
  state.allow = true
})
afterEach(cleanup)

describe("a plugin's settings", () => {
  it("its banner is blurred behind it and framed beside its name, else its icon", async () => {
    vi.mocked(commands.pluginBanner).mockResolvedValue(BANNER)
    state.plugins = [
      pluginInfo({ id: "dev.a.art", name: "Art", icon: ICON, bannerHash: "a1" }),
      pluginInfo({ id: "dev.a.icon", name: "Icon", icon: ICON }),
      pluginInfo({ id: "dev.a.plain", name: "Plain" }),
    ]
    let { container } = show("dev.a.art")
    await screen.findAllByRole("heading", { level: 1, name: "Art" })
    await waitFor(() => expect(drawn(container, BANNER).framed).toHaveLength(1))
    let art = drawn(container, BANNER)
    expect(art.backdrop).toHaveLength(1)
    expect(art.backdrop[0].className).toContain("blur-3xl")
    expect(art.framed[0].className).not.toContain("blur")
    // With the banner in hand, the icon isn't drawn too.
    expect(container.querySelector(`img[src="${ICON}"]`)).toBeNull()
    cleanup()

    ;({ container } = show("dev.a.icon"))
    await screen.findAllByRole("heading", { level: 1, name: "Icon" })
    art = drawn(container, ICON)
    expect(art.backdrop).toHaveLength(1)
    expect(art.backdrop[0].className).toContain("blur-3xl")
    expect(art.framed).toHaveLength(1)
    expect(commands.pluginBanner).not.toHaveBeenCalledWith("dev.a.icon")
    cleanup()

    // Neither: no picture, and no frame for one.
    ;({ container } = show("dev.a.plain"))
    await screen.findAllByRole("heading", { level: 1, name: "Plain" })
    expect(container.querySelectorAll("img")).toHaveLength(0)
    expect(container.querySelector(".artwork-fallback")).not.toBeNull()
  })

  it("the trust badge and the facts are not on the plugin's picture", async () => {
    vi.mocked(commands.pluginBanner).mockResolvedValue(BANNER)
    state.plugins = [
      pluginInfo({ id: "dev.a.art", name: "Art", trust: "unreviewed", license: "MIT", icon: ICON, bannerHash: "a1" }),
      pluginInfo({ id: "dev.a.icon", name: "Icon", trust: "unreviewed", license: "MIT", icon: ICON }),
    ]
    for (const [id, src] of [
      ["dev.a.art", BANNER],
      ["dev.a.icon", ICON],
    ]) {
      const { container } = show(id)
      const facts = [
        await screen.findByText("Not reviewed"),
        screen.getByText("version 1.0.0"),
        screen.getByText("MIT"),
        screen.getByText("Says it is by Someone"),
      ]
      await waitFor(() => expect(drawn(container, src).framed).toHaveLength(1))
      const { backdrop, framed } = drawn(container, src)
      // The backdrop, and the framed picture with its frame: no word of AutoScene's on either.
      const pictures = [backdrop[0].closest("[aria-hidden]")!, framed[0].parentElement!.tagName === "SPAN" ? framed[0].parentElement! : framed[0]]
      for (const picture of pictures) {
        expect(picture.textContent).toBe("")
        for (const fact of facts) expect(picture.contains(fact)).toBe(false)
      }
      cleanup()
    }
  })

  it("a plugin AutoScene hasn't reviewed says who it says it is by", async () => {
    state.plugins = [
      pluginInfo({ id: "dev.a.reviewed", name: "Reviewed", author: "Moongetsu" }),
      pluginInfo({ id: "dev.a.signed", name: "Signed", author: "Moongetsu", trust: "pinned" }),
      pluginInfo({ id: "dev.a.folder", name: "Folder", author: "Moongetsu", source: "folder" }),
    ]
    show("dev.a.reviewed")
    expect(await screen.findByText("By Moongetsu")).toBeTruthy()
    expect(screen.getByText("Reviewed by AutoScene")).toBeTruthy()
    cleanup()
    show("dev.a.signed")
    expect(await screen.findByText("Says it is by Moongetsu")).toBeTruthy()
    expect(screen.getByText("Signed, not reviewed")).toBeTruthy()
    expect(screen.queryByText("By Moongetsu")).toBeNull()
    cleanup()
    show("dev.a.folder")
    expect(await screen.findByText("Says it is by Moongetsu")).toBeTruthy()
    expect(screen.getByText("Loaded from a folder")).toBeTruthy()
  })

  it("turns it off from its screen, and turned off shows no banner", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One", icon: ICON })]
    const { container } = show("dev.a.one")
    const on = await screen.findByRole("switch", { name: "Turned on" })
    expect(on.getAttribute("aria-checked")).toBe("true")
    vi.mocked(commands.pluginSetEnabled).mockResolvedValueOnce({ ...state.plugins[0], enabled: false, active: false })
    await user.click(on)
    expect(commands.pluginSetEnabled).toHaveBeenCalledExactlyOnceWith("dev.a.one", false)
    await waitFor(() => expect(screen.getByRole("switch", { name: "Turned on" }).getAttribute("aria-checked")).toBe("false"))
    // The switch and the dimmed picture say so; nothing else does.
    expect(screen.queryByRole("status")).toBeNull()
    expect(screen.queryByText(/turned off/)).toBeNull()
    expect(drawn(container, ICON).backdrop[0].className).toContain("opacity-60")
    expect((await heroName("One")).className).toContain("text-white/60")
  })

  it("turned off, a plugin without a picture dims too", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.plain", name: "Plain", enabled: false, active: false })]
    const { container } = show("dev.a.plain")
    const name = await heroName("Plain")
    expect(container.querySelector(".artwork-fallback")!.className).toContain("opacity-60")
    expect(name.className).toContain("text-white/60")
    cleanup()
    queryClient.clear()
    state.plugins = [{ ...state.plugins[0], enabled: true, active: true }]
    const on = show("dev.a.plain")
    const lit = await heroName("Plain")
    expect(on.container.querySelector(".artwork-fallback")!.className).not.toContain("opacity-60")
    expect(lit.className).not.toContain("text-white/60")
  })

  it("a blocked plugin offers Allow them…, which asks the backend", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One", trust: "unreviewed", active: false, blockedReason: BLOCKED })]
    show("dev.a.one")
    const banner = await screen.findByRole("status")
    // Its own words: the backend's send people to Plugins, where this is.
    expect(within(banner).getByText(NOT_REVIEWED)).toBeTruthy()
    expect(screen.queryByText(BLOCKED)).toBeNull()
    // A choice of the person's, not a fault: not in red, and with an icon like every banner.
    expect(banner.className).not.toContain("danger")
    expect(banner.className).toContain("border-hairline")
    expect(banner.querySelector("svg")).not.toBeNull()
    await user.click(within(banner).getByRole("button", { name: "Allow them…" }))
    // The backend asks in the system's own dialog: the page can't say yes.
    expect(commands.pluginsAllowUnreviewed).toHaveBeenCalledExactlyOnceWith(true)
    cleanup()
    // Turned off, it says nothing of it.
    state.plugins = [{ ...state.plugins[0], enabled: false }]
    queryClient.clear()
    show("dev.a.one")
    await screen.findAllByRole("heading", { level: 1, name: "One" })
    expect(screen.queryByRole("status")).toBeNull()
  })

  it("a broken loaded folder offers Load its folder again…", async () => {
    const user = userEvent.setup()
    const broken = "Its program changed since it was loaded. Load its folder again to agree to it."
    state.plugins = [
      pluginInfo({ id: "dev.a.folder", name: "Folder", source: "folder", trust: "unreviewed", active: false, broken }),
      pluginInfo({ id: "dev.a.package", name: "Package", active: false, broken: "This plugin needs a newer AutoScene." }),
    ]
    show("dev.a.folder")
    const banner = await screen.findByRole("status")
    expect(within(banner).getByText(broken)).toBeTruthy()
    expect(banner.className).toContain("border-danger")
    const again = await within(banner).findByRole("button", { name: "Load its folder again…" })
    await user.click(again)
    // Through its review, the one way to agree to a changed program.
    expect(commands.pluginInspectFolder).toHaveBeenCalledExactlyOnceWith("C:\\AutoScene\\plugins\\dev.a.folder")
    await waitFor(() => expect(again.getAttribute("aria-busy")).toBe("true"))
    cleanup()
    // A package's reason says what to do; there is no button for it.
    show("dev.a.package")
    const said = await screen.findByRole("status")
    expect(within(said).getByText("This plugin needs a newer AutoScene.")).toBeTruthy()
    expect(within(said).queryByRole("button")).toBeNull()
    cleanup()
    // While unreviewed plugins are off, the backend refuses a folder's review: no button that could only fail.
    state.allow = false
    queryClient.clear()
    show("dev.a.folder")
    const off = await screen.findByRole("status")
    await waitFor(() => expect(queryClient.getQueryState(qk.settings)?.status).toBe("success"))
    expect(within(off).getByText(broken)).toBeTruthy()
    expect(within(off).queryByRole("button")).toBeNull()
  })

  it("a plugin that isn't installed says so, with Go to Plugins", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.other", name: "Other" })]
    const { router } = show("dev.a.gone")
    const title = await screen.findByText("This plugin isn't installed")
    expect(screen.queryByRole("switch")).toBeNull()
    // In the middle of the view, not in the settings' column.
    expect(title.closest(".max-w-4xl")).toBeNull()
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull()
    await user.click(screen.getByRole("button", { name: "Go to Plugins" }))
    await waitFor(() => expect(router.state.location.pathname).toBe("/plugins"))
  })

  it("uses the graphics card says why its runs wait", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.gpu", name: "Gpu", gpu: true }), pluginInfo({ id: "dev.a.cpu", name: "Cpu" })]
    show("dev.a.gpu")
    expect(await screen.findByText("Uses the graphics card")).toBeTruthy()
    expect(screen.getByText("Its runs wait while an analysis runs.")).toBeTruthy()
    expect(screen.queryByText("Nothing beyond its own settings.")).toBeNull()
    cleanup()
    show("dev.a.cpu")
    await screen.findAllByRole("heading", { level: 1, name: "Cpu" })
    expect(screen.queryByText("Uses the graphics card")).toBeNull()
  })

  it("a list it couldn't read offers Try again", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    // Read twice (the query tries once more) before it says so.
    const gone = { code: "io", message: "The disk is gone." }
    vi.mocked(commands.pluginsList).mockRejectedValueOnce(gone).mockRejectedValueOnce(gone)
    show("dev.a.one")
    const title = await screen.findByText("Couldn't read the plugins", undefined, { timeout: 4000 })
    expect(title.closest(".max-w-4xl")).toBeNull()
    expect(screen.getByRole("button", { name: "Open logs" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Go to Plugins" })).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Try again" }))
    expect(await heroName("One")).toBeTruthy()
  })

  it("uninstalls from its own screen, then goes to Plugins", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" }), pluginInfo({ id: "dev.a.two", name: "Two" })]
    const { router } = show("dev.a.one")
    await user.click(await screen.findByRole("button", { name: "One options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Uninstall…" }))
    const dialog = await screen.findByRole("dialog", { name: "Uninstall One?" })
    await user.click(within(dialog).getByRole("button", { name: "Uninstall" }))
    expect(commands.pluginUninstall).toHaveBeenCalledExactlyOnceWith("dev.a.one", false)
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Uninstalled One"))
    await waitFor(() => expect(router.state.location.pathname).toBe("/plugins"))
    expect(await screen.findByText("All plugins")).toBeTruthy()
    // It never said the plugin it just removed isn't installed.
    expect(screen.queryByText("This plugin isn't installed")).toBeNull()
    await waitFor(() => expect(queryClient.getQueryData<PluginInfo[]>(qk.plugins)?.map((x) => x.id)).toEqual(["dev.a.two"]))
  })
})

describe("a plugin AutoScene withdrew", () => {
  const leaky = () =>
    pluginInfo({ id: "dev.example.leaky", name: "Leaky Export", icon: ICON, withdrawn: "It sent export paths to a server.", active: false })
  /** The banner under the hero. */
  const banner = async () => (await screen.findByText("AutoScene withdrew Plugin “Leaky Export”")).closest("[role=status]") as HTMLElement

  it("says so under its hero, with its badge, and can't be turned on", async () => {
    state.plugins = [leaky()]
    const { container } = show("dev.example.leaky")
    const b = await banner()
    expect(within(b).getByText("It sent export paths to a server. It is turned off.")).toBeTruthy()
    // Its badge says Withdrawn in place of its trust.
    expect(screen.getByText("Withdrawn")).toBeTruthy()
    expect(screen.queryByText("Reviewed by AutoScene")).toBeNull()
    // AutoScene's words are beside the plugin's picture, never on it.
    expect(drawn(container, ICON).backdrop.concat(drawn(container, ICON).framed).every((img) => !b.contains(img))).toBe(true)
    // Dimmed, and off for good.
    expect(drawn(container, ICON).backdrop[0].className).toContain("opacity-60")
    const on = screen.getByRole("switch", { name: "Turned on" })
    expect(on.getAttribute("aria-checked")).toBe("false")
    expect((on as HTMLButtonElement).disabled).toBe(true)
    // Only this banner: nothing else on the screen asks to be fixed.
    expect(screen.getAllByRole("status")).toHaveLength(1)
  })

  it("Uninstall… asks, and once it is gone the screen goes to Plugins", async () => {
    const user = userEvent.setup()
    state.plugins = [leaky()]
    const { router } = show("dev.example.leaky")
    await user.click(within(await banner()).getByRole("button", { name: "Uninstall…" }))
    const dialog = await screen.findByRole("dialog", { name: "Uninstall Leaky Export?" })
    await user.click(within(dialog).getByRole("button", { name: "Uninstall" }))
    expect(commands.pluginUninstall).toHaveBeenCalledExactlyOnceWith("dev.example.leaky", false)
    await waitFor(() => expect(router.state.location.pathname).toBe("/plugins"))
    expect(screen.queryByText("This plugin isn't installed")).toBeNull()
  })

  it("a withdrawn plugin that is also broken or not allowed has this one banner and no fix", async () => {
    // Broken, from a folder (which "Load its folder again…" would offer a fix for).
    state.plugins = [{ ...leaky(), source: "folder", broken: "Its plugin.json is missing." }]
    state.allow = true
    show("dev.example.leaky")
    await banner()
    expect(screen.getAllByRole("status")).toHaveLength(1)
    expect(screen.queryByText("Its plugin.json is missing.")).toBeNull()
    expect(screen.queryByRole("button", { name: "Load its folder again…" })).toBeNull()
    cleanup()

    // Not allowed (unreviewed, so off by the setting).
    state.plugins = [{ ...leaky(), enabled: true, blockedReason: BLOCKED }]
    queryClient.clear()
    show("dev.example.leaky")
    await banner()
    expect(screen.getAllByRole("status")).toHaveLength(1)
    expect(screen.queryByText(NOT_REVIEWED)).toBeNull()
    expect(screen.queryByRole("button", { name: "Allow them…" })).toBeNull()
  })

  it("offers nothing to set up: no after-export switches, and its settings stay still", async () => {
    state.plugins = [
      {
        ...leaky(),
        exportActions: [{ id: "list", title: "List the clips", description: null, afterExport: true, afterFailed: false }],
        settings: [{ key: "on", title: "Post it", description: null, kind: { type: "boolean", default: true } }],
        pages: [{ id: "main", title: "Leaky Export", description: null, entry: "ui/index.html", scope: "global", rail: false }],
      },
    ]
    show("dev.example.leaky")
    await banner()
    expect(screen.queryByText("After every export")).toBeNull()
    expect(screen.queryByRole("switch", { name: "Run List the clips after every export" })).toBeNull()
    expect((await screen.findByRole("switch", { name: "Post it" })).hasAttribute("disabled")).toBe(true)
    expect((screen.getByRole("button", { name: "Open Leaky Export" }) as HTMLButtonElement).disabled).toBe(true)
  })

  it("a plugin that is not withdrawn has no banner", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.fine", name: "Fine" })]
    show("dev.a.fine")
    await heroName("Fine")
    expect(screen.queryByText(/AutoScene withdrew/)).toBeNull()
  })
})

describe("going from one plugin's screen straight to another's", () => {
  const two = (setting: PluginInfo["settings"][number]) => [
    pluginInfo({ id: "dev.a.aaa", name: "A", settings: [setting] }),
    pluginInfo({ id: "dev.a.bbb", name: "B", settings: [setting] }),
  ]
  afterEach(() => vi.mocked(commands.pluginSettingsGet).mockImplementation(() => Promise.resolve({})))

  it("saves a slider still settling to the plugin it was moved on", async () => {
    vi.mocked(commands.pluginSettingsGet).mockImplementation((id: string) => Promise.resolve(id === "dev.a.aaa" ? { n: 7 } : { n: 2 }))
    state.plugins = two({
      key: "n",
      title: "N",
      description: null,
      kind: { type: "number", default: 5, min: 0, max: 10, step: 1, integer: true },
    })
    const { router } = show("dev.a.aaa")
    const thumb = await screen.findByRole("slider", { name: "N" })
    await waitFor(() => expect(thumb.getAttribute("aria-valuenow")).toBe("7"))
    fireEvent.keyDown(thumb, { key: "ArrowRight" })
    await act(() => router.navigate({ to: "/plugins/$id", params: { id: "dev.a.bbb" } }))
    await screen.findAllByRole("heading", { level: 1, name: "B" })
    await waitFor(() => expect(screen.getByRole("slider", { name: "N" }).getAttribute("aria-valuenow")).toBe("2"))
    // Past the time a slider rests before it saves.
    await act(() => new Promise((r) => setTimeout(r, 600)))
    expect(vi.mocked(commands.pluginSettingsSet).mock.calls).toEqual([["dev.a.aaa", { n: 8 }]])
  })

  it("doesn't carry what was typed on one over to the other", async () => {
    vi.mocked(commands.pluginSettingsGet).mockImplementation((id: string) =>
      Promise.resolve({ g: id === "dev.a.aaa" ? "on A" : "on B" }),
    )
    state.plugins = two({
      key: "g",
      title: "G",
      description: null,
      kind: { type: "string", default: null, placeholder: null, maxLength: 100 },
    })
    const { router } = show("dev.a.aaa")
    const field = (await screen.findByRole("textbox", { name: "G" })) as HTMLInputElement
    await waitFor(() => expect(field.value).toBe("on A"))
    fireEvent.change(field, { target: { value: "typed on A" } })
    await act(() => router.navigate({ to: "/plugins/$id", params: { id: "dev.a.bbb" } }))
    await screen.findAllByRole("heading", { level: 1, name: "B" })
    const b = screen.getByRole("textbox", { name: "G" }) as HTMLInputElement
    await waitFor(() => expect(b.value).toBe("on B"))
    fireEvent.blur(b)
    expect(commands.pluginSettingsSet).not.toHaveBeenCalledWith("dev.a.bbb", expect.anything())
  })
})
