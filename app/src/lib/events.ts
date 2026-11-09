import { toast } from "sonner"
import type { HardwareInfo, JobInfo, Msg } from "@/bindings"
import { takePackSave } from "@/features/plugins/pack-saves"
import { pluginToast } from "@/features/plugins/plugin-toast"
import { isFinished, useActivity } from "@/state/activity"
import { usePluginReview } from "@/state/plugin-review"
import { useRescan } from "@/state/rescan"
import { say, t } from "./i18n"
import { commands, events } from "./ipc"
import { reveal } from "./os"
import { exportsQuery, isMadePack, qk, queryClient } from "./queries"

/**
 * Backend events → React Query invalidation and the activity store. The UI
 * never polls: every change the backend makes is announced here.
 *
 * | event            | effect                                             |
 * |------------------|----------------------------------------------------|
 * | job:changed      | activity store; toast when an export finishes or a plugin says something, exports(pid) when an export ends |
 * | job:progress     | activity store (progress only)                     |
 * | library:changed  | projects, project(id), exportDir                   |
 * | media:changed    | media(pid), frameMap(pid, mid), project(pid)       |
 * | analysis:changed | segments(pid, mid)                                 |
 * | cast:changed     | cast(pid) and everything under it (tracks, presence, packs) |
 * | rescan:changed   | the rescan store (faces read, how far); rescan(pid) when what it keeps changed |
 * | pack:changed     | pack(pid, cid): its settings (an export keeps them), and the shots a plugin's filter judged, once per episode |
 * | export:changed   | exports(pid)                                       |
 * | plugins:changed  | plugins and every plugin's settings, the catalog; every pack's preview and plan (its filters, on or off); settings when no plugin is named |
 * | catalog:changed  | the catalog, the plugins; a toast for each plugin it newly withdrew |
 * | catalog:progress | the install review's download (progress only)      |
 * | plugin-page:event | an open plugin page (PluginFrame listens)         |
 * | hardware:changed | hardware (the graphics the analysis runs on)       |
 * | plugins:opened   | packages opened from outside (useOpenedPackages listens) |
 * | accel:progress   | hardware: the bar of NVIDIA's download             |
 * | accel:finished   | settings; toast unless Settings is open            |
 */
