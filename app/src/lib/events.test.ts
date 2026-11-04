import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import type { ExportItem, ExportSummary, JobInfo } from "@/bindings"
import { useActivity } from "@/state/activity"
import { usePluginReview } from "@/state/plugin-review"
import { rememberPackSave, takePackSave } from "@/features/plugins/pack-saves"
import { startEventBridge } from "./events"
import { commands } from "./ipc"
import { isMadePack, qk, queryClient } from "./queries"
import { pluginInfo } from "@/test/plugins"

// Each backend event's listener, as the bridge registers it.
const { handlers } = vi.hoisted(() => ({ handlers: {} as Record<string, (e: { payload: unknown }) => void> }))

vi.mock("./ipc", () => ({
  events: new Proxy(
    {},
    {
      get: (_, name: string) => ({
        listen: (h: (e: { payload: unknown }) => void) => {
          handlers[name] = h
          return Promise.resolve(() => {})
        },
      }),
    },
  ),
  commands: {
    jobsList: vi.fn(() => Promise.resolve([])),
    exportsList: vi.fn(),
    revealPath: vi.fn(() => Promise.resolve(null)),
    pluginsList: vi.fn(),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { success: vi.fn(), error: vi.fn() }) }))

const job = (o: Partial<JobInfo>): JobInfo => ({
  id: "j1",
  kind: "export",
  title: "Exporting 2 clips",
  subtitle: "Breaking Bad",
  projectId: "p",
  mediaId: null,
  exportId: "e1",
  state: "running",
  progress: null,
  stage: null,
  etaMs: null,
  error: null,
  createdAt: 1,
  finishedAt: null,
  rev: 1,
  ...o,
})

const clip = (path: string) => ({ id: 1, mediaId: "m", start: 0, end: 24, name: "clip", status: "done", path, verified: true }) as ExportItem
const exported = (o: Partial<ExportSummary>) =>
  ({ id: "e1", outDir: "C:\\Exports\\Show", items: [clip("a.mov"), clip("b.mov")], merged: null, ...o }) as ExportSummary

/** The job finishing, and the toast that says so. */
async function finish(list: ExportSummary[]) {
  vi.mocked(commands.exportsList).mockResolvedValue(list)
  handlers.jobChanged({ payload: job({ state: "done", rev: 2 }) })
  await vi.waitFor(() => expect(toast.success).toHaveBeenCalledOnce())
  return vi.mocked(toast.success).mock.calls[0][1] as { action?: { label: string; onClick: () => void } }
}

const goPlugins = vi.fn()

let stop: () => void
beforeEach(() => {
  useActivity.setState({ jobs: {}, order: [] })
  stop = startEventBridge({ goPlugins })
  handlers.jobChanged({ payload: job({}) })
})
afterEach(() => {
  stop()
  queryClient.clear()
  vi.clearAllMocks()
})

describe("an export finishing", () => {
  it("offers to show the folder of a file per clip", async () => {
    const { action } = await finish([exported({})])
    expect(action?.label).toBe("Show in folder")
    action?.onClick()
    expect(commands.revealPath).toHaveBeenCalledWith("C:\\Exports\\Show")
  })

  it("shows the one file the clips were joined into", async () => {
    const merged = { name: "Pack", path: "C:\\Exports\\Show\\Pack.mov", verified: true, bytes: 1, error: null }
    const { action } = await finish([exported({ merged })])
    action?.onClick()
    expect(commands.revealPath).toHaveBeenCalledWith("C:\\Exports\\Show\\Pack.mov")
  })

  it("shows a single clip itself", async () => {
    const { action } = await finish([exported({ items: [clip("C:\\Exports\\Show\\clip.mov")] })])
    action?.onClick()
    expect(commands.revealPath).toHaveBeenCalledWith("C:\\Exports\\Show\\clip.mov")
  })

  it("says it finished without the button when the export can't be found", async () => {
    const { action } = await finish([])
    expect(action).toBeUndefined()
  })
})

describe("an export's job ending", () => {
  it("reads the project's exports again, a cancel too", () => {
    const read = vi.spyOn(queryClient, "invalidateQueries")
    handlers.jobProgress({ payload: { id: "j1", rev: 2, progress: 0.5, stage: null, etaMs: null } })
    expect(read).not.toHaveBeenCalled()
    handlers.jobChanged({ payload: job({ state: "cancelled", rev: 3 }) })
    expect(read).toHaveBeenCalledWith({ queryKey: ["exports", "p"] })
    expect(toast.success).not.toHaveBeenCalled()
    read.mockRestore()
  })
})

