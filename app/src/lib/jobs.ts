import { toast } from "sonner"
import type { JobInfo } from "@/bindings"
import { commands, errorMessage } from "./ipc"

/**
 * Stopping a media's job, named by what that job does: a preview being
 * built is never stopped as "analysis".
 */
export function cancelLabel(kind: JobInfo["kind"]): string {
  switch (kind) {
    case "ingest":
      return "Cancel indexing"
    case "analyze":
      return "Cancel analysis"
    case "proxy":
      return "Cancel preview"
    default:
      return stopWord(kind)
  }
}

/** The word for stopping a job beside its title: a plugin's run is stopped, other work cancelled. */
export function stopWord(kind: JobInfo["kind"]): "Stop" | "Cancel" {
  return kind === "plugin" ? "Stop" : "Cancel"
}

/**
 * What stops when its episode or project goes, in the order it is named.
 * Upkeep the person never started (posters, portraits, regrouping) isn't.
 */
const STOPPING: [JobInfo["kind"], (n: number) => string][] = [
  ["ingest", () => "indexing"],
  ["analyze", () => "analysing"],
  ["proxy", (n) => (n === 1 ? "building a preview" : "building previews")],
  ["export", (n) => (n === 1 ? "an export" : `${n} exports`)],
  ["plugin", (n) => (n === 1 ? "a plugin run" : `${n} plugin runs`)],
]

/**
 * The sentence a removal ends with when work runs on what goes: "Work
 * running on it stops first: indexing, analysing and an export." `null` when
 * nothing the person would miss runs. Running work is named by what it does,
 * never "analysis", which the project dialog uses for what is stored.
 */
export function stopsFirst(kinds: JobInfo["kind"][]): string | null {
  const words = STOPPING.flatMap(([kind, word]) => {
    const n = kinds.filter((k) => k === kind).length
    return n > 0 ? [word(n)] : []
  })
  const last = words.pop()
  if (last === undefined) return null
  return `Work running on it stops first: ${words.length > 0 ? `${words.join(", ")} and ${last}` : last}.`
}

/** Stop `job`, as the Activity drawer does; a toast says so when it can't. */
export function cancelJob(job: JobInfo) {
  const stop = stopWord(job.kind).toLowerCase()
  void commands.jobCancel(job.id).catch((e) => toast.error(`Couldn't ${stop} the job`, { description: errorMessage(e) }))
}
