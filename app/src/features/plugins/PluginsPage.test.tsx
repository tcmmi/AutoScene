import type * as React from "react"
import { QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AppSettings, PluginInfo, PluginReview } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { pluginInfo } from "@/test/plugins"
import { PluginSettingsSection } from "./PluginSettingsSection"
import { PluginsPage } from "./PluginsPage"

const { state, reveal, openExternalUrl } = vi.hoisted(() => ({
  state: { plugins: [] as PluginInfo[], allow: false },
  reveal: vi.fn(),
  openExternalUrl: vi.fn(),
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
    pluginInspectFolder: vi.fn(),
    pluginDiscard: vi.fn(() => Promise.resolve(null)),
    pluginSettingsGet: vi.fn(() => Promise.resolve({})),
    pluginBanner: vi.fn(() => Promise.resolve(null)),
    pluginOpenSource: vi.fn(() => Promise.resolve(true)),
    revealPath: vi.fn(() => Promise.resolve()),
  },
}))
vi.mock("@/lib/media", () => ({ pickFolder: vi.fn(() => Promise.resolve("D:\\Plugins\\mine")), pickPluginFile: vi.fn() }))
vi.mock("@/lib/os", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/os")>()), reveal, openExternalUrl }))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings: vi.fn() }) }))
// The window's menu, around the rows' own entries.
vi.mock("@/components/ui/page", () => ({ Page: ({ children }: { children: React.ReactNode }) => <main>{children}</main> }))
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useCanGoBack: () => false,
  useRouterState: () => ({ location: { pathname: "/plugins" } }),
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)
const allowSwitch = () => screen.getByRole("switch", { name: "Allow plugins that AutoScene has not reviewed" })