describe("a plugin job that finishes", () => {
  const plugin = (o: Partial<JobInfo>) =>
    job({ id: "j2", kind: "plugin", title: "Post to Discord", subtitle: "Discord notify", exportId: null, rev: 2, ...o })

  it("says its message", () => {
    handlers.jobChanged({ payload: plugin({ state: "running" }) })
    handlers.jobChanged({ payload: plugin({ state: "done", message: "Posted 12 clips.", rev: 3 }) })
    // Named by its plugin, after a word the plugin can't choose: its words
    // never read as AutoScene's, whatever it calls itself.
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “Discord notify”: Post to Discord", { description: "Posted 12 clips." })
    expect(toast.success).not.toHaveBeenCalled()
  })

  it("says it failed, and which plugin it was", () => {
    handlers.jobChanged({ payload: plugin({ state: "failed", error: "Discord refused the post.", rev: 3 }) })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin “Discord notify”: Post to Discord failed", {
      description: "Discord refused the post.",
    })
  })

  it("reads as a plugin's even when the plugin calls itself AutoScene", () => {
    handlers.jobChanged({ payload: plugin({ state: "done", title: "Update ready", subtitle: "AutoScene", message: "Restart now.", rev: 3 }) })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “AutoScene”: Update ready", { description: "Restart now." })
  })

  it("a pack saved with a plugin says which file, and shows it in its folder", () => {
    rememberPackSave("j2", "C:\\Cuts\\Walter.fcpxml")
    handlers.jobChanged({ payload: plugin({ state: "running" }) })
    // Not yet: the run is not over.
    expect(toast).not.toHaveBeenCalled()
    handlers.jobChanged({
      payload: plugin({ state: "done", title: "Save as FCPXML · Walter", subtitle: "FCPXML", message: "Saved Walter.fcpxml", rev: 3 }),
    })
    expect(toast).toHaveBeenCalledOnce()
    const [text, opts] = vi.mocked(toast).mock.calls[0] as unknown as [string, { action?: { label: string; onClick: () => void } }]
    expect(text).toBe("Plugin “FCPXML”: saved Walter.fcpxml")
    expect(opts.action?.label).toBe("Show in folder")
    opts.action?.onClick()
    expect(commands.revealPath).toHaveBeenCalledWith("C:\\Cuts\\Walter.fcpxml")
  })

  it("a pack save that failed says so, and forgets where the file would have gone", () => {
    rememberPackSave("j2", "C:\\Cuts\\Walter.fcpxml")
    handlers.jobChanged({ payload: plugin({ state: "failed", title: "Save as FCPXML · Walter", subtitle: "FCPXML", error: "No timeline.", rev: 3 }) })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin “FCPXML”: Save as FCPXML · Walter failed", { description: "No timeline." })
    expect(takePackSave("j2")).toBeUndefined()
  })

  it("a save started before a restart says the job's own words", () => {
    handlers.jobChanged({ payload: plugin({ state: "done", title: "Save as FCPXML · Walter", subtitle: "FCPXML", message: "Saved Walter.fcpxml", rev: 3 }) })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “FCPXML”: Save as FCPXML · Walter", { description: "Saved Walter.fcpxml" })
  })

  it("says nothing when it has nothing to say", () => {
    handlers.jobChanged({ payload: plugin({ state: "done", message: null }) })
    expect(toast).not.toHaveBeenCalled()
    expect(toast.success).not.toHaveBeenCalled()
  })
})

