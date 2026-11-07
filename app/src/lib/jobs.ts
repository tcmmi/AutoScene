import { toast } from "sonner"
import type { JobInfo } from "@/bindings"
import { fmt, t } from "./i18n"
import { commands, errorMessage } from "./ipc"

/**
 * Stopping a media's job, named by what that job does: a preview being
 * built is never stopped as "analysis".
 */
export function cancelLabel(kind: JobInfo["kind"]): string {
  switch (kind) {
    case "ingest":
      return t("shell:jobs.cancelIndexing")
    case "analyze":
      return t("shell:jobs.cancelAnalysis")
    case "proxy":
      return t("shell:jobs.cancelPreview")
    default:
      return stopWord(kind)
  }
}

/** The word for stopping a job beside its title: a plugin's run is stopped, other work cancelled. */
export function stopWord(kind: JobInfo["kind"]): string {
  return kind === "plugin" ? t("common:action.stop") : t("common:action.cancel")
}

/** What stops when its episode or project goes, in the order it is named. Each says how many of its kind there are. */
const STOPPING: [JobInfo["kind"], (n: number) => string][] = [
  ["ingest", () => t("shell:jobs.stops.indexing")],
  ["analyze", () => t("shell:jobs.stops.analysing")],
  ["proxy", (count) => t("shell:jobs.stops.preview", { count })],
  ["export", (n) => t("shell:jobs.stops.export", { count: n, n })],
  ["rescan", () => t("shell:jobs.stops.rescan")],
  ["plugin", (n) => t("shell:jobs.stops.pluginRun", { count: n, n })],
]

/**
 * The sentence a removal ends with when work runs on what goes: "Work
 * running on it stops first: indexing, analysing and an export." `null` when
 * nothing the person would miss runs. Upkeep the person never started
 * (posters, portraits, regrouping) isn't named. Running work is named by
 * what it does, never "analysis", which the project dialog uses for what is
 * stored.
 */
export function stopsFirst(kinds: JobInfo["kind"][]): string | null {
  const words = STOPPING.flatMap(([kind, word]) => {
    const n = kinds.filter((k) => k === kind).length
    return n > 0 ? [word(n)] : []
  })
  if (words.length === 0) return null
  return t("shell:jobs.stopsFirst", { list: fmt.list(words) })
}

/** The toast title when stopping a job fails: a plugin's run is stopped, other work cancelled. */
export function stopFailed(kind: JobInfo["kind"]): string {
  return kind === "plugin" ? t("shell:activity.stopFailed") : t("shell:activity.cancelFailed")
}

/** Stop `job`, as the Activity drawer does; a toast says so when it can't. */
export function cancelJob(job: JobInfo) {
  void commands.jobCancel(job.id).catch((e) => toast.error(stopFailed(job.kind), { description: errorMessage(e) }))
}
