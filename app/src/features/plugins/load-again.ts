import { useState } from "react"
import type { PluginInfo } from "@/bindings"
import { commands } from "@/lib/ipc"
import { baseName } from "@/lib/media"
import { usePluginReview } from "@/state/plugin-review"

/**
 * Loading a loaded folder again, through its review: the one way to agree to
 * a changed program. The review opens in the shell's dialog; `checking`
 * while the folder is read.
 */
export function useLoadAgain() {
  const [checking, setChecking] = useState(false)
  const loadAgain = async (p: Pick<PluginInfo, "folder" | "name">) => {
    setChecking(true)
    try {
      await usePluginReview.getState().openReview(() => commands.pluginInspectFolder(p.folder), baseName(p.folder))
    } finally {
      setChecking(false)
    }
  }
  return { checking, loadAgain }
}
