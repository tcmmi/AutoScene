import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { toast } from "sonner"
import type { ExportItem, ExportSummary, HardwareInfo, JobInfo } from "@/bindings"
import { useActivity } from "@/state/activity"
import { usePluginReview } from "@/state/plugin-review"
import { rememberPackSave, takePackSave } from "@/features/plugins/pack-saves"
import { startEventBridge } from "./events"
import { commands } from "./ipc"
import { isMadePack, qk, queryClient } from "./queries"
import { pluginInfo } from "@/test/plugins"
import { inLanguage, plain } from "@/test/i18n"

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
  title: plain("Exporting 2 clips"),
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

describe("a job that fails", () => {
  const indexing = { key: "job.indexing", args: { name: "S01E01 · Pilot.mkv" }, text: "Indexing S01E01 · Pilot.mkv" }
  const analysing = { key: "job.analysing", args: { name: "Pilot.mkv" }, text: "Analysing Pilot.mkv" }
  const failed = (id: string, kind: JobInfo["kind"], title: JobInfo["title"]) =>
    handlers.jobChanged({ payload: job({ id, kind, title, exportId: null, state: "failed", error: plain("No video."), rev: 3 }) })

  it("says it couldn't, by the key of its title", () => {
    failed("i", "ingest", indexing)
    failed("a", "analyze", analysing)
    expect(vi.mocked(toast.error).mock.calls).toEqual([
      ["Couldn't index S01E01 · Pilot.mkv", { description: "No video." }],
      ["Couldn't analyse Pilot.mkv", { description: "No video." }],
    ])
  })

  it("doesn't read the words of a title: only its key says what it was", () => {
    failed("p", "proxy", plain("Indexing is not what this says"))
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Indexing is not what this says", { description: "No video." })
  })

  it("names other work by its title", () => {
    failed("p", "proxy", { key: "job.preparingPreview", args: { name: "Pilot.mkv" }, text: "Preparing preview of Pilot.mkv" })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Preparing preview of Pilot.mkv", { description: "No video." })
  })

  it("says a name that is a message of its own, and its English when it has none", async () => {
    await inLanguage("ro", { shell: { jobFailed: { indexing: "{{name}} nu a putut fi indexat" } }, backend: { fixture: { episode: "Episodul {{n}}" } } })
    const nested = { key: "fixture.episode", args: { n: 3 }, text: "Episode 3" }
    failed("i", "ingest", { key: "job.indexing", args: { name: nested }, text: "Indexing Episode 3" })
    failed("j", "ingest", { key: "job.indexing", text: "Indexing it" })
    const [[named], [nameless]] = vi.mocked(toast.error).mock.calls
    expect(named).toBe("Episodul 3 nu a putut fi indexat")
    expect(nameless).toBe("Indexing it")
  })

  it("says a title with no name in the language: the upkeep jobs have none", async () => {
    await inLanguage("ro", { backend: { job: { groupingCharacters: "Se grupează personajele" } } })
    failed("g", "regroup", { key: "job.groupingCharacters", text: "Grouping characters" })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Se grupează personajele", { description: "No video." })
  })

  it("says all of it in the language", async () => {
    await inLanguage("ro", {
      shell: { jobFailed: { indexing: "{{name}} nu a putut fi indexat" } },
      backend: { job: { preparingPreview: "Se pregătește previzualizarea pentru {{name}}" }, episode: { fileNotFound: "Fișierul nu e de găsit." } },
    })
    const error = { key: "episode.fileNotFound", text: "That file can't be found." }
    handlers.jobChanged({ payload: job({ id: "i", kind: "ingest", title: indexing, exportId: null, state: "failed", error, rev: 3 }) })
    failed("p", "proxy", { key: "job.preparingPreview", args: { name: "Pilot.mkv" }, text: "Preparing preview of Pilot.mkv" })
    expect(vi.mocked(toast.error).mock.calls).toEqual([
      ["S01E01 · Pilot.mkv nu a putut fi indexat", { description: "Fișierul nu e de găsit." }],
      ["Se pregătește previzualizarea pentru Pilot.mkv", { description: "No video." }],
    ])
  })
})

