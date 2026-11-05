import { Trans, useTranslation } from "react-i18next"
import { ArrowCounterClockwise, Check, Clock, FileX, Scissors, User, UserFocus, Warning, X } from "@/components/icons"
import { toast } from "sonner"
import type { JobInfo, MediaItem } from "@/bindings"
import { Progress } from "@/components/ui/progress"
import { Tooltip } from "@/components/ui/tooltip"
import { formatEta, formatPercent } from "@/lib/format"
import { t } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { cancelJob, cancelLabel } from "@/lib/jobs"
import { useProject, useSettings } from "@/lib/queries"
import { useMediaJob } from "@/state/activity"

// The number of shots and faces is set in the timecode face.
const MONO = { mono: <span className="timecode" /> }

function runningLabel(job: JobInfo): string {
  if (job.kind === "proxy") return t("project:status.preparingPreview")
  if (job.kind === "analyze") return job.stage ?? t("project:status.analysing")
  return job.stage ?? t("project:status.indexing")
}

function queuedLabel(m: MediaItem, job?: JobInfo): string {
  if (m.status !== "ready") return t("project:status.queued")
  if (job?.kind === "proxy") return t("project:status.previewQueued")
  if (job?.kind === "analyze") return t("project:status.analysisQueued")
  return t("project:status.queued")
}

