import type * as React from "react"
import { useState } from "react"
import { Link } from "@tanstack/react-router"
import { Trans, useTranslation } from "react-i18next"
import { ArrowCounterClockwise, ArrowLeft, CircleNotch, Clock, MagnifyingGlass, X } from "@/components/icons"
import { toast } from "sonner"
import type { FrameCount, JobInfo, MediaItem } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { locateFile } from "@/features/project/relink"
import { formatCount, formatEta, formatPace, formatPercent, formatProgress } from "@/lib/format"
import { say } from "@/lib/i18n"
import { commands, errorMessage, isAppError } from "@/lib/ipc"
import { cancelJob, cancelLabel } from "@/lib/jobs"
import { assetUrl } from "@/lib/media"
import { cn } from "@/lib/utils"
import { jobBar, useMediaJob } from "@/state/activity"
import { useIndexingStill } from "./indexing-still"

/** Poster with what's happening instead of a player. */
export function NotReady({
  pid,
  media: m,
  error,
  gone,
  listError,
  onRetryList,
}: {
  pid: string
  media?: MediaItem
  error: unknown
  /** The project's episodes loaded, and this one isn't among them. */
  gone: boolean
  /** The episodes couldn't be loaded. */
  listError: unknown
  onRetryList: () => void
}) {
  const { t } = useTranslation(["player", "common"])
  const needsPreview = m?.status === "ready" && isAppError(error) && error.code === "not_ready"
  // The job playback waits for — its indexing or its preview — not an
  // analysis running meanwhile: its progress shows, and Cancel stops it.
  const ingest = useMediaJob(m?.id, "ingest")
  const preview = useMediaJob(m?.id, "proxy")
  const job = needsPreview ? preview : ingest
  const indexing = (m?.status === "queued" || m?.status === "indexing") && job?.kind === "ingest"
  // Where the indexing is, as a picture, asked for while it reads the frames
  // (there is no place to show before): it lights the room, as a poster would.
  const still = useIndexingStill(pid, m?.id, indexing && job.state === "running" && job.frames != null)
  const backdrop = still ?? (m?.poster ? assetUrl(m.posterSmall ?? m.poster) : null)
  let title: string = t("notReady.loading")
  let body: React.ReactNode = null
  if (listError) {
    title = t("notReady.listFailed")
    body = errorMessage(listError)
  } else if (gone) {
    title = t("notReady.goneTitle")
    body = t("notReady.goneBody")
  } else if (m?.status === "queued" || m?.status === "indexing") {
    title = t("notReady.indexingTitle")
    body = t("notReady.indexingBody")
  } else if (m?.status === "failed") {
    title = t("notReady.indexFailed")
    body = m.error == null ? null : say(m.error)
  } else if (m?.status === "missing") {
    title = t("notReady.missingTitle")
    body = t("notReady.missingBody", { path: m.path })
  } else if (needsPreview) {
    title = job
      ? t("notReady.previewing")
      : m?.proxy === "failed"
        ? t("notReady.previewFailed")
        : t("notReady.previewMissing")
    body = job ? t("notReady.previewBody") : errorMessage(error)
  } else if (error) {
    title = t("notReady.cantPlay")
    body = errorMessage(error)
  }
  const retryable = m?.status === "failed"
  return (
    <div className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden">
      {backdrop && (
        <div aria-hidden className="absolute inset-0 scale-110 opacity-30 blur-2xl">
          <Crossfade src={backdrop} />
        </div>
      )}
      <div className="glass-strong relative flex w-[min(460px,90%)] flex-col items-center rounded-3xl px-8 py-8 text-center">
        {indexing ? (
          <div className="picture-edge artwork-fallback relative grid aspect-video w-64 place-items-center overflow-hidden rounded-xl">
            {!still && <Busy waiting={job.state === "queued"} />}
            <Crossfade src={still} />
          </div>
        ) : job ? (
          <Busy waiting={job.state === "queued"} />
        ) : null}
        <h2 className={cn("display text-3xl text-fg-1", indexing ? "mt-5" : "mt-3")}>{title}</h2>
        {body && <p className="mt-2 text-md break-words text-fg-2">{body}</p>}
        {job && (
          <div className="mt-6 w-full">
            <Progress value={jobBar(job)} />
            {job.state === "running" && job.frames ? (
              <FramesRead job={job} frames={job.frames} />
            ) : (
              <div className="mt-2 flex justify-between gap-2 text-xs text-fg-3">
                <span className="truncate">{job.state === "queued" ? t("common:state.waiting") : say(job.stage ?? "")}</span>
                <span className="timecode shrink-0">{formatProgress(job.progress, job.etaMs)}</span>
              </div>
            )}
            {/* Indexing keeps a line for its pace, so the card doesn't jump as its frames start and end. */}
            {indexing && !(job.state === "running" && job.frames) && <div aria-hidden className="mt-1 h-4" />}
            <Button className="mt-5" size="sm" onClick={() => cancelJob(job)}>
              <X /> {cancelLabel(job.kind)}
            </Button>
          </div>
        )}
        {!job && needsPreview && (
          <Button
            className="mt-6"
            variant="primary"
            onClick={() => void commands.mediaPreparePreview(pid, m!.id).catch((e) => toast.error(errorMessage(e)))}
          >
            <ArrowCounterClockwise /> {t("notReady.preparePreview")}
          </Button>
        )}
        {listError ? (
          <Button className="mt-6" variant="primary" onClick={onRetryList}>
            <ArrowCounterClockwise /> {t("common:action.tryAgain")}
          </Button>
        ) : null}
        {gone && (
          <Button className="mt-6" variant="primary" asChild>
            <Link to="/p/$pid/episodes" params={{ pid }}>
              <ArrowLeft /> {t("notReady.backToEpisodes")}
            </Link>
          </Button>
        )}
        {m?.status === "missing" && (
          <Button className="mt-6" variant="primary" onClick={() => void locateFile(pid, m)}>
            <MagnifyingGlass /> {t("notReady.locate")}
          </Button>
        )}
        {!job && retryable && (
          <Button
            className="mt-6"
            variant="primary"
            onClick={() => void commands.mediaRetry(pid, m.id).catch((e) => toast.error(errorMessage(e)))}
          >
            <ArrowCounterClockwise /> {t("common:action.tryAgain")}
          </Button>
        )}
      </div>
    </div>
  )
}

