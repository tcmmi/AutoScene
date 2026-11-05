import { toast } from "sonner"
import type { ProjectSummary } from "@/bindings"
import { CONTENT_LABEL } from "./footage"
import { t } from "./i18n"
import { commands, errorMessage } from "./ipc"

/** "Movie" or "3 episodes": what a project is made of, for a tooltip or a palette entry. */
export function projectSize(p: Pick<ProjectSummary, "kind" | "mediaCount">): string {
  return p.kind === "movie" ? t("shell:project.movie") : t("shell:project.episodes", { count: p.mediaCount, n: p.mediaCount })
}

/** "Series · Live action · 3 episodes". */
export function projectFacts(p: ProjectSummary): string {
  const footage = CONTENT_LABEL[p.content]
  const n = p.mediaCount
  return p.kind === "movie" ? t("shell:projectFacts.movie", { footage, count: n, n }) : t("shell:projectFacts.series", { footage, count: n, n })
}

/** What a toast says after files were added: how many were new, or that none were. */
export function addedFiles(n: number, none: string = t("shell:files.nothingNew")): string {
  return n === 0 ? none : t("shell:files.added", { count: n, n })
}

/** Add files or a folder to a project, saying how many were new. */
export async function addToProject(pid: string, paths: string[]) {
  if (!paths.length) return
  try {
    const n = await commands.mediaAdd(pid, paths)
    toast(addedFiles(n, t("shell:files.alreadyIn")))
  } catch (e) {
    toast.error(t("shell:files.addFailed"), { description: errorMessage(e) })
  }
}