/** Indexing, preview and analysis state of one media, with live progress from its job. */
export function StatusCell({ media: m, projectId }: { media: MediaItem; projectId: string }) {
  // Subscribes this cell to the language: the helpers above say their words with the global `t`.
  useTranslation("project")
  const job = useMediaJob(m.id)
  // People are found by their bodies too (the setting; never with Fast
  // Scan, never in anime, whose characters are found whole).
  const { data: settings } = useSettings()
  const { data: project } = useProject(projectId)
  const people = (settings?.findBodies ?? false) && !(project?.fastScan ?? false) && project?.content !== "anime"
  const retry = () => {
    commands.mediaRetry(projectId, m.id).catch((err) => toast.error(t("project:status.retryFailed"), { description: errorMessage(err) }))
  }
  const analyze = () => {
    commands
      .mediaAnalyze(projectId, m.id, null)
      .catch((err) => toast.error(t("project:actions.analysisFailed"), { description: errorMessage(err) }))
  }
  // The job whose state is shown here, never another one of the media's.
  const cancel = (job: JobInfo) => () => {
    cancelJob(job)
  }

  if (m.status === "missing") {
    return (
      <Tooltip label={m.path}>
        <span className="inline-flex items-center gap-1.5 text-sm text-danger">
          <FileX className="size-3.5" /> {t("project:status.fileMissing")}
        </span>
      </Tooltip>
    )
  }
  if (m.status === "failed") {
    return (
      <span className="inline-flex items-center gap-2 text-sm">
        <Tooltip label={m.error ?? t("project:status.indexingFailed")}>
          <span className="inline-flex items-center gap-1.5 text-danger">
            <Warning className="size-3.5" /> {t("project:status.failed")}
          </span>
        </Tooltip>
        <button onClick={retry} className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-3 hover:text-fg-1">
          <ArrowCounterClockwise className="size-3" /> {t("common:action.retry")}
        </button>
      </span>
    )
  }
  if (job && job.state === "running") {
    const eta = formatEta(job.etaMs)
    return (
      <div className="flex items-center gap-1.5">
        <div className="flex w-36 flex-col gap-1.5">
          <div className="flex justify-between gap-2 text-xs text-fg-3">
            <span className="truncate">{runningLabel(job)}</span>
            <span className="timecode shrink-0">{formatPercent(job.progress)}</span>
          </div>
          <Progress value={job.progress} />
          {eta && <span className="tabular text-xs text-fg-3">{eta}</span>}
        </div>
        <Tooltip label={cancelLabel(job.kind)}>
          <button
            aria-label={cancelLabel(job.kind)}
            onClick={cancel(job)}
            className="flex size-6 shrink-0 items-center justify-center rounded-full text-fg-3 hover:bg-white/[0.08] hover:text-fg-1"
          >
            <X className="size-3" />
          </button>
        </Tooltip>
      </div>
    )
  }
  if (m.status !== "ready" || job) {
    return (
      <span className="inline-flex items-center gap-2 text-sm">
        <span className="inline-flex items-center gap-1.5 text-fg-3">
          <Clock className="size-3.5" /> {queuedLabel(m, job)}
        </span>
        {job && (
          <button
            aria-label={cancelLabel(job.kind)}
            onClick={cancel(job)}
            className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-3 hover:text-fg-1"
          >
            <X className="size-3" /> {t("common:action.cancel")}
          </button>
        )}
      </span>
    )
  }
  if (m.shots === "failed") {
    return (
      <span className="inline-flex items-center gap-2 text-sm">
        <Tooltip label={m.shotsError ?? t("project:status.shotsFailed")}>
          <span className="inline-flex items-center gap-1.5 text-danger">
            <Warning className="size-3.5" /> {t("project:status.analysisFailed")}
          </span>
        </Tooltip>
        <button onClick={analyze} className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-3 hover:text-fg-1">
          <ArrowCounterClockwise className="size-3" /> {t("common:action.retry")}
        </button>
      </span>
    )
  }
  if (m.shots !== "ready") {
    return (
      <span className="inline-flex items-center gap-2 text-sm">
        <span className="inline-flex items-center gap-1.5 text-fg-2">
          <Check className="size-3.5 text-fg-3" /> {m.shots === "stopped" ? t("project:status.stopped") : t("project:status.indexed")}
        </span>
        <button onClick={analyze} className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-3 hover:text-fg-1">
          <Scissors className="size-3" /> {m.shots === "stopped" ? t("project:status.continue") : t("project:status.findShots")}
        </button>
      </span>
    )
  }
  const shots = (
    <span className="inline-flex items-center gap-1.5 text-fg-2">
      <Check className="size-3.5 text-fg-3" /> <Trans ns="project" i18nKey="status.shots" count={m.shotCount ?? 0} components={MONO} />
    </span>
  )
  if (m.faces === "ready") {
    return (
      <span className="inline-flex flex-col text-sm">
        {shots}
        <span className="pl-5 text-xs text-fg-3">
          <Trans ns="project" i18nKey="status.faces" count={m.faceCount ?? 0} components={MONO} />
        </span>
        {people && m.bodies && m.bodies !== "ready" && <PeopleLine media={m} onContinue={analyze} />}
      </span>
    )
  }
  if (m.faces === "failed") {
    return (
      <span className="inline-flex flex-col gap-0.5 text-sm">
        {shots}
        <span className="inline-flex items-center gap-2 pl-5 text-xs">
          <Tooltip label={m.facesError ?? t("project:status.charactersFailedWhy")}>
            <span className="inline-flex items-center gap-1 text-danger">
              <Warning className="size-3" /> {t("project:status.charactersFailed")}
            </span>
          </Tooltip>
          <button onClick={analyze} className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-3 hover:text-fg-1">
            <ArrowCounterClockwise className="size-3" /> {t("common:action.retry")}
          </button>
        </span>
      </span>
    )
  }
  return (
    <span className="inline-flex flex-col gap-0.5 text-sm">
      {shots}
      <button onClick={analyze} className="inline-flex items-center gap-1 rounded-xs pl-5 text-left text-xs text-fg-3 hover:text-fg-1">
        <UserFocus className="size-3" /> {m.faces === "stopped" ? t("project:status.stoppedContinue") : t("project:status.findCharacters")}
      </button>
    </span>
  )
}

/**
 * The people pass of a media whose faces are found, when it didn't finish
 * and nothing runs it (it failed, was stopped, or was cut short): it goes on
 * from where it was.
 */
function PeopleLine({ media: m, onContinue }: { media: MediaItem; onContinue: () => void }) {
  useTranslation("project")
  const failed = m.bodies === "failed"
  return (
    <span className="inline-flex items-center gap-2 pl-5 text-xs">
      {failed && (
        <Tooltip label={m.bodiesError ?? t("project:status.peopleFailedWhy")}>
          <span className="inline-flex items-center gap-1 text-danger">
            <Warning className="size-3" /> {t("project:status.peopleFailed")}
          </span>
        </Tooltip>
      )}
      <button onClick={onContinue} className="inline-flex items-center gap-1 rounded-xs px-1 text-fg-3 hover:text-fg-1">
        {failed ? <ArrowCounterClockwise className="size-3" /> : <User className="size-3" />}
        {failed ? t("common:action.retry") : m.bodies === "stopped" ? t("project:status.peopleStoppedContinue") : t("project:status.findPeople")}
      </button>
    </span>
  )
}
