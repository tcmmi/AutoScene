import { toast } from "sonner"
import type { ActionKind, PackNaming, PackSpec, PluginInfo } from "@/bindings"
import { commands, errorMessage } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { rememberPackSave } from "./pack-saves"
import { pluginToast } from "./plugin-toast"

/**
 * Run a plugin's export action on an export, or its command (in the open
 * project, if any). The run shows in Activity; a toast says when it can't
 * start, or the same run waits or runs already. The title is the plugin's,
 * so the toast names its plugin first.
 */
export async function runPlugin(
  run: { pluginId: string; pluginName: string; actionId: string; title: string },
  kind: ActionKind,
  projectId: string | null,
  exportId: string | null,
) {
  try {
    const job = await commands.pluginRun(run.pluginId, run.actionId, kind, projectId, exportId)
    if (job == null)
      pluginToast(run.pluginName, { text: `${run.title} is already waiting or running`, description: "It's in Activity." })
  } catch (e) {
    pluginToast(run.pluginName, { text: `${run.title} couldn't start`, description: errorMessage(e), tone: "error" })
  }
}

/**
 * Save a character's pack, as the grid shows it (`stamp` names that), with a
 * plugin's pack action: the system's save dialog asks where first, so a
 * closed one says nothing. The run shows in Activity; a toast says when it
 * can't start, or the same pack action of this pack waits or runs already.
 * The title is the plugin's, so the toast names its plugin first. Where the
 * file goes is kept for the run's end, which says it saved the file and
 * offers to show it.
 */
export async function runPackAction(
  run: { pluginId: string; pluginName: string; actionId: string; title: string },
  projectId: string,
  characterId: string,
  spec: PackSpec,
  naming: PackNaming,
  stamp: string,
) {
  // "Save as FCPXML…" reads "Save as FCPXML couldn't start".
  const title = run.title.replace(/[….\s]+$/u, "")
  try {
    const start = await commands.packActionRun(projectId, characterId, `${run.pluginId}/${run.actionId}`, spec, naming, stamp)
    if (start.kind === "queued") rememberPackSave(start.job, start.path)
    else if (start.kind === "waits")
      pluginToast(run.pluginName, { text: `${title} is already waiting or running`, description: "It's in Activity." })
  } catch (e) {
    pluginToast(run.pluginName, { text: `${title} couldn't start`, description: errorMessage(e), tone: "error" })
  }
}

/**
 * Let plugins AutoScene hasn't reviewed run, or not (off, their runs stop).
 * On, AutoScene asks first, in a dialog of the system's own: whether they
 * may run now.
 */
export async function allowUnreviewed(on: boolean): Promise<boolean> {
  try {
    const settings = await commands.pluginsAllowUnreviewed(on)
    queryClient.setQueryData(qk.settings, settings)
    void queryClient.invalidateQueries({ queryKey: qk.plugins })
    return settings.allowUnreviewedPlugins ?? false
  } catch (e) {
    toast.error("Couldn't save the setting", { description: errorMessage(e) })
    return false
  }
}

/**
 * Open a plugin's source in the browser. AutoScene reads the link from the
 * plugin's own plugin.json, and asks first, in a dialog of the system's
 * own, unless it is a GitHub repository. A no says nothing more.
 */
export async function openSource(p: Pick<PluginInfo, "id">) {
  try {
    await commands.pluginOpenSource(p.id)
  } catch (e) {
    toast.error("Couldn't open the link", { description: errorMessage(e) })
  }
}

/** Put `p` as the backend gave it back into the plugin list on hand. */
export function keepPlugin(p: PluginInfo) {
  queryClient.setQueryData<PluginInfo[]>(qk.plugins, (list) => list?.map((x) => (x.id === p.id ? p : x)))
}
