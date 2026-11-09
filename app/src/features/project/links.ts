import { useMemo } from "react"
import type { JobInfo } from "@/bindings"
import { sourceItems } from "@/features/plugins/plugin-items"
import { usePlugins } from "@/lib/queries"
import { isFinished, useActivity } from "@/state/activity"

/** The downloads of project `pid` that wait or run, the oldest first. */
export function useProjectDownloads(pid: string): JobInfo[] {
  const jobs = useActivity((s) => s.jobs)
  const order = useActivity((s) => s.order)
  return useMemo(
    () =>
      order
        .map((id) => jobs[id])
        .filter((j) => j && j.kind === "download" && j.projectId === pid && !isFinished(j))
        .reverse(),
    [jobs, order, pid],
  )
}

/** Whether a plugin that may run now takes links: Add links… shows only then. */
export function useTakesLinks(): boolean {
  const { data: plugins } = usePlugins()
  return sourceItems(plugins).length > 0
}
