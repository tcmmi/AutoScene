import { toast } from "sonner"
import { commands } from "@/lib/ipc"
import { baseName } from "@/lib/media"
import { usePluginReview } from "@/state/plugin-review"

/** Whether a review is read or shown now: one asked for waits its turn. */
export const busyReviewing = () => {
  const { review, loading } = usePluginReview.getState()
  return review != null || loading
}

/**
 * Queues plugin packages (`.asp` paths) for their install review and says
 * what happens to them: "Reading X…" while the first is read, "X waits for
 * the review open now" behind another review. A package that already waits,
 * is read or is shown (dropped or opened twice) is not queued again, and the
 * window says so. Dropped packages and those opened from outside come here.
 */
export function reviewPackages(paths: string[]) {
  const store = usePluginReview.getState()
  const fresh = paths.filter((p, i) => paths.indexOf(p) === i && !store.pending(p))
  if (fresh.length === 0) {
    toast(paths.length === 1 ? `${baseName(paths[0])} is already being reviewed` : "These plugins are already being reviewed")
    return
  }
  const label = baseName(fresh[0])
  const waits = busyReviewing()
  const rest = fresh.length - 1
  const reviews = fresh.map((p) => store.openReview(() => commands.pluginInspect(p), baseName(p), p))
  if (waits) {
    toast(fresh.length === 1 ? `${label} waits for the review open now` : `${fresh.length} plugins wait for the review open now`)
    return
  }
  const id = toast.loading(rest > 0 ? `Reading ${label}… ${rest} more wait their turn` : `Reading ${label}…`)
  void reviews[0].finally(() => toast.dismiss(id))
}