export function startEventBridge(where: { goPlugins: () => void; goRescan?: (pid: string) => void }): () => void {
  const unlisten: Promise<() => void>[] = []

  unlisten.push(
    events.jobChanged.listen(({ payload }) => {
      const prev = useActivity.getState().jobs[payload.id]
      if (prev && prev.rev > payload.rev) return
      useActivity.getState().upsert(payload)
      if (prev?.state === payload.state) return
      // An export's card reads it again when its job ends, should the
      // export's own event be late or not come (cancelled while queued).
      if (payload.kind === "export" && payload.projectId && isFinished(payload)) {
        void queryClient.invalidateQueries({ queryKey: qk.exports(payload.projectId) })
      }
      notifyFinished(payload)
      if (payload.kind === "rescan" && payload.projectId && isFinished(payload)) {
        useRescan.getState().ended(payload.projectId)
        const pid = payload.projectId
        if (payload.state === "done") {
          toast(t("cast:rescan.finished"), {
            description: payload.subtitle ? say(payload.subtitle) : undefined,
            action: where.goRescan && { label: t("cast:rescan.show"), onClick: () => where.goRescan?.(pid) },
          })
        }
      }
    }),
    events.rescanChanged.listen(({ payload }) => {
      if (payload.total > 0) {
        useRescan.getState().progress(payload.projectId, payload.looked, payload.done, payload.total)
      }
      if (payload.results) void queryClient.invalidateQueries({ queryKey: qk.rescan(payload.projectId) })
    }),
    events.jobProgress.listen(({ payload }) => useActivity.getState().progress(payload)),
    events.libraryChanged.listen(({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: qk.projects })
      if (payload.projectId) void queryClient.invalidateQueries({ queryKey: qk.project(payload.projectId) })
      // A rename moves the project's default export folder.
      void queryClient.invalidateQueries({ queryKey: ["exportDir"] })
    }),
    events.mediaChanged.listen(({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: qk.media(payload.projectId) })
      void queryClient.invalidateQueries({ queryKey: qk.project(payload.projectId) })
      // A file gone or back, indexed again: the packs cut from it change.
      void queryClient.invalidateQueries({ queryKey: [...qk.cast(payload.projectId), "pack"] })
      const key = payload.mediaId
        ? qk.frameMap(payload.projectId, payload.mediaId)
        : (["frameMap", payload.projectId] as const)
      void queryClient.invalidateQueries({ queryKey: key })
    }),
    events.analysisChanged.listen(({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: qk.segments(payload.projectId, payload.mediaId) })
    }),
    events.castChanged.listen(({ payload }) => {
      // Prefix match: the cast, its track lists, presence lanes and clip lists.
      void queryClient.invalidateQueries({ queryKey: qk.cast(payload.projectId) })
      // Portraits may have been made anew: they are asked for again when
      // next shown, not now (the face on screen stays).
      void queryClient.invalidateQueries({ queryKey: qk.portraits(payload.projectId), refetchType: "none" })
    }),
    events.anilistChanged.listen(({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: qk.anilistNames(payload.projectId), exact: true })
    }),
    events.portraitsChanged.listen(({ payload }) => {
      // Covers got sharp: the cast alone, not its lists.
      void queryClient.invalidateQueries({ queryKey: qk.cast(payload.projectId), exact: true })
    }),
    events.packChanged.listen(({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: qk.pack(payload.projectId, payload.characterId) })
    }),
    events.exportChanged.listen(({ payload }) => {
      void queryClient.invalidateQueries({ queryKey: qk.exports(payload.projectId) })
    }),
    events.pluginsChanged.listen(({ payload }) => {
      // Prefix match: the list and each plugin's settings.
      void queryClient.invalidateQueries({ queryKey: qk.plugins })
      // What the catalog offers is worked out against what is installed.
      void queryClient.invalidateQueries({ queryKey: qk.catalog })
      // A plugin turned on or off, or a setting a filter judges by: the packs, their sizes too, are made again.
      void queryClient.invalidateQueries({ predicate: isMadePack })
      // None named: the setting may have changed, under a settings save
      // still on its way with the value from before.
      if (payload.pluginId == null) void queryClient.invalidateQueries({ queryKey: qk.settings })
    }),
    events.hardwareChanged.listen(() => void queryClient.invalidateQueries({ queryKey: qk.hardware })),
    events.catalogChanged.listen(({ payload }) => {
      // A check may offer new versions, and withdraw installed plugins.
      void queryClient.invalidateQueries({ queryKey: qk.catalog })
      void queryClient.invalidateQueries({ queryKey: qk.plugins })
      void tellWithdrawn(payload.withdrawn, where.goPlugins)
    }),
    events.catalogProgress.listen(({ payload }) => usePluginReview.getState().progress(payload.pluginId, payload.done, payload.total)),
    events.accelProgress.listen(({ payload }) => {
      queryClient.setQueryData<HardwareInfo>(qk.hardware, (hw) =>
        hw?.accel.row.state === "downloading"
          ? { ...hw, accel: { ...hw.accel, row: { state: "downloading", done: payload.done, total: payload.total } } }
          : hw,
      )
    }),
    events.accelFinished.listen(({ payload }) => {
      // A first download finished switches it on.
      void queryClient.invalidateQueries({ queryKey: qk.settings })
      // In Settings the row says so itself.
      if (window.location.hash.startsWith("#/settings")) return
      if (payload.error) toast.error(t("shell:events.nvidiaFailed"), { description: say(payload.error) })
      else toast.success(t("shell:events.nvidiaReady"))
    }),
    events.pluginCompiling.listen(({ payload }) => usePluginReview.getState().compileStarted(payload.ticketId)),
  )
  // Snapshot only once the listeners are live, so nothing falls between the two.
  void Promise.all(unlisten)
    .then(() => commands.jobsList())
    .then((jobs) => useActivity.getState().replaceAll(jobs))

  return () => unlisten.forEach((p) => void p.then((un) => un()))
}

/** The plugins whose withdrawal was told of this session: each is told once, however many checks name it. */
const toldWithdrawn = new Set<string>()

