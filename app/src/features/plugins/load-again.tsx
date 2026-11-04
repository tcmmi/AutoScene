import { useState } from "react"
import { toast } from "sonner"
import type { PluginInfo, PluginReview } from "@/bindings"
import { commands, errorMessage } from "@/lib/ipc"
import { InstallReviewDialog } from "./InstallReviewDialog"

/**
 * Loading a loaded folder again, through its review: the one way to agree to
 * a changed program. `dialog` is that review, to be rendered once; `checking`
 * while the folder is read.
 */
export function useLoadAgain() {
  const [checking, setChecking] = useState(false)
  const [review, setReview] = useState<PluginReview | null>(null)
  const loadAgain = async (p: Pick<PluginInfo, "folder">) => {
    setChecking(true)
    try {
      setReview(await commands.pluginInspectFolder(p.folder))
    } catch (e) {
      toast.error("Couldn't open the plugin", { description: errorMessage(e) })
    } finally {
      setChecking(false)
    }
  }
  return { checking, loadAgain, dialog: <InstallReviewDialog review={review} onClose={() => setReview(null)} /> }
}
