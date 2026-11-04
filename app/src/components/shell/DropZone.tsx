import { useEffect, useState } from "react"
import { useRouterState } from "@tanstack/react-router"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { FolderPlus, PuzzlePiece } from "@/components/icons"
import { toast } from "sonner"
import { isPluginPagePath } from "@/features/plugins/page-items"
import { isPluginPackage } from "@/features/plugins/plugin-items"
import { commands, errorMessage } from "@/lib/ipc"
import { baseName } from "@/lib/media"
import { isTauri } from "@/lib/platform"
import { useNewProject } from "@/state/new-project"
import { usePluginReview } from "@/state/plugin-review"

/**
 * Files and folders dropped anywhere on the window: inside a project they are
 * added to it, elsewhere they start a new project. While the New Project
 * dialog is open they fill its file list, which lights up for them (no
 * window-wide "Create a project" over it). A plugin package (`.asp`) dropped
 * alone opens its install review, not a project: several are reviewed one
 * after the other, and the window says so. Dropped with other files they add
 * nothing, and the window says so before the drop. While a review is read or
 * shown no overlay comes up and files are not taken, packages wait their turn.
 * On a plugin's page a drop adds nothing and shows nothing: the window takes
 * every drop, also one meant for the page, which never gets it.
 */
const busyReviewing = () => {
  const { review, loading } = usePluginReview.getState()
  return review != null || loading
}

/** Queues every dropped package for review and says what happens to them. */
function reviewPackages(paths: string[]) {
  const label = baseName(paths[0])
  const waits = busyReviewing()
  const rest = paths.length - 1
  const reviews = paths.map((p) => usePluginReview.getState().openReview(() => commands.pluginInspect(p), baseName(p)))
  if (waits) {
    toast(paths.length === 1 ? `${label} waits for the review open now` : `${paths.length} plugins wait for the review open now`)
    return
  }
  const id = toast.loading(rest > 0 ? `Reading ${label}… ${rest} more wait their turn` : `Reading ${label}…`)
  void reviews[0].finally(() => toast.dismiss(id))
}

export function DropZone() {
  // What is dragged over the window: files for a project, plugin packages, or
  // both (which nothing takes).
  const [over, setOver] = useState<"files" | "package" | "mixed" | null>(null)
  const dialogOpen = useNewProject((s) => s.open)
  const path = useRouterState({ select: (s) => s.location.pathname })
  const pid = path.match(/^\/p\/([^/]+)/)?.[1]
  const pluginPage = isPluginPagePath(path)

  useEffect(() => {
    if (!isTauri) return
    const dragging = (on: boolean, paths: string[] = []) => {
      // Over a plugin's page only the New Project dialog takes files; while a
      // review is read or shown nothing but packages (waiting) is taken.
      const kind = paths.length > 0 && paths.every(isPluginPackage) ? "package" : paths.some(isPluginPackage) ? "mixed" : "files"
      setOver(on && !pluginPage && !busyReviewing() ? kind : null)
      useNewProject.getState().setDragging(on && !busyReviewing())
    }
    const un = getCurrentWebview().onDragDropEvent(({ payload }) => {
      // Only files from outside: text or a link dragged inside the window
      // (no paths) is no drop for a project. "over" keeps what "enter" said.
      if (payload.type === "enter") dragging(payload.paths.length > 0, payload.paths)
      else if (payload.type === "leave") dragging(false)
      else if (payload.type === "drop") {
        dragging(false)
        const paths = payload.paths
        if (!paths.length) return
        const np = useNewProject.getState()
        const packages = paths.every(isPluginPackage)
        if (busyReviewing()) {
          // A review is asked about: no second dialog over it.
          if (pluginPage) return
          if (packages) reviewPackages(paths)
          else toast("Finish the plugin review first.")
        } else if (np.open) {
          // A package is no source for a project.
          const media = paths.filter((p) => !isPluginPackage(p))
          if (media.length < paths.length) toast("Plugin packages are no project files. Close this window and drop them on their own.")
          if (media.length) np.show([...np.seed, ...media])
        } else if (pluginPage) return
        else if (paths.some(isPluginPackage)) {
          // Packages are reviewed on their own: not added to a project, not made one.
          if (packages) reviewPackages(paths)
          else toast("Drop plugin packages on their own.")
        } else if (pid) {
          commands
            .mediaAdd(pid, paths)
            .then((n) => toast(n === 0 ? "Nothing new to add" : `Added ${n} ${n === 1 ? "file" : "files"}`))
            .catch((e) => toast.error("Couldn't add files", { description: errorMessage(e) }))
        } else np.show(paths)
      }
    })
    return () => void un.then((f) => f())
  }, [pid, pluginPage])

  if (over == null || dialogOpen) return null
  const pkg = over === "package"
  if (over === "mixed") {
    return (
      <div className="pointer-events-none fixed inset-0 z-[60] p-4">
        <div className="flex size-full animate-fade-in flex-col items-center justify-center rounded-[28px] border-2 border-dashed border-hairline bg-black/60 backdrop-blur-xl">
          <span className="flex size-20 items-center justify-center rounded-full bg-white/5 text-fg-3 [&_svg]:size-9">
            <PuzzlePiece />
          </span>
          <p className="display mt-6 text-4xl text-fg-2">Drop plugin packages on their own</p>
          <p className="mt-2 text-md text-fg-3">Packages and other files together are not taken</p>
        </div>
      </div>
    )
  }
  return (
    <div className="pointer-events-none fixed inset-0 z-[60] p-4">
      <div className="flex size-full animate-fade-in flex-col items-center justify-center rounded-[28px] border-2 border-dashed border-accent-line bg-black/60 backdrop-blur-xl">
        <span className="flex size-20 items-center justify-center rounded-full bg-accent-subtle text-accent shadow-[0_0_60px_rgb(224_180_92/0.3)] [&_svg]:size-9">
          {pkg ? <PuzzlePiece /> : <FolderPlus />}
        </span>
        <p className="display mt-6 text-4xl text-fg-1">{pkg ? "Review a plugin" : pid ? "Add to this project" : "Create a project"}</p>
        <p className="mt-2 text-md text-fg-2">{pkg ? "Drop its .asp package" : "Drop a season folder or video files"}</p>
      </div>
    </div>
  )
}
