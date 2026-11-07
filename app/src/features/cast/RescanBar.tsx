import { useTranslation } from "react-i18next"
import { Stop } from "@/components/icons"
import { Avatar } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { ProgressRing } from "@/components/ui/progress"
import { fmt } from "@/lib/i18n"
import { cancelJob } from "@/lib/jobs"
import { useCast } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { useActivity } from "@/state/activity"
import { useRescan } from "@/state/rescan"

/** Avatars shown at most, side by side. */
const SHOWN = 4

/** The project's unfinished rescan, if any. */
function useRescanJob(pid: string) {
  return useActivity((s) => {
    const mine = s.order.map((id) => s.jobs[id]).filter((j) => j?.kind === "rescan" && j.projectId === pid)
    // One that runs before one that waits behind it.
    return mine.find((j) => j.state === "running") ?? mine.find((j) => j.state === "queued")
  })
}

/**
 * The rescan running on this project, in the place the selection's bar takes
 * (`lifted`: above it while something is selected). Waiting behind an
 * analysis, its ring stands empty and still.
 */
export function RescanBar({ pid, lifted = false }: { pid: string; lifted?: boolean }) {
  const { t } = useTranslation("cast")
  const job = useRescanJob(pid)
  const run = useRescan((s) => s.runs[pid])
  const { data: cast } = useCast(pid, job !== undefined)
  if (!job) return null
  const waiting = job.state === "queued"
  const faces = (run?.characters ?? []).flatMap((id) => cast?.characters.find((c) => c.id === id) ?? [])
  const value = run && run.total > 0 ? run.done / run.total : null
  return (
    <div
      className={cn(
        "glass-strong fixed bottom-6 left-1/2 z-40 flex w-max max-w-[calc(100vw-3rem)] -translate-x-1/2 animate-pop-in items-center gap-3 rounded-full py-1.5 pr-1.5 pl-4 transition-transform duration-[var(--dur-med)]",
        lifted && "-translate-y-14",
      )}
    >
      <ProgressRing value={value} waiting={waiting} className="size-5" />
      <span className="text-md font-semibold whitespace-nowrap text-fg-1">
        {waiting
          ? t("rescan.bar.waiting")
          : run && run.total > 0
            ? t("rescan.bar.reading", { done: fmt.count(run.done), total: fmt.count(run.total) })
            : t("rescan.bar.starting")}
      </span>
      {faces.length > 0 && (
        <span className="flex -space-x-2 pl-1">
          {faces.slice(0, SHOWN).map((c) => (
            <Avatar key={c.id} path={c.coverSmall ?? c.cover} className="size-7 ring-2 ring-canvas" />
          ))}
        </span>
      )}
      <Button size="sm" variant="ghost" onClick={() => cancelJob(job)}>
        <Stop /> {t("rescan.bar.stop")}
      </Button>
    </div>
  )
}
