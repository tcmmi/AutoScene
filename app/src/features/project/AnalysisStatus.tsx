import { useTranslation } from "react-i18next"
import { UserFocus } from "@/components/icons"
import { Progress } from "@/components/ui/progress"
import { useProjectAnalysis } from "@/features/cast/cast"
import { formatPercent } from "@/lib/format"
import { useMediaList } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { jobBar } from "@/state/activity"

/** What the analysis of a project is doing right now (nothing when idle). */
export function AnalysisStatus({ pid, className }: { pid: string; className?: string }) {
  const { t } = useTranslation("project")
  const jobs = useProjectAnalysis(pid)
  const { data: media } = useMediaList(pid)
  const job = jobs.find((j) => j.state === "running") ?? jobs[0]
  if (!job) return null
  const m = media?.find((x) => x.id === job.mediaId)
  const waiting = jobs.length - 1
  return (
    <div
      className={cn(
        "flex w-full max-w-md items-center gap-3.5 rounded-2xl border border-white/10 bg-black/35 px-4 py-3 backdrop-blur-xl",
        className,
      )}
    >
      <span
        className={cn(
          "relative flex size-9 shrink-0 items-center justify-center rounded-full [&_svg]:size-[18px]",
          // Gold and a ping while it works; while it waits it stands still, as the Activity drawer's rows do.
          job.state === "running" ? "bg-accent-subtle text-accent" : "bg-white/[0.06] text-fg-3",
        )}
      >
        {job.state === "running" && (
          <span className="absolute inset-0 animate-ping rounded-full bg-accent/20 [animation-duration:2.4s]" />
        )}
        <UserFocus />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3 text-sm">
          <span className="truncate font-medium text-fg-1">
            {job.state === "queued" ? t("analysis.waiting") : (job.stage ?? t("analysis.running"))}
            {m && <span className="font-normal text-fg-2"> · {m.label || m.fileName}</span>}
          </span>
          <span className="timecode shrink-0 text-xs text-fg-2">{formatPercent(job.progress)}</span>
        </div>
        <Progress className="mt-2" value={jobBar(job)} />
        {waiting > 0 && <div className="mt-1.5 text-xs text-fg-3">{t("analysis.moreQueued", { count: waiting })}</div>}
      </div>
    </div>
  )
}