/** Work under way spins; work that waits its turn stands still, as its bar does. */
function Busy({ waiting }: { waiting: boolean }) {
  return waiting ? (
    <Clock className="size-6 text-fg-3" />
  ) : (
    <CircleNotch className="size-6 animate-spin text-accent" />
  )
}

/**
 * The frames an indexing has read, of about how many, and how fast. The
 * pace and the time left show once they are measured, never guessed: a
 * first burst says little about either.
 */
function FramesRead({ job, frames }: { job: JobInfo; frames: FrameCount }) {
  const { t } = useTranslation(["player", "common"])
  const { done, total, rate, perSec } = frames
  return (
    <div className="mt-2 flex flex-col gap-1 text-xs text-fg-3">
      <div className="flex justify-between gap-2">
        <span className="truncate">
          {/* The file's own count can be off: past it, only what was read is told. */}
          {total != null && total >= done ? (
            <Trans
              t={t}
              i18nKey="notReady.framesOfAbout"
              count={total}
              values={{ done: formatCount(done) }}
              components={{ x: <span className="timecode text-fg-2" />, mono: <span className="timecode" /> }}
            />
          ) : (
            <Trans t={t} i18nKey="notReady.framesRead" count={done} components={{ x: <span className="timecode text-fg-2" /> }} />
          )}
        </span>
        <span className="timecode shrink-0">{formatPercent(job.progress)}</span>
      </div>
      <div className="flex h-4 justify-between gap-2">
        {perSec == null ? (
          <span className="truncate">{t("notReady.measuring")}</span>
        ) : (
          <span className="truncate">
            {rate && perSec > 0 ? (
              <Trans
                t={t}
                i18nKey="notReady.paceReal"
                values={{ pace: formatPace(perSec), speed: formatPace(perSec / rate) }}
                components={{ mono: <span className="timecode" /> }}
              />
            ) : (
              <Trans t={t} i18nKey="notReady.pace" values={{ pace: formatPace(perSec) }} components={{ mono: <span className="timecode" /> }} />
            )}
          </span>
        )}
        <span className="timecode shrink-0">{perSec == null ? "" : formatEta(job.etaMs)}</span>
      </div>
    </div>
  )
}

/** `src` fading in over the picture before it once loaded (the first one over nothing). */
function Crossfade({ src }: { src: string | null }) {
  const [loaded, setLoaded] = useState<{ now: string | null; before: string | null }>({ now: null, before: null })
  // The one on screen stays underneath until `src` has faded in over it.
  const under = loaded.now !== src ? loaded.now : loaded.before
  return (
    <>
      {under && (
        <img key={under} src={under} alt="" draggable={false} className="absolute inset-0 size-full object-cover" />
      )}
      {src && (
        <img
          key={src}
          src={src}
          alt=""
          draggable={false}
          decoding="async"
          onLoad={() => setLoaded((l) => (l.now === src ? l : { now: src, before: l.now }))}
          className={cn(
            "absolute inset-0 size-full object-cover transition-opacity duration-[var(--dur-slow)] ease-out",
            loaded.now === src ? "opacity-100" : "opacity-0",
          )}
        />
      )}
    </>
  )
}
