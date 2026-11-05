import type * as React from "react"
import { useEffect, useState } from "react"
import { useRouterState } from "@tanstack/react-router"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { FolderPlus, PuzzlePiece } from "@/components/icons"
import { toast } from "sonner"
import { useTranslation } from "react-i18next"
import { isPluginPagePath } from "@/features/plugins/page-items"
import { isPluginPackage } from "@/features/plugins/plugin-items"
import { busyReviewing, reviewPackages } from "@/features/plugins/review-packages"
import { commands, errorMessage } from "@/lib/ipc"
import { isTauri } from "@/lib/platform"
import { addedFiles } from "@/lib/projects"
import { cn } from "@/lib/utils"
import { useNewProject } from "@/state/new-project"

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
export function DropZone() {
  const { t } = useTranslation("shell")
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
          else toast(t("drop.finishReview"))
        } else if (np.open) {
          // A package is no source for a project.
          const media = paths.filter((p) => !isPluginPackage(p))
          if (media.length < paths.length) toast(t("drop.packagesNotFiles"))
          if (media.length) np.show([...np.seed, ...media])
        } else if (pluginPage) return
        else if (paths.some(isPluginPackage)) {
          // Packages are reviewed on their own: not added to a project, not made one.
          if (packages) reviewPackages(paths)
          else toast(t("drop.packagesAlone"))
        } else if (pid) {
          commands
            .mediaAdd(pid, paths)
            .then((n) => toast(addedFiles(n)))
            .catch((e) => toast.error(t("files.addFailed"), { description: errorMessage(e) }))
        } else np.show(paths)
      }
    })
    return () => void un.then((f) => f())
  }, [pid, pluginPage, t])

  if (over == null || dialogOpen) return null
  const pkg = over === "package"
  if (over === "mixed") {
    return (
      <Overlay
        quiet
        icon={<PuzzlePiece />}
        heading={t("drop.mixed.heading")}
        hint={t("drop.mixed.hint")}
      />
    )
  }
  return (
    <Overlay
      icon={pkg ? <PuzzlePiece /> : <FolderPlus />}
      heading={pkg ? t("drop.plugin.heading") : pid ? t("drop.addHere") : t("drop.create")}
      hint={pkg ? t("drop.plugin.hint") : t("drop.media")}
    />
  )
}

/** The window's answer to files dragged over it: gold where a drop is taken, grey where it is not (`quiet`). */
function Overlay({ icon, heading, hint, quiet }: { icon: React.ReactNode; heading: string; hint: string; quiet?: boolean }) {
  return (
    <div className="pointer-events-none fixed inset-0 z-[60] p-4">
      <div
        className={cn(
          "flex size-full animate-fade-in flex-col items-center justify-center rounded-[28px] border-2 border-dashed bg-black/60 backdrop-blur-xl",
          quiet ? "border-hairline" : "border-accent-line",
        )}
      >
        <span
          className={cn(
            "flex size-20 items-center justify-center rounded-full [&_svg]:size-9",
            quiet ? "bg-white/5 text-fg-3" : "bg-accent-subtle text-accent shadow-[0_0_60px_rgb(224_180_92/0.3)]",
          )}
        >
          {icon}
        </span>
        <p className={cn("display mt-6 text-4xl", quiet ? "text-fg-2" : "text-fg-1")}>{heading}</p>
        <p className={cn("mt-2 text-md", quiet ? "text-fg-3" : "text-fg-2")}>{hint}</p>
      </div>
    </div>
  )
}