describe("plugins changing", () => {
  it("plugins:changed refetches the plugins", () => {
    const read = vi.spyOn(queryClient, "invalidateQueries")
    handlers.pluginsChanged({ payload: { pluginId: "dev.a.one" } })
    handlers.pluginsChanged({ payload: { pluginId: null } })
    // The list, and by prefix every plugin's settings, and what the catalog
    // offers against them; the packs' previews (they show the plugins'
    // filters, and their sizes follow); the settings too when whether
    // unreviewed plugins may run changed.
    expect(read.mock.calls).toEqual([
      [{ queryKey: ["plugins"] }],
      [{ queryKey: ["catalog"] }],
      [{ predicate: isMadePack }],
      [{ queryKey: ["plugins"] }],
      [{ queryKey: ["catalog"] }],
      [{ predicate: isMadePack }],
      [{ queryKey: ["settings"] }],
    ])
    read.mockRestore()
  })

  it("catalog:changed refetches the catalog and the plugins", () => {
    const read = vi.spyOn(queryClient, "invalidateQueries")
    handlers.catalogChanged({ payload: { withdrawn: [] } })
    expect(read.mock.calls).toEqual([[{ queryKey: ["catalog"] }], [{ queryKey: ["plugins"] }]])
    read.mockRestore()
  })

  it("a newly withdrawn plugin toasts once", async () => {
    const leaky = pluginInfo({ id: "dev.example.leaky", name: "Leaky Export", withdrawn: "It sent export paths to a server.", active: false })
    vi.mocked(commands.pluginsList).mockResolvedValue([leaky, pluginInfo({ id: "dev.a.fine", name: "Fine" })])
    handlers.catalogChanged({ payload: { withdrawn: ["dev.example.leaky"] } })
    await vi.waitFor(() => expect(toast.error).toHaveBeenCalledOnce())
    // AutoScene's words: the plugin's name between quotes, the catalog's reason under it.
    const [title, options] = vi.mocked(toast.error).mock.calls[0] as unknown as [string, { description: string; action: { label: string; onClick: () => void } }]
    expect(title).toBe("AutoScene withdrew Plugin “Leaky Export”")
    expect(options.description).toBe("It sent export paths to a server.")
    expect(options.action.label).toBe("Show")
    options.action.onClick()
    expect(goPlugins).toHaveBeenCalledOnce()

    // The next checks name it again: once per session is enough.
    handlers.catalogChanged({ payload: { withdrawn: ["dev.example.leaky"] } })
    handlers.catalogChanged({ payload: { withdrawn: [] } })
    await new Promise((done) => setTimeout(done, 20))
    expect(toast.error).toHaveBeenCalledOnce()
  })

  it("a check that names one told of and one new toasts only the new one", async () => {
    const withdrawn = (id: string, name: string) => pluginInfo({ id, name, withdrawn: "It was unsafe.", active: false })
    vi.mocked(commands.pluginsList).mockResolvedValue([withdrawn("dev.a.first", "First"), withdrawn("dev.a.second", "Second")])
    handlers.catalogChanged({ payload: { withdrawn: ["dev.a.first"] } })
    await vi.waitFor(() => expect(toast.error).toHaveBeenCalledOnce())
    handlers.catalogChanged({ payload: { withdrawn: ["dev.a.first", "dev.a.second"] } })
    await vi.waitFor(() => expect(toast.error).toHaveBeenCalledTimes(2))
    expect(vi.mocked(toast.error).mock.calls.map(([title]) => title)).toEqual([
      "AutoScene withdrew Plugin “First”",
      "AutoScene withdrew Plugin “Second”",
    ])
  })

  it("a withdrawal the plugins don't show gets no toast", async () => {
    // Uninstalled meanwhile, or not withdrawn as the list has it now.
    vi.mocked(commands.pluginsList).mockResolvedValue([pluginInfo({ id: "dev.a.fine", name: "Fine" })])
    handlers.catalogChanged({ payload: { withdrawn: ["dev.a.gone", "dev.a.fine"] } })
    await vi.waitFor(() => expect(commands.pluginsList).toHaveBeenCalled())
    await new Promise((done) => setTimeout(done, 20))
    expect(toast.error).not.toHaveBeenCalled()

    // Nor when the list can't be read.
    vi.mocked(commands.pluginsList).mockRejectedValue(new Error("no"))
    handlers.catalogChanged({ payload: { withdrawn: ["dev.a.fine"] } })
    await new Promise((done) => setTimeout(done, 20))
    expect(toast.error).not.toHaveBeenCalled()
  })

  it("catalog:progress moves the download the review waits for, and no other", () => {
    usePluginReview.setState({
      getting: { pluginId: "dev.a.one", label: "One", done: null, total: null, installing: false },
    })
    handlers.catalogProgress({ payload: { pluginId: "dev.a.two", done: 5, total: 10 } })
    expect(usePluginReview.getState().getting?.done).toBeNull()
    handlers.catalogProgress({ payload: { pluginId: "dev.a.one", done: 5, total: 10 } })
    expect(usePluginReview.getState().getting).toMatchObject({ done: 5, total: 10 })
    usePluginReview.setState({ getting: null })
  })

  it("plugins:changed makes every pack again, its preview and its plan, and keeps the settings it was made with", () => {
    // A pack on screen and one not: both show filters of plugins that changed.
    const [preview, other, plan, state] = [
      [...qk.packPreviews("p", "c1"), { solo: true }],
      [...qk.packPreviews("q", "c2"), { solo: false }],
      [...qk.packPlans("p", "c1"), { solo: true }, { folder: true }],
      [...qk.pack("p", "c1"), "state"],
    ]
    for (const key of [preview, other, plan, state]) queryClient.setQueryData(key, {})
    handlers.pluginsChanged({ payload: { pluginId: "dev.a.one" } })
    const stale = (key: unknown[]) => queryClient.getQueryState(key)?.isInvalidated
    expect([preview, other, plan].map(stale)).toEqual([true, true, true])
    expect(stale(state)).toBe(false)
  })
})
