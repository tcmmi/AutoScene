import { useTranslation } from "react-i18next"
import { DownloadSimple } from "@/components/icons"
import { Progress } from "@/components/ui/progress"
import { formatPercent } from "@/lib/format"
import { say } from "@/lib/i18n"
import { cn } from "@/lib/utils"
import { jobBar } from "@/state/activity"
import { useProjectDownloads } from "./links"

/** What a project's downloads from links are doing right now (nothing when none waits or runs), as AnalysisStatus shows its analysis. */
export function DownloadStatus({ pid, className }: { pid: string; className?: string }) {
  const { t } = useTranslation("project")
  const jobs = useProjectDownloads(pid)
  const job = jobs.find((j) => j.state === "running") ?? jobs[0]
  if (!job) return null
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
          job.state === "running" ? "bg-accent-subtle text-accent" : "bg-white/[0.06] text-fg-3",
        )}
      >
        {job.state === "running" && (
          <span className="absolute inset-0 animate-ping rounded-full bg-accent/20 [animation-duration:2.4s]" />
        )}
        <DownloadSimple />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3 text-sm">
          <span className="truncate font-medium text-fg-1">
            {job.state === "queued" ? t("downloads.waiting") : job.stage ? say(job.stage) : say(job.title)}
          </span>
          <span className="timecode shrink-0 text-xs text-fg-2">{formatPercent(job.progress)}</span>
        </div>
        <Progress className="mt-2" value={jobBar(job)} />
        {waiting > 0 && <div className="mt-1.5 text-xs text-fg-3">{t("downloads.moreQueued", { count: waiting })}</div>}
      </div>
    </div>
  )
}