/**
 * Says that AutoScene withdrew the plugins `ids`, one toast each, in
 * AutoScene's words: the catalog's reason, read from the plugins as they
 * stand now. A plugin the list doesn't show as withdrawn gets none: the
 * toast says no more than the list does.
 */
async function tellWithdrawn(ids: string[], goPlugins: () => void) {
  if (ids.every((id) => toldWithdrawn.has(id))) return
  let plugins
  try {
    plugins = await queryClient.fetchQuery({ queryKey: qk.plugins, queryFn: () => commands.pluginsList(), staleTime: 0 })
  } catch {
    return
  }
  for (const id of ids) {
    const p = plugins.find((x) => x.id === id)
    if (p?.withdrawn == null || toldWithdrawn.has(id)) continue
    toldWithdrawn.add(id)
    toast.error(t("shell:events.withdrew", { name: p.name }), {
      description: p.withdrawn,
      action: { label: t("shell:events.show"), onClick: goPlugins },
    })
  }
}

/**
 * Exports finish in the background; say so wherever the user is. A plugin's
 * job that finishes says its message the same way, named by its plugin. Its
 * name, title and words are all the plugin's own (a plugin may be called
 * "AutoScene"), so the toast starts with a word it can't choose. A pack
 * saved with a plugin's action says which file, and shows it in its folder.
 */
function notifyFinished(job: JobInfo) {
  // A download that ends well shows as its episode; one that fails says why, named by its plugin.
  if (job.kind === "download") {
    if (job.state === "failed")
      pluginToast(job.subtitle, "jobFailed", { action: say(job.title) }, { description: errorOf(job) })
    return
  }
  if (job.kind === "plugin") {
    const saved = isFinished(job) ? takePackSave(job.id) : undefined
    if (job.state === "failed")
      pluginToast(job.subtitle, "jobFailed", { action: say(job.title) }, { description: errorOf(job) })
    else if (job.state === "done" && saved)
      pluginToast(
        job.subtitle,
        "fileSaved",
        { file: saved.split(/[\\/]/).pop() ?? "" },
        { action: { label: t("plugins:toast.showInFolder"), onClick: () => reveal(saved) } },
      )
    else if (job.state === "done" && job.message)
      pluginToast(job.subtitle, "jobFinished", { action: say(job.title) }, { description: say(job.message) })
    return
  }
  if (job.kind !== "export") {
    if (job.state === "failed") toast.error(failedTitle(job.title), { description: errorOf(job) })
    return
  }
  if (job.state === "done") {
    // Said once it's known where the export went (a moment): Show in folder goes there.
    void exportPlace(job).then((place) =>
      toast.success(t("shell:events.exportDone"), {
        description: job.subtitle ?? undefined,
        action: place ? { label: t("shell:events.showInFolder"), onClick: () => reveal(place) } : undefined,
      }),
    )
  } else if (job.state === "failed") toast.error(t("shell:events.exportErrors"), { description: errorOf(job) })
}

/** What a job that failed says went wrong, if it says. */
const errorOf = (job: JobInfo) => (job.error ? say(job.error) : undefined)

/** A failed job's title: indexing and analysing say that they couldn't, by their key, and their English when they lost their name; the others name their work, with a name or without (the upkeep jobs). */
function failedTitle(title: Msg): string {
  if (title.key !== "job.indexing" && title.key !== "job.analysing") return say(title)
  const name = title.args?.name
  if (name == null) return title.text
  const named = { name: typeof name === "object" ? say(name) : name }
  return title.key === "job.indexing" ? t("shell:jobFailed.indexing", named) : t("shell:jobFailed.analysing", named)
}

/**
 * Where a finished export is: its one file (the clips joined, or a single
 * clip), else its folder. `null` when that can't be read.
 */
async function exportPlace(job: JobInfo): Promise<string | null> {
  if (!job.projectId || !job.exportId) return null
  try {
    // Read afresh: the list on hand may be from before the export was made.
    const list = await queryClient.fetchQuery({ ...exportsQuery(job.projectId), staleTime: 0 })
    const ex = list.find((e) => e.id === job.exportId)
    if (!ex) return null
    const single = ex.items.length === 1 ? ex.items[0].path : null
    return ex.merged?.path ?? single ?? ex.outDir
  } catch {
    return null
  }
}
