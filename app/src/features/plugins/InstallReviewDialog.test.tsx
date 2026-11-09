import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { PluginInfo, PluginReview, PluginUpdate } from "@/bindings"
import { commands } from "@/lib/ipc"
import { usePluginReview } from "@/state/plugin-review"
import { inLanguage } from "@/test/i18n"
import { catalogReview, plain, pluginInfo } from "@/test/plugins"
import { InstallReviewDialog } from "./InstallReviewDialog"

vi.mock("@/lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/ipc")>()),
  commands: {
    pluginInstall: vi.fn(() => Promise.resolve(pluginInfo({ id: "dev.example.discord-notify", name: "Discord Notify" }))),
    pluginDiscard: vi.fn(() => Promise.resolve(null)),
    pluginCompileCancel: vi.fn(() => Promise.resolve(null)),
    catalogGet: vi.fn(),
    catalogCancel: vi.fn(() => Promise.resolve(null)),
    pluginsAllowUnreviewed: vi.fn((on: boolean) =>
      Promise.resolve({ libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng", allowUnreviewedPlugins: on }),
    ),
  },
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

const discord = pluginInfo({
  id: "dev.example.discord-notify",
  name: "Discord Notify",
  version: "0.3.1",
  author: "Example Dev",
  trust: "unreviewed",
  permissions: ["exports:read", "network"],
  networkHosts: ["discord.com"],
  exportActions: [{ id: "post", title: "Post to Discord", description: null, afterExport: false, afterFailed: false }],
})

const review = (o: Partial<PluginReview> = {}): PluginReview => ({
  ticketId: "t1",
  plugin: discord,
  update: null,
  needsUnreviewed: true,
  allowed: true,
  uvMissing: false,
  uvBytes: 15236317,
  resourceBytes: 0,
  resourceHosts: [],
  catalog: null,
  componentWarnings: [],
  ...o,
})

/** The review as the shell shows it: from the store. `onClose` is called when the store lets it go. */
function show(r: PluginReview) {
  const onClose = vi.fn()
  stops.push(usePluginReview.subscribe((s, before) => before.review && !s.review && onClose()))
  usePluginReview.setState({ review: r })
  render(<InstallReviewDialog />)
  return onClose
}

const stops: Array<() => void> = []

const install = () => screen.getByRole("button", { name: "Install" }) as HTMLButtonElement

beforeEach(() => {
  vi.clearAllMocks()
  usePluginReview.setState({ review: null, loading: false, queue: [], getting: null, compiling: null })
})
afterEach(() => {
  cleanup()
  stops.splice(0).forEach((stop) => stop())
})

describe("the install review", () => {
  it("needs the name typed for an unreviewed plugin", async () => {
    const onClose = show(review())
    expect(screen.getByText("Not reviewed")).toBeTruthy()
    expect(install().disabled).toBe(true)
    const name = screen.getByLabelText("Type Discord Notify to install it")
    fireEvent.change(name, { target: { value: "discord notify" } })
    expect(install().disabled).toBe(true)
    fireEvent.change(name, { target: { value: " Discord Notify " } })
    expect(install().disabled).toBe(false)
    fireEvent.click(install())
    await waitFor(() => expect(commands.pluginInstall).toHaveBeenCalledExactlyOnceWith("t1", true))
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce())
    expect(toast).toHaveBeenCalledWith("Installed Discord Notify")
    // Installed: the review isn't dropped.
    expect(commands.pluginDiscard).not.toHaveBeenCalled()
    cleanup()

    // The setting off: it is turned on from here, and the review stays.
    show(review({ allowed: false }))
    expect(screen.queryByRole("textbox")).toBeNull()
    expect(install().disabled).toBe(true)
    vi.mocked(commands.pluginsAllowUnreviewed).mockImplementationOnce((on) =>
      Promise.resolve({ libraryRoot: "C:\\Library", exportPresetId: "prores-hq", audioLanguage: "eng", allowUnreviewedPlugins: !on }),
    )
    // The person said no in AutoScene's own dialog: still off.
    fireEvent.click(screen.getByRole("button", { name: "Allow them…" }))
    await waitFor(() => expect(commands.pluginsAllowUnreviewed).toHaveBeenCalledExactlyOnceWith(true))
    expect(screen.queryByRole("textbox")).toBeNull()
    fireEvent.click(screen.getByRole("button", { name: "Allow them…" }))
    fireEvent.change(await screen.findByLabelText("Type Discord Notify to install it"), { target: { value: "Discord Notify" } })
    expect(install().disabled).toBe(false)
    expect(commands.pluginDiscard).not.toHaveBeenCalled()
    cleanup()

    // A reviewed one installs at once, without the setting's yes.
    show(review({ plugin: { ...discord, trust: "reviewed" }, needsUnreviewed: false, allowed: true }))
    expect(screen.queryByRole("textbox")).toBeNull()
    fireEvent.click(install())
    await waitFor(() => expect(commands.pluginInstall).toHaveBeenLastCalledWith("t1", false))
  })

  it("says permissions are declared, not enforced", () => {
    show(review({ plugin: { ...discord, description: "Posts each export to Discord." } }))
    // What counts first, as plainly as the rest.
    expect(screen.getByText("It runs with your user rights: it can read, change and send anything you can.")).toBeTruthy()
    expect(screen.getByRole("heading", { name: "What it says it does (not enforced)" })).toBeTruthy()
    expect(screen.getByText("Reads the exported clips")).toBeTruthy()
    expect(screen.getByText("Talks to the internet")).toBeTruthy()
    expect(screen.getByText("discord.com")).toBeTruthy()
    // Who it is, as it says, and what it adds; its description as its own words, not the dialog's.
    expect(screen.getByText("dev.example.discord-notify")).toBeTruthy()
    expect(screen.getByText("0.3.1")).toBeTruthy()
    expect(screen.getByText("Says it is by")).toBeTruthy()
    expect(screen.getByText("Example Dev")).toBeTruthy()
    expect(screen.getByRole("heading", { name: "In its own words" })).toBeTruthy()
    expect(screen.getByRole("dialog").getAttribute("aria-describedby")).not.toBe(screen.getByText("Posts each export to Discord.").id)
    expect(screen.getByText("Post to Discord")).toBeTruthy()
    expect(screen.queryByText(/graphics card/)).toBeNull()
    cleanup()

    show(review({ plugin: { ...discord, permissions: [], networkHosts: [] } }))
    expect(screen.getByText("It names no permissions.")).toBeTruthy()
    cleanup()

    // The GPU, and what its plugin.json had skipped.
    show(review({ plugin: { ...discord, gpu: true, warnings: [plain("Skipped its page: AutoScene 1.0 has no plugin pages.")] } }))
    expect(screen.getByText("Uses the graphics card: its runs wait while an analysis runs")).toBeTruthy()
    expect(screen.getByText("Skipped its page: AutoScene 1.0 has no plugin pages.")).toBeTruthy()
  })

  it("loads a folder where it is", () => {
    const folder = String.raw`D:\Plugins\discord-notify`
    show(review({ plugin: { ...discord, source: "folder", folder } }))
    expect(screen.getByRole("heading", { name: "Load Discord Notify?" })).toBeTruthy()
    expect(screen.getByText("Loaded from a folder")).toBeTruthy()
    expect(screen.getByText(folder)).toBeTruthy()
    expect(screen.getByLabelText("Type Discord Notify to load it")).toBeTruthy()
    cleanup()

    // A package's folder is AutoScene's own: not shown.
    show(review())
    expect(screen.queryByText(discord.folder)).toBeNull()
  })

  it("says what the installed version was when that changes", () => {
    const update: PluginUpdate = {
      fromVersion: "0.2.0",
      trustBefore: "reviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      raisedLimits: false,
      addedResourceHosts: [],
    }
    show(review({ update }))
    expect(screen.getByText("The installed version: Reviewed by AutoScene.")).toBeTruthy()
    cleanup()

    // A package in place of a developer's folder.
    show(review({ update: { ...update, trustBefore: "unreviewed", sourceBefore: "folder" } }))
    expect(
      screen.getByText("The installed version: Loaded from a folder. This package takes its place; that folder stays as it is."),
    ).toBeTruthy()
    cleanup()

    // Nothing to say when it stays as it was.
    show(review({ update: { ...update, trustBefore: "unreviewed" } }))
    expect(screen.queryByText(/The installed version/)).toBeNull()
  })

  it("takes a long name typed without its ellipsis", () => {
    const name = "A plugin whose name is far too long to …"
    show(review({ plugin: { ...discord, name } }))
    const field = screen.getByRole("textbox")
    for (const typed of ["A plugin whose name is far too long to", "A plugin whose name is far too long to...", name]) {
      fireEvent.change(field, { target: { value: typed } })
      expect(install().disabled, typed).toBe(false)
    }
    fireEvent.change(field, { target: { value: "A plugin whose name" } })
    expect(install().disabled).toBe(true)
    cleanup()

    // Typing nothing never names a plugin, whatever its name.
    show(review({ plugin: { ...discord, name: "..." } }))
    fireEvent.change(screen.getByRole("textbox"), { target: { value: " " } })
    expect(install().disabled).toBe(true)
  })

  it("shows the permissions an update adds", () => {
    const signed = { ...discord, trust: "pinned" as const, authorKeyId: "4B2D9C1E0F3A7E51", networkHosts: ["discord.com", "cdn.example.com"] }
    show(
      review({
        plugin: signed,
        update: {
          fromVersion: "0.2.0",
          trustBefore: "pinned",
          sourceBefore: "package",
          addedPermissions: ["network"],
          addedHosts: ["cdn.example.com"],
          addedPages: [],
          addedShotFilters: [],
          addedPackActions: [],
          addedSources: [],
          addedLinkHosts: [],
          startsProgram: false,
          becomesService: false,
          leavesSandbox: false,
          raisedLimits: false,
          addedResourceHosts: [],
        },
      }),
    )
    expect(screen.getByRole("heading", { name: "Update Discord Notify?" })).toBeTruthy()
    expect(screen.getByText(/0\.2\.0 is installed/)).toBeTruthy()
    expect(screen.getByText("4B2D9C1E0F3A7E51")).toBeTruthy()
    expect(screen.getByText("Signed, not reviewed")).toBeTruthy()
    expect(screen.getByText("Signed with the same key as the installed version. AutoScene has not reviewed it.")).toBeTruthy()
    const network = screen.getByText("Talks to the internet").closest("li")!
    expect(within(network).getByText("New")).toBeTruthy()
    expect(within(network).getByText("New: cdn.example.com")).toBeTruthy()
    expect(within(network).getByText("discord.com")).toBeTruthy()
    // What the installed version could do already isn't marked.
    expect(within(screen.getByText("Reads the exported clips").closest("li")!).queryByText("New")).toBeNull()
    expect(screen.getByRole("button", { name: "Update" })).toBeTruthy()
  })

  it("keeps the review when the person says no outside the page", async () => {
    vi.mocked(commands.pluginInstall).mockRejectedValueOnce({ code: "cancelled", message: "It wasn't installed." })
    const onClose = show(review())
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Discord Notify" } })
    fireEvent.click(install())
    await waitFor(() => expect(install().disabled).toBe(false))
    expect(onClose).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
    expect(commands.pluginDiscard).not.toHaveBeenCalled()
  })

  it("says how it runs", () => {
    show(review())
    expect(screen.getByText(/A Python script that uv runs\. On its first run, uv downloads Python/)).toBeTruthy()
    expect(screen.queryByText(/AutoScene gets uv/)).toBeNull()
    cleanup()

    show(review({ plugin: { ...discord, uvScript: false } }))
    expect(screen.getByText("A program in its folder.")).toBeTruthy()
  })

  it("says AutoScene gets uv", () => {
    show(review({ uvMissing: true }))
    const words =
      "A Python script that uv runs. On its first run AutoScene gets uv (15 MB, from github.com/astral-sh/uv), and uv downloads Python (if this computer has none) and the packages the script names."
    expect(screen.getByText(words)).toBeTruthy()
    // Said once, and nobody installs it by hand any more.
    expect(screen.getAllByText(/first run/)).toHaveLength(1)
    expect(screen.queryByText(/docs\.astral\.sh/)).toBeNull()
    cleanup()

    // As AutoScene's other downloads say their size; a service's is its first start.
    show(review({ uvMissing: true, uvBytes: 400_000, plugin: { ...discord, runtime: "service" } }))
    expect(screen.getByText(/^A Python script that uv runs\. On its first start AutoScene gets uv \(391 KB, /)).toBeTruthy()
  })

  it("says a plugin that runs no program is its pages, and when an update starts one", () => {
    const page = (id: string, title: string) => ({
      id,
      title,
      description: null,
      entry: `ui/${id}.html`,
      scope: "global" as const,
      rail: false,
    })
    const pages = { ...discord, runtime: "none" as const, uvScript: false, exportActions: [], pages: [page("main", "Main")] }
    show(review({ plugin: { ...pages, permissions: ["project:read"], networkHosts: [] } }))
    expect(screen.getByText("Runs no program. Its pages run in a sandbox.")).toBeTruthy()
    expect(screen.queryByText(/user rights/)).toBeNull()
    expect(screen.getByRole("heading", { name: "What its pages may use" })).toBeTruthy()
    expect(screen.getByText("AutoScene checks these each time its pages ask.")).toBeTruthy()
    expect(screen.getByText("Main")).toBeTruthy()
    expect(screen.queryByText("New")).toBeNull()
    cleanup()

    // The pages an update adds are marked.
    const update: PluginUpdate = {
      fromVersion: "0.2.0",
      trustBefore: "unreviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: ["clips"],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      raisedLimits: false,
      addedResourceHosts: [],
    }
    // By its id: another page may have the same title.
    show(review({ plugin: { ...pages, pages: [page("main", "Clips"), page("clips", "Clips")] }, update }))
    const [kept, added] = screen.getAllByText("Clips").map((e) => e.closest("li")!)
    expect(within(added).getByText("New")).toBeTruthy()
    expect(within(kept).queryByText("New")).toBeNull()
    cleanup()

    // One the person agreed to as pages only, which now runs a program, says so.
    show(review({ plugin: discord, update: { ...update, addedPages: [], startsProgram: true } }))
    const runs = screen.getByText(/A Python script that uv runs/)
    expect(within(runs).getByText("New")).toBeTruthy()
    expect(screen.getByText("The installed version runs no program.")).toBeTruthy()
    expect(screen.getByText("It runs with your user rights: it can read, change and send anything you can.")).toBeTruthy()
  })

  it("says a service keeps its program running while its page is open, and when an update makes it one", () => {
    const counter = { ...discord, runtime: "service" as const, protocol: 2 }
    show(review({ plugin: counter }))
    expect(screen.getByText(/A Python script that uv runs/)).toBeTruthy()
    const keeps = screen.getByText("Keeps its program running while its page is open.")
    expect(within(keeps).queryByText("New")).toBeNull()
    // Still a program with the person's rights.
    expect(screen.getByText("It runs with your user rights: it can read, change and send anything you can.")).toBeTruthy()
    cleanup()

    // A plugin of a process, now a service: marked as new.
    const update: PluginUpdate = {
      fromVersion: "0.2.0",
      trustBefore: "unreviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: true,
      leavesSandbox: false,
      raisedLimits: false,
      addedResourceHosts: [],
    }
    show(review({ plugin: counter, update }))
    const now = screen.getByText("Now keeps its program running while its page is open.")
    expect(within(now).getByText("New")).toBeTruthy()
    cleanup()

    // A process plugin says nothing of it.
    show(review())
    expect(screen.queryByText(/keeps its program running/i)).toBeNull()
  })

  it("a plugin that downloads files says how much and from where", () => {
    show(review({ resourceBytes: 327_155_712, resourceHosts: ["huggingface.co", "cdn.example.com"] }))
    expect(
      screen.getByText(
        "It downloads 312 MB from huggingface.co and cdn.example.com at its first run. AutoScene keeps the files in its cache and checks each one by its hash.",
      ),
    ).toBeTruthy()
    // A first install has nothing to compare: no host is new.
    expect(screen.queryByText("New")).toBeNull()
    cleanup()

    // A service gets its files at its first start, and a plugin that names none says nothing.
    show(review({ resourceBytes: 5_242_880, resourceHosts: ["huggingface.co"], plugin: { ...discord, runtime: "service" } }))
    expect(screen.getByText(/^It downloads 5 MB from huggingface\.co at its first start\./)).toBeTruthy()
    cleanup()
    show(review())
    expect(screen.queryByText(/It downloads/)).toBeNull()
    expect(screen.queryByText(/by its hash/)).toBeNull()
  })

  it("a new download host is marked New", () => {
    const update: PluginUpdate = {
      fromVersion: "0.2.0",
      trustBefore: "unreviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      raisedLimits: false,
      addedResourceHosts: ["cdn.example.com"],
    }
    show(review({ update, resourceBytes: 1_000_000, resourceHosts: ["huggingface.co", "cdn.example.com"] }))
    // An update's plugin has run: its files come at its next run. The badge marks the host, on the line that names it.
    const line = screen.getByText(/^It downloads 977 KB from huggingface\.co and cdn\.example\.com at its next run\./)
    expect(within(line).queryByText("New")).toBeNull()
    const host = screen.getByText("The installed version names no files from cdn.example.com.")
    expect(within(host).getByText("New")).toBeTruthy()
    cleanup()

    // An update from the hosts the installed version names: nothing is new.
    show(review({ update: { ...update, addedResourceHosts: [] }, resourceBytes: 1_000_000, resourceHosts: ["huggingface.co"] }))
    expect(screen.getByText(/^It downloads 977 KB from huggingface\.co at its next run/)).toBeTruthy()
    expect(screen.queryByText("New")).toBeNull()
    expect(screen.queryByText(/names no files/)).toBeNull()
    cleanup()

    // Files AutoScene keeps already: where they come from, and no download.
    show(review({ update: { ...update, addedResourceHosts: [] }, resourceBytes: 0, resourceHosts: ["huggingface.co"] }))
    expect(screen.getByText(/^The large files it names come from huggingface\.co\. AutoScene already has them/)).toBeTruthy()
    expect(screen.queryByText(/It downloads/)).toBeNull()
  })

  it("names each secret it keeps in the keychain", () => {
    const settings = [
      { key: "webhook", title: "Webhook address", description: null, kind: { type: "secret" as const, placeholder: null } },
      { key: "loud", title: "Loud", description: null, kind: { type: "boolean" as const, default: false } },
    ]
    show(review({ plugin: { ...discord, settings } }))
    expect(screen.getByText("Keeps Webhook address in your system's keychain")).toBeTruthy()
    // A secret isn't counted with the settings.
    expect(screen.getByText("1 setting")).toBeTruthy()
  })

  it("says what may run by itself once the person turns it on", () => {
    const exportActions = [
      { id: "post", title: "Post to Discord", description: null, afterExport: false, afterFailed: true },
      { id: "tag", title: "Tag the clips", description: null, afterExport: false, afterFailed: false },
    ]
    const events = [
      { id: "done", title: "Post when an analysis finishes", description: null, on: "analysisFinished" as const, turnedOn: false },
    ]
    show(review({ plugin: { ...discord, exportActions, events } }))
    const adds = (where: string) =>
      screen.getByText(`${where}:`, { exact: true }).closest("li")!.textContent!.replace(/\s+/g, " ").trim()
    expect(adds("Can run when an analysis finishes")).toBe("Can run when an analysis finishes: Post when an analysis finishes (you turn it on)")
    expect(adds("Can run after every export, also one that failed")).toBe(
      "Can run after every export, also one that failed: Post to Discord (you turn it on)",
    )
    // Each action stays in the export's menu; only the one that says so runs after a failed export.
    expect(screen.getAllByText("Each export's menu:")).toHaveLength(2)
    expect(screen.getAllByText("Can run after every export, also one that failed:")).toHaveLength(1)
  })

  it("names what it adds to a scenepack", () => {
    const filter = { id: "speaks", title: "Speaks", description: null, reason: "No speech", settings: [], revision: 1 }
    const action = { id: "fcpxml", title: "Save as FCPXML…", file: { name: "{character}.fcpxml", extensions: ["fcpxml"] } }
    const plugin = { ...discord, exportActions: [], shotFilters: [filter], packActions: [action] }
    show(review({ plugin }))
    expect(screen.getByText("A scenepack filter:").parentElement?.textContent).toContain("Speaks")
    expect(screen.getByText("A scenepack's ⋯ menu:").parentElement?.textContent).toContain("Save as FCPXML…")
    expect(screen.queryByText("Nothing to list.")).toBeNull()
    expect(screen.queryByText("New")).toBeNull()
  })

  it("marks the scenepack filters and ⋯ entries an update adds", () => {
    const filter = (id: string, title: string) => ({ id, title, description: null, reason: "Left out", settings: [], revision: 1 })
    const action = (id: string, title: string) => ({ id, title, file: { name: "{character}.xml", extensions: ["xml"] } })
    const plugin = {
      ...discord,
      exportActions: [],
      shotFilters: [filter("speaks", "Speaks"), filter("loud", "Loud")],
      packActions: [action("xml", "Save as XML…"), action("edl", "Save as EDL…")],
    }
    const update: PluginUpdate = {
      fromVersion: "0.3.0",
      trustBefore: "unreviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      addedShotFilters: ["loud"],
      addedPackActions: ["edl"],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      raisedLimits: false,
      addedResourceHosts: [],
    }
    show(review({ plugin, update }))
    const row = (name: string) => screen.getByText(name).closest("li")
    expect(within(row("Loud") as HTMLElement).getByText("New")).toBeTruthy()
    expect(within(row("Save as EDL…") as HTMLElement).getByText("New")).toBeTruthy()
    expect(within(row("Speaks") as HTMLElement).queryByText("New")).toBeNull()
    expect(within(row("Save as XML…") as HTMLElement).queryByText("New")).toBeNull()
  })

  it("says so when it adds nothing to list", () => {
    show(review({ plugin: { ...discord, exportActions: [] } }))
    expect(screen.getByText("Nothing to list.")).toBeTruthy()
  })

  it("never says a first signature shows who made it", () => {
    const signed = { ...discord, trust: "pinned" as const, author: "AutoScene", authorKeyId: "4B2D9C1E0F3A7E51" }
    show(review({ plugin: signed }))
    const badge = screen.getByText("Signed, not reviewed")
    expect(badge.className).not.toContain("accent")
    expect(screen.getByText(/Signed with a key that came in the package: nothing shows who holds it\./)).toBeTruthy()
    // The name it gives its author is its own claim.
    expect(screen.getByText("Says it is by")).toBeTruthy()
    expect(screen.queryByText("Author")).toBeNull()
    cleanup()

    // An unsigned plugin's signed update pins whatever key it brings: said plainly.
    const update: PluginUpdate = {
      fromVersion: "0.2.0",
      trustBefore: "unreviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      raisedLimits: false,
      addedResourceHosts: [],
    }
    show(review({ plugin: signed, update }))
    expect(
      screen.getByText("The installed version wasn't signed. This one pins its key: from now on, only updates signed with it install."),
    ).toBeTruthy()
    cleanup()

    // Only a reviewed package's author is AutoScene's word.
    show(review({ plugin: { ...discord, trust: "reviewed" }, needsUnreviewed: false }))
    expect(screen.getByText("Author")).toBeTruthy()
  })

  /** Clip List, a WebAssembly plugin: it may change the export's folder, in AutoScene's sandbox. */
  const clipList = pluginInfo({
    id: "dev.autoscene.clip-list",
    name: "Clip List",
    trust: "unreviewed",
    runtime: "wasm",
    uvScript: false,
    sandbox: { memoryMb: 512, minutes: 30 },
    permissions: ["exports:write"],
    exportActions: [{ id: "list", title: "Write the clip list", description: null, afterExport: false, afterFailed: false }],
  })

  it("says a WebAssembly plugin runs in a sandbox and its permissions are enforced", () => {
    show(review({ plugin: clipList }))
    expect(screen.getByText("It runs in a sandbox. It sees only its own folder and what is listed here.")).toBeTruthy()
    expect(screen.getByRole("heading", { name: "What it may do (enforced)" })).toBeTruthy()
    expect(screen.getByText("Reads, changes and deletes files in the export's folder")).toBeTruthy()
    expect(screen.getByText("It can't start programs or see your other files.")).toBeTruthy()
    expect(screen.getByText("Not limited: how much it writes.")).toBeTruthy()
    expect(screen.getByText("A WebAssembly plugin. AutoScene compiles it once you say yes.")).toBeTruthy()
    // None of what a program's review says, and never "safe".
    expect(screen.queryByText("It runs with your user rights: it can read, change and send anything you can.")).toBeNull()
    expect(screen.queryByRole("heading", { name: /not enforced|says it does/ })).toBeNull()
    expect(screen.queryByText("Changes or adds files beside the exported clips")).toBeNull()
    expect(document.body.textContent?.toLowerCase()).not.toMatch(/\bsafe/)
    cleanup()

    // One that names nothing more says so.
    show(review({ plugin: { ...clipList, permissions: [] } }))
    expect(screen.getByText("It asks for nothing more.")).toBeTruthy()
  })

  it("lists the only sites a sandboxed plugin may reach", () => {
    const update: PluginUpdate = {
      fromVersion: "1.0.0",
      trustBefore: "unreviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: ["hooks.example.com"],
      addedPages: [],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      raisedLimits: false,
      addedResourceHosts: [],
    }
    const posting = { ...clipList, permissions: ["exports:read", "network"] as PluginInfo["permissions"], networkHosts: ["discord.com", "hooks.example.com"] }
    show(review({ plugin: posting, update }))
    const sites = screen.getByText("Talks to these sites, over https only:").closest("li")
    expect(sites).toBeTruthy()
    expect(within(sites as HTMLElement).getByText("discord.com")).toBeTruthy()
    expect(within(sites as HTMLElement).getByText("New: hooks.example.com")).toBeTruthy()
    expect(screen.getByText("Reads the files in the export's folder")).toBeTruthy()
    expect(screen.getByText("Not limited: what it sends to those sites, and how much it writes.")).toBeTruthy()
  })

  it("says a sandboxed plugin that names no site reaches none", () => {
    show(review({ plugin: { ...clipList, permissions: ["network"] } }))
    expect(screen.getByText("May use the network, but names no site: every request is refused.")).toBeTruthy()
    expect(screen.queryByText("Talks to these sites, over https only:")).toBeNull()
    expect(screen.getByText("Not limited: how much it writes.")).toBeTruthy()
    expect(screen.queryByText(/what it sends to those sites/)).toBeNull()
  })

  it("says what it skipped and what it noticed in the UI's language", async () => {
    await inLanguage("ro", { backend: { plugin: { warn: { bannerKind: "Bannerul a rămas pe dinafară.", httpWithoutNetwork: "Cererile lui sunt refuzate." } } } })
    const skipped = { key: "plugin.warn.bannerKind", text: "Left out the banner: it must be a .png, .jpg or .webp file in the plugin." }
    const noticed = { key: "plugin.warn.httpWithoutNetwork", text: "It imports wasi:http but doesn't ask for network: every request it makes is refused." }
    show(review({ plugin: { ...clipList, warnings: [skipped] }, componentWarnings: [noticed] }))
    expect(screen.getByText("Bannerul a rămas pe dinafară.")).toBeTruthy()
    expect(screen.getByText("Cererile lui sunt refuzate.")).toBeTruthy()
    expect(document.body.textContent).not.toContain("[object Object]")
  })

  it("lists what the check of its component noticed apart from what its plugin.json skipped", () => {
    const noticed = "It imports wasi:http but doesn't ask for network: every request it makes is refused."
    show(review({ plugin: clipList, componentWarnings: [plain(noticed)] }))
    const section = screen.getByRole("heading", { name: "Noticed in its component" }).closest("section")
    expect(within(section as HTMLElement).getByText(noticed)).toBeTruthy()
    expect(screen.queryByRole("heading", { name: "Skipped in its plugin.json" })).toBeNull()
    cleanup()
    // Nothing noticed, no heading.
    show(review({ plugin: clipList }))
    expect(screen.queryByRole("heading", { name: "Noticed in its component" })).toBeNull()
  })

  it("shows a sandboxed plugin's limits", () => {
    show(review({ plugin: clipList }))
    expect(screen.getByText("It may use up to 512 MB of memory. A run stops after 30 minutes.")).toBeTruthy()
    cleanup()
    show(review({ plugin: { ...clipList, sandbox: { memoryMb: 64, minutes: 60 } } }))
    expect(screen.getByText("It may use up to 64 MB of memory. A run stops after 1 hour.")).toBeTruthy()
  })

  it("marks a sandboxed plugin's limits when an update raises them, and only then", () => {
    const update: PluginUpdate = {
      fromVersion: "1.0.0",
      trustBefore: "reviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: false,
      raisedLimits: true,
      addedResourceHosts: [],
    }
    const plugin = { ...clipList, permissions: [], sandbox: { memoryMb: 4096, minutes: 1440 } }
    const limits = "It may use up to 4096 MB of memory. A run stops after 24 hours."
    show(review({ plugin, update }))
    expect(within(screen.getByText(limits)).getByText("New")).toBeTruthy()
    cleanup()
    show(review({ plugin, update: { ...update, raisedLimits: false } }))
    expect(within(screen.getByText(limits)).queryByText("New")).toBeNull()
  })

  it("warns when an update leaves the sandbox", () => {
    const update: PluginUpdate = {
      fromVersion: "1.0.0",
      trustBefore: "unreviewed",
      sourceBefore: "package",
      addedPermissions: [],
      addedHosts: [],
      addedPages: [],
      addedShotFilters: [],
      addedPackActions: [],
      addedSources: [],
      addedLinkHosts: [],
      startsProgram: false,
      becomesService: false,
      leavesSandbox: true,
      raisedLimits: false,
      addedResourceHosts: [],
    }
    show(review({ plugin: discord, update }))
    const warning = screen.getByText("The installed version ran in a sandbox. This version runs with your user rights.")
    expect(warning.className).toContain("text-danger")
    // The program's review, word for word.
    expect(screen.getByText("It runs with your user rights: it can read, change and send anything you can.")).toBeTruthy()
    expect(screen.getByRole("heading", { name: "What it says it does (not enforced)" })).toBeTruthy()
    cleanup()
    // An update that stays in it says nothing of the kind.
    show(review({ plugin: clipList, update: { ...update, leavesSandbox: false } }))
    expect(screen.queryByText(/ran in a sandbox/)).toBeNull()
  })

  it("still needs the name typed for an unreviewed sandboxed plugin", async () => {
    show(review({ plugin: clipList }))
    expect(screen.getByText("Not reviewed")).toBeTruthy()
    expect(install().disabled).toBe(true)
    fireEvent.change(screen.getByLabelText("Type Clip List to install it"), { target: { value: "Clip List" } })
    expect(install().disabled).toBe(false)
    fireEvent.click(install())
    await waitFor(() => expect(commands.pluginInstall).toHaveBeenCalledExactlyOnceWith("t1", true))
    cleanup()
    // The setting off: it stays off for a sandboxed plugin too.
    show(review({ plugin: clipList, allowed: false }))
    expect(screen.queryByRole("textbox")).toBeNull()
    expect(install().disabled).toBe(true)
  })

  it("drops the review when it is closed", async () => {
    const onClose = show(review())
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(commands.pluginDiscard).toHaveBeenCalledExactlyOnceWith("t1")
    expect(onClose).toHaveBeenCalledOnce()
    cleanup()

    // A refusal ends the review, but one while the plugin runs keeps it for another try.
    const reviewed = review({ needsUnreviewed: false, plugin: { ...discord, trust: "reviewed" } })
    vi.mocked(commands.pluginInstall).mockRejectedValueOnce({ code: "not_ready", message: "Discord Notify is running." })
    const kept = show(reviewed)
    fireEvent.click(install())
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't install the plugin", { description: "Discord Notify is running." }))
    expect(kept).not.toHaveBeenCalled()
    expect(install().disabled).toBe(false)
    expect(commands.pluginDiscard).toHaveBeenCalledOnce()
    cleanup()

    vi.mocked(commands.pluginInstall).mockRejectedValueOnce({ code: "invalid", message: "This plugin needs another AutoScene." })
    const ended = show(reviewed)
    fireEvent.click(install())
    await waitFor(() => expect(ended).toHaveBeenCalledOnce())
    expect(toast.error).toHaveBeenLastCalledWith("Couldn't install the plugin", { description: "This plugin needs another AutoScene." })
    expect(commands.pluginDiscard).toHaveBeenLastCalledWith("t1")
    expect(commands.pluginDiscard).toHaveBeenCalledTimes(2)
  })

  it("can't be closed while it installs", async () => {
    let done: (p: PluginInfo) => void = () => {}
    vi.mocked(commands.pluginInstall).mockReturnValueOnce(new Promise((resolve) => (done = resolve)))
    const onClose = show(review({ needsUnreviewed: false, plugin: { ...discord, trust: "reviewed" } }))
    fireEvent.click(install())
    await waitFor(() => expect(install().getAttribute("aria-busy")).toBe("true"))
    expect((screen.getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" })
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull()
    expect(screen.getByRole("dialog")).toBeTruthy()
    expect(onClose).not.toHaveBeenCalled()
    // It installs all the same, and then it closes, its review taken.
    done(discord)
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce())
    expect(commands.pluginDiscard).not.toHaveBeenCalled()
    expect(toast).toHaveBeenCalledWith("Installed Discord Notify")
  })

  it("says what it does while a plugin compiles, and Cancel comes back to the review", async () => {
    let fail: (e: unknown) => void = () => {}
    vi.mocked(commands.pluginInstall).mockReturnValueOnce(new Promise((_, reject) => (fail = reject)))
    vi.mocked(commands.pluginCompileCancel).mockImplementationOnce(() => {
      fail({ code: "cancelled", message: "It wasn't installed." })
      return Promise.resolve(null)
    })
    const onClose = show(review())
    const typed = screen.getByRole("textbox")
    fireEvent.change(typed, { target: { value: "Discord Notify" } })
    fireEvent.click(install())
    // The system's dialog may still ask: until the compile starts, the review stays.
    await waitFor(() => expect(install().getAttribute("aria-busy")).toBe("true"))
    act(() => usePluginReview.getState().compileStarted("another ticket"))
    expect(screen.getByRole("dialog", { name: "Install Discord Notify?" })).toBeTruthy()

    act(() => usePluginReview.getState().compileStarted("t1"))
    const dialog = await screen.findByRole("dialog", { name: "Getting Discord Notify ready…" })
    expect(within(dialog).getByRole("progressbar").getAttribute("aria-valuenow")).toBeNull()
    expect(within(dialog).getByText(/This can take up to 2 minutes/)).toBeTruthy()
    expect(within(dialog).queryByRole("button", { name: "Close" })).toBeNull()
    fireEvent.keyDown(dialog, { key: "Escape" })
    expect(onClose).not.toHaveBeenCalled()

    const cancel = within(dialog).getByRole("button", { name: "Cancel" }) as HTMLButtonElement
    fireEvent.click(cancel)
    expect(commands.pluginCompileCancel).toHaveBeenCalledExactlyOnceWith("t1")
    await screen.findByRole("dialog", { name: "Install Discord Notify?" })
    expect(usePluginReview.getState().compiling).toBeNull()
    expect((screen.getByRole("textbox") as HTMLInputElement).value).toBe("Discord Notify")
    expect(install().disabled).toBe(false)
    expect(onClose).not.toHaveBeenCalled()
    expect(commands.pluginDiscard).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("a compile that fails says so, and the review goes", async () => {
    let fail: (e: unknown) => void = () => {}
    vi.mocked(commands.pluginInstall).mockReturnValueOnce(new Promise((_, reject) => (fail = reject)))
    const onClose = show(review({ needsUnreviewed: false, plugin: { ...discord, trust: "reviewed" } }))
    fireEvent.click(install())
    await waitFor(() => expect(install().getAttribute("aria-busy")).toBe("true"))
    act(() => usePluginReview.getState().compileStarted("t1"))
    await screen.findByRole("dialog", { name: "Getting Discord Notify ready…" })
    const why = "This plugin took too long to get ready, so AutoScene stopped. Nothing changed."
    act(() => fail({ code: "invalid", message: why }))
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce())
    expect(toast.error).toHaveBeenCalledWith("Couldn't install the plugin", { description: why })
    expect(usePluginReview.getState().compiling).toBeNull()
  })

  it("an update without a review shows its compile too, and Cancel ends it", async () => {
    render(<InstallReviewDialog />)
    const poster = pluginInfo({ id: "dev.autoscene.poster-maker", name: "Poster Maker" })
    vi.mocked(commands.catalogGet).mockResolvedValue(catalogReview(poster, {}))
    let fail: (e: unknown) => void = () => {}
    vi.mocked(commands.pluginInstall).mockReturnValueOnce(new Promise((_, reject) => (fail = reject)))
    vi.mocked(commands.pluginCompileCancel).mockImplementationOnce(() => {
      fail({ code: "cancelled", message: "It wasn't installed." })
      return Promise.resolve(null)
    })
    const { updatePlugin } = await import("./catalog-actions")
    const done = updatePlugin(poster)
    const dialog = await screen.findByRole("dialog", { name: "Updating Poster Maker…" })
    await waitFor(() => expect(commands.pluginInstall).toHaveBeenCalledOnce())
    expect(within(dialog).getByText("It asks for nothing new, so there is nothing to review.")).toBeTruthy()
    expect((within(dialog).getByRole("button", { name: "Cancel" }) as HTMLButtonElement).disabled).toBe(true)

    const ticket = vi.mocked(commands.pluginInstall).mock.calls[0][0]
    act(() => usePluginReview.getState().compileStarted(ticket))
    const compiling = await screen.findByRole("dialog", { name: "Getting Poster Maker ready…" })
    fireEvent.click(within(compiling).getByRole("button", { name: "Cancel" }))
    expect(await done).toBe(true)
    expect(commands.pluginCompileCancel).toHaveBeenCalledExactlyOnceWith(ticket)
    expect(commands.pluginDiscard).toHaveBeenCalledExactlyOnceWith(ticket)
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    expect(usePluginReview.getState().review).toBeNull()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("says where AutoScene built a catalog plugin from", () => {
    show(catalogReview(pluginInfo({ id: "dev.autoscene.poster-maker", name: "Poster Maker" })))
    expect(screen.getByText(/^Built from/).textContent).toBe(
      "Built from github.com/tcmmi/AutoScene at 4f1c2a9, in examples/plugins/poster-maker. Catalog of 3 November 2026.",
    )
    expect(install().disabled).toBe(false)
    // A package picked from a file says nothing of the kind.
    cleanup()
    show(review())
    expect(screen.queryByText(/^Built from/)).toBeNull()
  })

  it("shows the download and cancels it", async () => {
    render(<InstallReviewDialog />)
    let fail: (e: unknown) => void = () => {}
    vi.mocked(commands.catalogGet).mockReturnValue(new Promise((_, reject) => (fail = reject)))
    vi.mocked(commands.catalogCancel).mockImplementation(() => {
      fail({ code: "cancelled", message: "Cancelled." })
      return Promise.resolve(null)
    })
    const { getFromCatalog } = await import("./catalog-actions")
    const done = getFromCatalog({ id: "dev.autoscene.poster-maker", name: "Poster Maker" })
    const dialog = await screen.findByRole("dialog", { name: "Getting Poster Maker…" })
    expect(within(dialog).getByRole("progressbar").getAttribute("aria-valuenow")).toBeNull()
    // `catalog:progress` moves it.
    act(() => usePluginReview.getState().progress("dev.autoscene.poster-maker", 24_000, 48_000))
    expect(within(dialog).getByRole("progressbar").getAttribute("aria-valuenow")).toBe("50")
    expect(within(dialog).getByText("23.4 KB of 46.9 KB from AutoScene's plugin catalog")).toBeTruthy()

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }))
    await done
    expect(commands.catalogCancel).toHaveBeenCalledWith("dev.autoscene.poster-maker")
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    // Cancelled by the person: nothing went wrong.
    expect(toast.error).not.toHaveBeenCalled()
  })
})