beforeEach(() => {
  queryClient.clear()
  vi.clearAllMocks()
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
        blockedReason: "Plugins that AutoScene has not reviewed are turned off in Settings.",
      }),
      // A folder says so first, whatever its trust.
      pluginInfo({ id: "dev.a.folder", name: "Folder One", trust: "unreviewed", source: "folder", enabled: false, active: false }),
    ]
    render(<PluginsPage />, { wrapper })
    const row = (name: string) => screen.findByRole("group", { name })
    const badges: [string, string][] = [
      ["Reviewed One", "Reviewed by AutoScene"],
      ["Pinned One", "Signed, not reviewed"],
      ["Unreviewed One", "Not reviewed"],
      ["Folder One", "Loaded from a folder"],
    ]
    for (const [name, badge] of badges) expect(within(await row(name)).getByText(badge)).toBeTruthy()
    expect(within(await row("Reviewed One")).getByText("By AutoScene, version 1.0.0")).toBeTruthy()
    // Only AutoScene's review stands behind the name a plugin gives its author.
    expect(within(await row("Pinned One")).getByText("Says it is by Someone, version 1.0.0")).toBeTruthy()
    // Why one that is turned on doesn't run; nothing for one turned off.
    expect(within(await row("Unreviewed One")).getByText(/turned off in Settings/)).toBeTruthy()
    const folder = screen.getByRole("switch", { name: "Enable Folder One" })
    expect(folder.getAttribute("aria-checked")).toBe("false")
    expect(screen.getByRole("switch", { name: "Enable Reviewed One" }).getAttribute("aria-checked")).toBe("true")

    // Turned on, the list shows what the backend gave back.
    vi.mocked(commands.pluginSetEnabled).mockResolvedValue({ ...state.plugins[3], enabled: true })
    fireEvent.click(folder)
    await waitFor(() => expect(screen.getByRole("switch", { name: "Enable Folder One" }).getAttribute("aria-checked")).toBe("true"))
    expect(commands.pluginSetEnabled).toHaveBeenCalledWith("dev.a.folder", true)
  })

  it("says so when no plugin is installed", async () => {
    render(<PluginsPage />, { wrapper })
    expect(await screen.findByText("No plugins yet")).toBeTruthy()
  })

  it("leaves the question about unreviewed plugins to AutoScene's own dialog", async () => {
    render(<PluginsPage />, { wrapper })
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
    render(<PluginsPage />, { wrapper })
    const trigger = await screen.findByRole("button", { name: "One options" })
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false })
    const menu = await screen.findByRole("menu")
    expect(within(menu).getAllByRole("menuitem").map((i) => i.textContent?.trim())).toEqual([
      "Show its folder",
      "Show its log",
      "Uninstall…",
    ])
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Uninstall…" }))
    expect(await screen.findByRole("dialog", { name: "Uninstall One?" })).toBeTruthy()
  })

  it("stays open while it uninstalls", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" })]
    let done: (v: null) => void = () => {}
    vi.mocked(commands.pluginUninstall).mockReturnValueOnce(new Promise((resolve) => (done = resolve)))
    render(<PluginsPage />, { wrapper })
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
    let checked: (r: PluginReview) => void = () => {}
    vi.mocked(commands.pluginInspectFolder).mockReturnValueOnce(new Promise((resolve) => (checked = resolve)))
    render(<PluginsPage />, { wrapper })
    const load = await screen.findByRole("button", { name: /Load a folder/ })
    fireEvent.click(load)
    await waitFor(() => expect(load.querySelector(".animate-spin")).not.toBeNull())
    expect(screen.getByRole("button", { name: /Install a plugin/ }).querySelector(".animate-spin")).toBeNull()
    checked({ ticketId: "t", plugin: pluginInfo({ id: "dev.me.mine", name: "Mine" }), update: null, needsUnreviewed: true, allowed: true, uvMissing: false, catalog: null })
    await waitFor(() => expect(load.querySelector(".animate-spin")).toBeNull())
  })

  it("uninstalls a plugin, and what it kept only when asked", async () => {
    state.plugins = [pluginInfo({ id: "dev.a.one", name: "One" }), pluginInfo({ id: "dev.a.two", name: "Two" })]
    render(
      <AppMenu>
        <div>
          <PluginsPage />
        </div>
      </AppMenu>,
      { wrapper },
    )
    const uninstall = async (name: string) => {
      fireEvent.contextMenu(await screen.findByRole("group", { name }), { clientX: 40, clientY: 40 })
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
    render(<PluginsPage />, { wrapper })
    expect(await screen.findByRole("button", { name: "Install a plugin…" })).toBeTruthy()
    await waitFor(() => expect((allowSwitch() as HTMLButtonElement).disabled).toBe(false))
    expect(screen.queryByRole("button", { name: "Load a folder…" })).toBeNull()
    cleanup()
    queryClient.clear()

    state.allow = true
    render(<PluginsPage />, { wrapper })
    expect(await screen.findByRole("button", { name: "Load a folder…" })).toBeTruthy()
  })

  it("filters plugins by search text and filter tabs", async () => {
    state.plugins = [
      pluginInfo({ id: "dev.a.alpha", name: "Alpha Tool", author: "Alice", description: "Video helper", enabled: true, gpu: true }),
      pluginInfo({ id: "dev.b.beta", name: "Beta Script", author: "Bob", description: "Audio cleaner", enabled: false, gpu: false }),
    ]
    render(<PluginsPage />, { wrapper })
    expect(await screen.findByRole("group", { name: "Alpha Tool" })).toBeTruthy()
    expect(screen.getByRole("group", { name: "Beta Script" })).toBeTruthy()

    // Search query
    const input = screen.getByPlaceholderText("Search plugins…")
    fireEvent.change(input, { target: { value: "alpha" } })
    expect(screen.getByRole("group", { name: "Alpha Tool" })).toBeTruthy()
    expect(screen.queryByRole("group", { name: "Beta Script" })).toBeNull()

    // Clear search
    fireEvent.change(input, { target: { value: "" } })
    expect(screen.getByRole("group", { name: "Alpha Tool" })).toBeTruthy()
    expect(screen.getByRole("group", { name: "Beta Script" })).toBeTruthy()

    // Filter tab: Disabled
    fireEvent.click(screen.getByRole("button", { name: /Disabled/ }))
    expect(screen.queryByRole("group", { name: "Alpha Tool" })).toBeNull()
    expect(screen.getByRole("group", { name: "Beta Script" })).toBeTruthy()

    // Filter tab: GPU
    fireEvent.click(screen.getByRole("button", { name: /GPU/ }))
    expect(screen.getByRole("group", { name: "Alpha Tool" })).toBeTruthy()
    expect(screen.queryByRole("group", { name: "Beta Script" })).toBeNull()
  })

  it("opens plugin folder and log via ⋯ menu items", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.alpha", name: "Alpha Tool", folder: "/path/to/folder", logPath: "/path/to/log.txt" })]
    render(<PluginsPage />, { wrapper })
    const trigger = await screen.findByRole("button", { name: "Alpha Tool options" })
    await user.click(trigger)
    await user.click(await screen.findByRole("menuitem", { name: "Show its folder" }))
    expect(reveal).toHaveBeenCalledWith("/path/to/folder")

    await user.click(trigger)
    await user.click(await screen.findByRole("menuitem", { name: "Show its log" }))
    expect(reveal).toHaveBeenCalledWith("/path/to/log.txt")
  })

  it("enables and disables all plugins via bulk action buttons", async () => {
    state.plugins = [
      pluginInfo({ id: "dev.a.alpha", name: "Alpha", enabled: false }),
      pluginInfo({ id: "dev.b.beta", name: "Beta", enabled: true }),
    ]
    vi.mocked(commands.pluginSetEnabled).mockImplementation((id, on) =>
      Promise.resolve(pluginInfo({ id, name: id, enabled: on })),
    )
    render(<PluginsPage />, { wrapper })

    // Enable all
    fireEvent.click(await screen.findByRole("button", { name: "Enable all" }))
    await waitFor(() => expect(commands.pluginSetEnabled).toHaveBeenCalledWith("dev.a.alpha", true))

    // Disable all
    fireEvent.click(screen.getByRole("button", { name: "Disable all" }))
    await waitFor(() => expect(commands.pluginSetEnabled).toHaveBeenCalledWith("dev.b.beta", false))
  })

  it("navigates to settings on clicking a card with settings page", async () => {
    state.plugins = [
      pluginInfo({
        id: "dev.a.alpha",
        name: "Alpha Tool",
        settings: [{ key: "opt", title: "Option", description: "Test desc", kind: { type: "boolean", default: false } }],
      }),
    ]
    render(<PluginsPage />, { wrapper })
    const card = await screen.findByRole("group", { name: "Alpha Tool" })
    expect(card.className).toContain("cursor-pointer")
  })

  it("keeps a banner's place while it loads, and draws neither trust nor facts on it", async () => {
    const art = "data:image/jpeg;base64,/9j/4AAQ"
    let answer: (url: string | null) => void = () => {}
    vi.mocked(commands.pluginBanner).mockReturnValue(new Promise((done) => (answer = done)))
    state.plugins = [pluginInfo({ id: "dev.a.art", name: "Art", trust: "unreviewed", author: "someone", bannerHash: "a1" })]
    render(<PluginsPage />, { wrapper })
    const card = await screen.findByRole("group", { name: "Art" })
    const slot = card.firstElementChild!
    expect(slot.querySelector("img")).toBeNull()
    const facts = [within(card).getByText("Not reviewed"), within(card).getByText("Says it is by someone"), within(card).getByText("v1.0.0")]
    act(() => answer(art))
    await waitFor(() => expect(slot.querySelector("img")?.getAttribute("src")).toBe(art))
    // The same place, and nothing the host vouches for on the plugin's pixels.
    expect(card.firstElementChild).toBe(slot)
    for (const fact of facts) expect(slot.contains(fact)).toBe(false)
    expect(slot.textContent).toBe("")
  })

  // A guard for good: a picture fetched from a link would tell its host who
  // looked, and when, and could make a claim look checked.
  it("no picture on the plugin screens comes from the internet", async () => {
    const art = "data:image/jpeg;base64,/9j/4AAQ"
    // Its plugin.json may name its author's GitHub page: only its own files show.
    state.plugins = [
      pluginInfo({
        id: "dev.a.art",
        name: "Art",
        author: "someone",
        repository: "https://github.com/someone/art",
        icon: "data:image/svg+xml;base64,PHN2Zy8+",
        bannerHash: "a1",
        settings: [{ key: "on", title: "On", description: null, kind: { type: "boolean", default: true } }],
      }),
      // A banner answered with a link isn't shown.
      pluginInfo({ id: "dev.a.link", name: "Link", author: "someone", repository: "https://github.com/someone/link", bannerHash: "b2" }),
    ]
    vi.mocked(commands.pluginBanner).mockImplementation((id) =>
      Promise.resolve(id === "dev.a.art" ? art : "https://github.com/someone.png"),
    )
    const { container } = render(
      <>
        <PluginsPage />
        <PluginSettingsSection id="dev.a.art" />
      </>,
      { wrapper },
    )
    // The banner on its card and in its settings, beside the icons.
    await waitFor(() => expect(container.querySelectorAll(`img[src="${art}"]`)).toHaveLength(2))
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
    const page = render(<PluginsPage />, { wrapper })
    await user.click(await screen.findByRole("button", { name: "Mine options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Show the source it names" }))
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledWith("dev.a.mine"))
    page.unmount()
    // Its settings say the same, in their ⋯ (the card's list without
    // Settings and Uninstall) and in their right-click.
    const settings = render(
      <AppMenu>
        <div>
          <PluginSettingsSection id="dev.a.mine" />
        </div>
      </AppMenu>,
      { wrapper },
    )
    await user.click(await screen.findByRole("button", { name: "Mine options" }))
    const items = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
    expect(items).toEqual(["Show the source it names", "Show its folder", "Show its log"])
    await user.click(screen.getByRole("menuitem", { name: "Show the source it names" }))
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledTimes(2))
    fireEvent.contextMenu(screen.getByRole("heading", { name: "Mine" }), { clientX: 40, clientY: 40 })
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
    render(<PluginsPage />, { wrapper })
    const show = async () => {
      await user.click(await screen.findByRole("button", { name: "Mine options" }))
      await user.click(await screen.findByRole("menuitem", { name: "Show its source" }))
    }
    vi.mocked(commands.pluginOpenSource).mockResolvedValueOnce(false)
    await show()
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledTimes(1))
    await act(() => new Promise((done) => setTimeout(done, 0)))
    expect(error).not.toHaveBeenCalled()
    vi.mocked(commands.pluginOpenSource).mockRejectedValueOnce({ code: "io", message: "AutoScene couldn't open the link." })
    await show()
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
    render(<PluginsPage />, { wrapper })
    const entries = async (name: string) => {
      await user.click(await screen.findByRole("button", { name: `${name} options` }))
      const items = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
      await user.keyboard("{Escape}")
      await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())
      return items
    }
    expect(await entries("Reviewed")).toEqual(["Show its source", "Show its folder", "Show its log", "Uninstall…"])
    expect((await entries("Pinned"))[0]).toBe("Show the source it names")
    expect((await entries("Folder"))[0]).toBe("Show the source it names")
    await user.click(await screen.findByRole("button", { name: "Reviewed options" }))
    await user.click(await screen.findByRole("menuitem", { name: "Show its source" }))
    await waitFor(() => expect(commands.pluginOpenSource).toHaveBeenCalledWith("dev.a.reviewed"))
    expect(openExternalUrl).not.toHaveBeenCalled()
  })

  it("no source entry without a repository", async () => {
    const user = userEvent.setup()
    state.plugins = [pluginInfo({ id: "dev.a.plain", name: "Plain", trust: "unreviewed" })]
    const page = render(<PluginsPage />, { wrapper })
    await user.click(await screen.findByRole("button", { name: "Plain options" }))
    const items = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
    expect(items).toEqual(["Show its folder", "Show its log", "Uninstall…"])
    page.unmount()
    // Nor does the ⋯ of its settings.
    render(<PluginSettingsSection id="dev.a.plain" />, { wrapper })
    await user.click(await screen.findByRole("button", { name: "Plain options" }))
    const ownItems = within(await screen.findByRole("menu")).getAllByRole("menuitem").map((i) => i.textContent?.trim())
    expect(ownItems).toEqual(["Show its folder", "Show its log"])
    expect(screen.queryByText(/source/)).toBeNull()
    expect(commands.pluginOpenSource).not.toHaveBeenCalled()
  })
})
