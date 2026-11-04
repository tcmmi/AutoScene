import { create } from "zustand"
import type { PluginReview } from "@/bindings"
import { commands } from "@/lib/ipc"
import { baseName, pickFolder, pickPluginFile } from "@/lib/media"
import { useSettings } from "@/lib/queries"
import { usePluginReview } from "@/state/plugin-review"

type InstallFlow = {
  /** What is being read, once picked: the entries that started it spin, and every entry waits. */
  checking: "file" | "folder" | null
  /** From the click until the plugin is read (or failed to be): the picker is open, or the plugin is read. */
  busy: boolean
}

/**
 * The app's one install flow. The Plugins page's buttons, its menu and the
 * rail tile's menu all start it, so one picker runs at a time. The review it
 * reads opens in the shell's one dialog (`usePluginReview`), so it survives
 * the rail being hidden, and one asked for while another is shown waits.
 */
export const useInstallFlow = create<InstallFlow>(() => ({ checking: null, busy: false }))

async function inspect(what: "file" | "folder", pick: () => Promise<string | null>, check: (path: string) => Promise<PluginReview>) {
  // A second picker over the first would read a second plugin behind the person's back.
  if (useInstallFlow.getState().busy) return
  useInstallFlow.setState({ busy: true })
  try {
    const path = await pick()
    if (!path) return
    useInstallFlow.setState({ checking: what })
    // The review opens in the shell's dialog, once it is read; one that can't be read is named by its file.
    await usePluginReview.getState().openReview(() => check(path), baseName(path), path)
  } finally {
    useInstallFlow.setState({ checking: null, busy: false })
  }
}

/**
 * Installing a plugin from its .asp file, or loading one from its folder:
 * the person picks it, AutoScene reads and reviews it, and the shell's one
 * `InstallReviewDialog` asks. `checking` names what is being read. A folder
 * only while unreviewed plugins are allowed: the backend refuses it otherwise.
 */
export function usePluginInstall() {
  const allowed = useSettings().data?.allowUnreviewedPlugins ?? false
  const checking = useInstallFlow((s) => s.checking)
  return {
    allowed,
    checking,
    install: () => void inspect("file", pickPluginFile, commands.pluginInspect),
    loadFolder: () => void inspect("folder", () => pickFolder("Choose the plugin's folder"), commands.pluginInspectFolder),
  }
}