describe("a plugin job that finishes", () => {
  const plugin = (o: Partial<JobInfo>) =>
    job({ id: "j2", kind: "plugin", title: plain("Post to Discord"), subtitle: "Discord notify", exportId: null, rev: 2, ...o })

  it("says its message", () => {
    handlers.jobChanged({ payload: plugin({ state: "running" }) })
    handlers.jobChanged({ payload: plugin({ state: "done", message: plain("Posted 12 clips."), rev: 3 }) })
    // Named by its plugin, after a word the plugin can't choose: its words
    // never read as AutoScene's, whatever it calls itself.
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “Discord notify”: Post to Discord", { description: "Posted 12 clips." })
    expect(toast.success).not.toHaveBeenCalled()
  })

  it("says it failed, and which plugin it was", () => {
    handlers.jobChanged({ payload: plugin({ state: "failed", error: plain("Discord refused the post."), rev: 3 }) })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin “Discord notify”: Post to Discord failed", {
      description: "Discord refused the post.",
    })
  })

  it("reads as a plugin's even when the plugin calls itself AutoScene", () => {
    handlers.jobChanged({ payload: plugin({ state: "done", title: plain("Update ready"), subtitle: "AutoScene", message: plain("Restart now."), rev: 3 }) })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “AutoScene”: Update ready", { description: "Restart now." })
  })

  it("a pack saved with a plugin says which file, and shows it in its folder", () => {
    rememberPackSave("j2", "C:\\Cuts\\Walter.fcpxml")
    handlers.jobChanged({ payload: plugin({ state: "running" }) })
    // Not yet: the run is not over.
    expect(toast).not.toHaveBeenCalled()
    handlers.jobChanged({
      payload: plugin({ state: "done", title: plain("Save as FCPXML · Walter"), subtitle: "FCPXML", message: plain("Saved Walter.fcpxml"), rev: 3 }),
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
    handlers.jobChanged({ payload: plugin({ state: "failed", title: plain("Save as FCPXML · Walter"), subtitle: "FCPXML", error: plain("No timeline."), rev: 3 }) })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin “FCPXML”: Save as FCPXML · Walter failed", { description: "No timeline." })
    expect(takePackSave("j2")).toBeUndefined()
  })

  it("a save started before a restart says the job's own words", () => {
    handlers.jobChanged({ payload: plugin({ state: "done", title: plain("Save as FCPXML · Walter"), subtitle: "FCPXML", message: plain("Saved Walter.fcpxml"), rev: 3 }) })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin “FCPXML”: Save as FCPXML · Walter", { description: "Saved Walter.fcpxml" })
  })

  it("a save started before a restart says AutoScene's words in the language", async () => {
    await inLanguage("ro", { plugins: { toast: { finished: { named: "Plugin „{{name}}”: {{action}}" } } }, backend: { plugin: { saved: "S-a salvat {{file}}" } } })
    const message = { key: "plugin.saved", args: { file: "Walter.fcpxml" }, text: "Saved Walter.fcpxml" }
    handlers.jobChanged({ payload: plugin({ state: "done", title: plain("Save as FCPXML · Walter"), subtitle: "FCPXML", message, rev: 3 }) })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin „FCPXML”: Save as FCPXML · Walter", { description: "S-a salvat Walter.fcpxml" })
  })

  it("keeps a plugin's own words inside a sentence in another language", async () => {
    await inLanguage("ro", { plugins: { toast: { finished: { named: "Plugin „{{name}}”: {{action}}" }, failed: { named: "Plugin „{{name}}”: {{action}} a eșuat" } } } })
    handlers.jobChanged({ payload: plugin({ state: "done", message: plain("Posted 12 clips."), rev: 3 }) })
    handlers.jobChanged({ payload: plugin({ id: "j3", state: "failed", error: plain("Discord refused the post."), rev: 4 }) })
    expect(toast).toHaveBeenCalledExactlyOnceWith("Plugin „Discord notify”: Post to Discord", { description: "Posted 12 clips." })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("Plugin „Discord notify”: Post to Discord a eșuat", {
      description: "Discord refused the post.",
    })
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

  it("plugin:compiling tells the review which ticket compiles", () => {
    handlers.pluginCompiling({ payload: { ticketId: "t1" } })
    expect(usePluginReview.getState().compiling).toBe("t1")
    usePluginReview.getState().compileEnded()
    expect(usePluginReview.getState().compiling).toBeNull()
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

describe("the download of NVIDIA files", () => {
  const hardware = (state: "downloading" | "ready") =>
    ({ accel: { row: state === "ready" ? { state } : { state, done: 0, total: 100 } } }) as HardwareInfo

  it("moves the bar of the row, and only while it downloads", () => {
    queryClient.setQueryData(qk.hardware, hardware("downloading"))
    handlers.accelProgress({ payload: { done: 45, total: 100 } })
    expect(queryClient.getQueryData<HardwareInfo>(qk.hardware)?.accel.row).toEqual({ state: "downloading", done: 45, total: 100 })
    queryClient.setQueryData(qk.hardware, hardware("ready"))
    handlers.accelProgress({ payload: { done: 46, total: 100 } })
    expect(queryClient.getQueryData<HardwareInfo>(qk.hardware)?.accel.row).toEqual({ state: "ready" })
  })

  it("says how it ended outside Settings, where the row can't", () => {
    window.location.hash = "#/p/1"
    handlers.accelFinished({ payload: { error: null } })
    expect(toast.success).toHaveBeenCalledExactlyOnceWith("NVIDIA acceleration is ready")
    handlers.accelFinished({ payload: { error: "No room." } })
    expect(toast.error).toHaveBeenCalledExactlyOnceWith("NVIDIA acceleration couldn't be downloaded", { description: "No room." })
  })

  it("leaves it to the row in Settings, and reads the setting it switched on", () => {
    window.location.hash = "#/settings/general"
    const read = vi.spyOn(queryClient, "invalidateQueries")
    handlers.accelFinished({ payload: { error: null } })
    expect(toast.success).not.toHaveBeenCalled()
    expect(read).toHaveBeenCalledWith({ queryKey: qk.settings })
    window.location.hash = ""
    read.mockRestore()
  })
})
