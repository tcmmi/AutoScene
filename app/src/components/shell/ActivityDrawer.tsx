import type * as React from "react"
import { useNavigate } from "@tanstack/react-router"
import { toast } from "sonner"
import { useTranslation } from "react-i18next"
import {
  Check,
  Copy,
  Eye,
  FilmStrip,
  Image,
  Package,
  Play,
  Prohibit,
  PuzzlePiece,
  Queue,
  Scan,
  Scissors,
  SquaresFour,
  Trash,
  UserFocus,
  Users,
  Warning,
  X,
} from "@/components/icons"
import type { JobInfo } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { EmptyState } from "@/components/ui/empty-state"
import { Progress } from "@/components/ui/progress"
import { Sheet, SheetContent } from "@/components/ui/sheet"
import { Tooltip, TruncatedText } from "@/components/ui/tooltip"
import { formatProgress } from "@/lib/format"
import { say } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { cancelJob, stopWord } from "@/lib/jobs"
import { copyText } from "@/lib/os"
import { cn } from "@/lib/utils"
import { isFinished, jobBar, useActivity, useJobs } from "@/state/activity"
import { useUi } from "@/state/ui"

const KIND_ICON: Record<JobInfo["kind"], React.ReactNode> = {
  ingest: <FilmStrip />,
  proxy: <Eye />,
  export: <Scissors />,
  analyze: <Scan />,
  posters: <Image />,
  portraits: <UserFocus />,
  regroup: <Users />,
  plugin: <PuzzlePiece />,
  rescan: <Scan />,
}

/** Global view of background work (indexing, previews, analysis, exports, plugins). */
export function ActivityDrawer() {
  const { t } = useTranslation("shell")
  const open = useUi((s) => s.activityOpen)
  const setOpen = useUi((s) => s.setActivityOpen)
  const jobs = useJobs()
  const active = jobs.filter((j) => !isFinished(j))
  const finished = jobs.filter(isFinished)

  const clear = () => {
    void commands
      .jobsClearFinished()
      .catch((e) => toast.error(t("activity.clearFailed"), { description: errorMessage(e) }))
    useActivity.getState().clearFinished()
  }

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetContent
        title={t("activity.title")}
        actions={
          finished.length > 0 && (
            <Button size="sm" variant="ghost" onClick={clear}>
              {t("activity.clearFinished")}
            </Button>
          )
        }
      >
        {jobs.length === 0 ? (
          <EmptyState
            icon={<Queue />}
            title={t("activity.empty.title")}
            description={t("activity.empty.description")}
            className="h-full"
          />
        ) : (
          <ContextArea
            menu={
              finished.length > 0 && (
                <ContextMenuItem onSelect={clear}>
                  <Trash /> {t("activity.clearFinished")}
                </ContextMenuItem>
              )
            }
          >
            <div className="flex flex-col">
              {active.length > 0 && <Group title={t("activity.group.inProgress")} jobs={active} />}
              {finished.length > 0 && <Group title={t("activity.group.finished")} jobs={finished} />}
            </div>
          </ContextArea>
        )}
      </SheetContent>
    </Sheet>
  )
}

function Group({ title, jobs }: { title: string; jobs: JobInfo[] }) {
  return (
    <section className="px-3 pb-3">
      <h3 className="eyebrow px-2 pt-4 pb-2">{title}</h3>
      <ul className="flex flex-col gap-2">
        {jobs.map((j) => (
          <JobRow key={j.id} job={j} />
        ))}
      </ul>
    </section>
  )
}

function JobRow({ job: j }: { job: JobInfo }) {
  const { t } = useTranslation("shell")
  const navigate = useNavigate()
  const done = isFinished(j)
  const canOpen = j.kind === "export" && j.projectId
  const open = () => {
    if (!canOpen) return
    useUi.getState().setActivityOpen(false)
    void navigate({ to: "/p/$pid/exports", params: { pid: j.projectId! } })
  }
  const go = (to: () => Promise<void>) => {
    useUi.getState().setActivityOpen(false)
    void to()
  }
  const pid = j.projectId
  const mid = j.mediaId
  const stop = stopWord(j.kind)
  const title = say(j.title)
  const menu = (
    <>
      {!done && (
        <ContextMenuItem onSelect={() => cancelJob(j)}>
          <X /> {stop}
        </ContextMenuItem>
      )}
      {canOpen && (
        <ContextMenuItem onSelect={open}>
          <Package /> {t("activity.showExports")}
        </ContextMenuItem>
      )}
      {pid && mid && j.kind !== "export" && (
        <ContextMenuItem onSelect={() => go(() => navigate({ to: "/p/$pid/timeline/$mid", params: { pid, mid } }))}>
          <Play /> {t("activity.openPlayer")}
        </ContextMenuItem>
      )}
      {pid && j.kind === "rescan" && (
        <ContextMenuItem onSelect={() => go(() => navigate({ to: "/p/$pid/cast/rescan", params: { pid } }))}>
          <Scan /> {t("activity.openRescan")}
        </ContextMenuItem>
      )}
      {pid && (j.kind === "portraits" || j.kind === "regroup") && (
        <ContextMenuItem onSelect={() => go(() => navigate({ to: "/p/$pid/cast", params: { pid } }))}>
          <Users /> {t("activity.openCast")}
        </ContextMenuItem>
      )}
      {pid && !mid && j.kind === "posters" && (
        <ContextMenuItem onSelect={() => go(() => navigate({ to: "/p/$pid", params: { pid } }))}>
          <SquaresFour /> {t("activity.openProject")}
        </ContextMenuItem>
      )}
      {j.state === "failed" && j.error && (
        <ContextMenuItem onSelect={() => void copyText(say(j.error!), "error")}>
          <Copy /> {t("activity.copyError")}
        </ContextMenuItem>
      )}
      {j.state === "done" && j.message && (
        <ContextMenuItem onSelect={() => void copyText(say(j.message!), "message")}>
          <Copy /> {t("activity.copyMessage")}
        </ContextMenuItem>
      )}
    </>
  )
  return (
    <ContextArea menu={menu}>
      <li
        onClick={canOpen ? open : undefined}
        tabIndex={canOpen ? 0 : undefined}
        role={canOpen ? "link" : undefined}
        onKeyDown={(e) => {
          if (canOpen && e.key === "Enter" && e.target === e.currentTarget) open()
        }}
        className={cn(
          "group flex gap-3 rounded-2xl border border-white/[0.06] bg-white/[0.03] p-3.5 transition-colors focus-visible:bg-white/[0.06] focus-visible:outline-none",
          canOpen && "hover:bg-white/[0.06]",
        )}
      >
        <span
          className={cn(
            "flex size-9 shrink-0 items-center justify-center rounded-xl [&_svg]:size-[18px]",
            j.state === "running" ? "bg-accent-subtle text-accent" : "bg-white/[0.06] text-fg-3",
          )}
        >
          {KIND_ICON[j.kind]}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-start gap-2">
            <div className="min-w-0 flex-1">
              <TruncatedText className="text-md font-medium text-fg-1">{title}</TruncatedText>
              {j.subtitle && <div className="truncate text-sm text-fg-3">{j.subtitle}</div>}
            </div>
            {!done ? (
              <Tooltip label={stop}>
                <button
                  aria-label={j.kind === "plugin" ? t("activity.stopJob", { title }) : t("activity.cancelJob", { title })}
                  onClick={(e) => {
                    e.stopPropagation()
                    cancelJob(j)
                  }}
                  className="flex size-7 items-center justify-center rounded-full text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-white/[0.1] hover:text-fg-1 focus-visible:opacity-100"
                >
                  <X className="size-3.5" />
                </button>
              </Tooltip>
            ) : (
              <StateIcon job={j} />
            )}
          </div>
          {!done && (
            <div className="mt-3">
              <Progress value={jobBar(j)} />
              <div className="mt-1.5 flex justify-between gap-2 text-xs text-fg-3">
                <span className="truncate">{j.state === "queued" ? t("activity.waiting") : (j.stage ? say(j.stage) : t("activity.working"))}</span>
                <span className="timecode shrink-0">{formatProgress(j.progress, j.etaMs)}</span>
              </div>
            </div>
          )}
          {j.state === "failed" && j.error && <p className="mt-1.5 line-clamp-3 text-sm break-words text-danger">{say(j.error)}</p>}
          {j.state === "done" && j.message && <p className="mt-1.5 line-clamp-3 text-sm break-words text-fg-2">{say(j.message)}</p>}
        </div>
      </li>
    </ContextArea>
  )
}

/** How a finished job ended: a small round badge, centred on its title and subtitle. */
function StateIcon({ job }: { job: JobInfo }) {
  const { t } = useTranslation("shell")
  const badge = "flex size-6 shrink-0 items-center justify-center self-center rounded-full [&_svg]:size-3.5"
  if (job.state === "done") {
    return (
      <span role="img" aria-label={t("activity.done")} className={cn(badge, "bg-ok/15 text-ok")}>
        <Check weight="bold" />
      </span>
    )
  }
  if (job.state === "failed") {
    return (
      <span role="img" aria-label={t("activity.failed")} className={cn(badge, "bg-danger-subtle text-danger")}>
        <Warning weight="bold" />
      </span>
    )
  }
  return (
    <span role="img" aria-label={t("activity.cancelled")} className={cn(badge, "bg-white/[0.06] text-fg-3")}>
      <Prohibit weight="bold" />
    </span>
  )
}
