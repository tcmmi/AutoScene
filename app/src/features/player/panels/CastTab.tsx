import { useCallback, useMemo, useState } from "react"
import { Link, useNavigate } from "@tanstack/react-router"
import type { TFunction } from "i18next"
import { useTranslation } from "react-i18next"
import {
  ArrowCounterClockwise,
  CaretDown,
  CaretLeft,
  CaretRight,
  Copy,
  FilmStrip,
  SkipForward,
  User,
  UserFocus,
  Users,
} from "@/components/icons"
import { toast } from "sonner"
import type { MediaItem, Segment } from "@/bindings"
import { Avatar } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { Progress } from "@/components/ui/progress"
import { Tooltip } from "@/components/ui/tooltip"
import { formatPercent } from "@/lib/format"
import { say } from "@/lib/i18n"
import { commands, errorMessage } from "@/lib/ipc"
import { copyText } from "@/lib/os"
import { type Lane, charactersIn, nextAppearance } from "@/lib/presence"
import { nextShotStart, prevShotStart, segmentIndexAt, shotAt, shotNumbers } from "@/lib/segments"
import { useShortcutKeys } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"
import { jobBar, useMediaJob } from "@/state/activity"
import { type FrameStore, useFrameSelector } from "../frame-store"
import { characterName } from "../words"
import { ShotHeader } from "./ShotHeader"
import { TabBody } from "./TabBody"

/** What the Cast tab says, and offers, while shots or characters aren't found and no analysis runs. */
type Step = {
  /** Where it stands: the sentence and the button are the catalog's, by state. */
  state: "shotsFailed" | "shotsStopped" | "shotsMissing" | "facesFailed" | "facesStopped" | "facesMissing"
  /** A failure: the sentence is in the danger colour. */
  failed: boolean
  /** The error the stage left, under the sentence. */
  detail: string | null
  /** The sentence the app gave for a failure, in place of the catalog's. */
  reason: string | null
}

/** A step's sentence, its one button (it goes on with the first stage that isn't ready) and the same in the tab's menu, where no sentence says what it goes on with. */
function stepWords(t: TFunction<["player", "common"]>, step: Step): { text: string; action: string; entry: string } {
  switch (step.state) {
    case "shotsFailed":
      return { text: t("castTab.shotsFailed"), action: t("common:action.tryAgain"), entry: t("castTab.shotsAgain") }
    case "shotsStopped":
      return { text: t("castTab.shotsStopped"), action: t("castTab.continue"), entry: t("castTab.shotsContinue") }
    case "shotsMissing":
      return { text: t("castTab.shotsMissing"), action: t("castTab.findAll"), entry: t("castTab.findAll") }
    case "facesFailed":
      return { text: step.reason ?? t("castTab.facesFailed"), action: t("common:action.tryAgain"), entry: t("castTab.facesAgain") }
    case "facesStopped":
      return { text: t("castTab.facesStopped"), action: t("castTab.continue"), entry: t("castTab.facesContinue") }
    case "facesMissing":
      return { text: t("castTab.facesMissing"), action: t("castTab.findCharacters"), entry: t("castTab.findCharacters") }
  }
}

/**
 * The analysis block's state, from the episode's two stages. Shots come
 * first: the one analysis job finds them, then the characters, so a missing
 * shot detection is the only thing said even when the characters are missing
 * too. `null` once both are found.
 */
function analysisStep(m: MediaItem): Step | null {
  if (m.shots !== "ready") {
    if (m.shots === "failed") {
      return { state: "shotsFailed", failed: true, detail: m.shotsError == null ? null : say(m.shotsError), reason: null }
    }
    if (m.shots === "stopped") return { state: "shotsStopped", failed: false, detail: null, reason: null }
    return { state: "shotsMissing", failed: false, detail: null, reason: null }
  }
  if (m.faces === "ready") return null
  if (m.faces === "failed") {
    return { state: "facesFailed", failed: true, detail: null, reason: m.facesError == null ? null : say(m.facesError) }
  }
  if (m.faces === "stopped") return { state: "facesStopped", failed: false, detail: null, reason: null }
  return { state: "facesMissing", failed: false, detail: null, reason: null }
}

/** The block's button and menu entry: Try again turns back, the others find. */
function StepIcon({ step }: { step: Step }) {
  return step.failed ? <ArrowCounterClockwise /> : <UserFocus />
}

/**
 * The Cast tab: the shot under the playhead, where the analysis stands while
 * something isn't found yet, who is in the shot and everyone in this
 * episode. It draws again when the shot or who is on screen changes, not on
 * every frame.
 */
export function CastTab({
  pid,
  media: m,
  lanes,
  segments,
  store,
  tc,
  focus,
  onFocus,
  onMarkShot,
  onSeek,
  onGo,
}: {
  pid: string
  media: MediaItem
  lanes: Lane[]
  segments: Segment[]
  store: FrameStore
  tc: (f: number) => string
  focus: string | null
  onFocus: (id: string | null) => void
  /** The shot under the playhead becomes the clip. */
  onMarkShot: () => void
  /** Move the playhead (the shot steps: playing goes on). */
  onSeek: (f: number) => void
  /** Jump to a character's appearance (it pauses there). */
  onGo: (f: number) => void
}) {
  const { t } = useTranslation(["player", "common"])
  const keys = useShortcutKeys()
  const job = useMediaJob(m.id, "analyze")
  const navigate = useNavigate()
  const [all, setAll] = useState(false)

  // The shot under the playhead.
  const shotsReady = m.shots === "ready"
  const numbers = useMemo(() => shotNumbers(segments), [segments])
  const total = numbers.reduce((n, x) => (x >= 0 ? n + 1 : n), 0)
  const i = useFrameSelector(
    store,
    useCallback((f: number) => segmentIndexAt(segments, f), [segments]),
  )
  const seg = shotsReady && i >= 0 ? segments[i] : null
  const prevShot = () => {
    const to = prevShotStart(segments, store.get())
    if (to != null) onSeek(to)
  }
  const nextShot = () => {
    const to = nextShotStart(segments, store.get())
    if (to != null) onSeek(to)
  }

  // Who is on screen: in the shot under the playhead (or at the frame, before shots are found).
  const hereKey = useFrameSelector(
    store,
    useCallback(
      (f: number) => {
        const shot = shotAt(segments, f)
        const on = shot ? charactersIn(lanes, shot.start, shot.end) : charactersIn(lanes, f, f + 1)
        return on.map((l) => l.character.id).join(",")
      },
      [lanes, segments],
    ),
  )
  const hereIds = new Set(hereKey ? hereKey.split(",") : [])
  const here = lanes.filter((l) => hereIds.has(l.character.id))
  // Who is in the shot, then the episode's main characters; the rest on request.
  const others = lanes.filter((l) => !hereIds.has(l.character.id))
  const listed = [...here, ...(all ? others : others.slice(0, Math.max(0, 8 - here.length)))]
  const hiddenCount = here.length + others.length - listed.length
  const facesReady = m.faces === "ready"
  const canShowMore = facesReady && (hiddenCount > 0 || all)
  const next = (lane: Lane) => nextAppearance(lane, store.get())
  const openCast = () => void navigate({ to: "/p/$pid/cast", params: { pid } })

  // One command for every state: it goes on with the first stage that isn't
  // ready, and the one analysis job then finds the shots and the characters.
  // Nothing to press while that job is queued or running.
  const analyze = () =>
    void commands
      .mediaAnalyze(pid, m.id, null)
      .catch((e) => toast.error(t("castTab.startFailed"), { description: errorMessage(e) }))
  const step = job ? null : analysisStep(m)
  const words = step ? stepWords(t, step) : null

  const menu = (
    <>
      {seg && (
        <>
          {seg.kind === "shot" && (
            <ContextMenuItem onSelect={onMarkShot} shortcut={keys("player.markShot")}>
              <FilmStrip /> {t("shot.markAsClip")}
            </ContextMenuItem>
          )}
          <ContextMenuItem onSelect={prevShot} shortcut={keys("player.shot", 0)}>
            <CaretLeft /> {t("shot.previous")}
          </ContextMenuItem>
          <ContextMenuItem onSelect={nextShot} shortcut={keys("player.shot", 1)}>
            <CaretRight /> {t("shot.next")}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => void copyText(`${tc(seg.start)} - ${tc(seg.end - 1)}`, "timecodes")}>
            <Copy /> {t("shot.copyTimecodes")}
          </ContextMenuItem>
          <ContextMenuSeparator />
        </>
      )}
      {step && (
        <>
          <ContextMenuItem onSelect={analyze}>
            <StepIcon step={step} /> {words!.entry}
          </ContextMenuItem>
          <ContextMenuSeparator />
        </>
      )}
      <ContextMenuItem onSelect={openCast}>
        <Users /> {t("castTab.openCast")}
      </ContextMenuItem>
      {canShowMore && (
        <ContextMenuItem onSelect={() => setAll((v) => !v)}>
          <CaretDown className={cn(all && "rotate-180")} />{" "}
          {all ? t("castTab.showFewer") : t("castTab.showMore", { count: hiddenCount })}
        </ContextMenuItem>
      )}
    </>
  )

  // Where the analysis stands, while shots or characters aren't found yet.
  const analysis =
    shotsReady && facesReady ? null : job ? (
      <div>
        <div className="flex justify-between gap-3 text-xs text-fg-3">
          <span className="min-w-0 truncate">
            {job.state === "queued"
              ? t("common:state.waiting")
              : job.stage != null
                ? say(job.stage)
                : shotsReady
                  ? t("castTab.findingCharacters")
                  : t("castTab.findingShots")}
          </span>
          <span className="timecode">{formatPercent(job.progress)}</span>
        </div>
        <Progress className="mt-1.5" value={jobBar(job)} />
      </div>
    ) : step && words ? (
      <div>
        <p className={cn("text-sm break-words", step.failed ? "text-danger" : "text-fg-3")}>{words.text}</p>
        {step.detail && <p className="mt-0.5 text-xs break-words text-fg-3">{step.detail}</p>}
        <Button size="sm" variant="ghost" className="mt-2 -ml-2" onClick={analyze}>
          <StepIcon step={step} /> {words.action}
        </Button>
      </div>
    ) : null

  return (
    <TabBody label={t("details.cast")} menu={menu}>
      {seg && (
        <ShotHeader
          seg={seg}
          number={numbers[i]}
          total={total}
          tc={tc}
          onMark={onMarkShot}
          onPrev={prevShot}
          onNext={nextShot}
        />
      )}
      {analysis && <div className={cn(seg && "mt-5")}>{analysis}</div>}
      {facesReady &&
        (lanes.length === 0 ? (
          <p className={cn("text-sm text-fg-3", (seg || analysis) && "mt-5")}>{t("castTab.nobody")}</p>
        ) : (
          <ul className={cn("-mx-1.5 flex flex-col gap-0.5", (seg || analysis) && "mt-5")}>
            {listed.map((l) => {
              const c = l.character
              const on = hereIds.has(c.id)
              return (
                <li key={c.id}>
                  <Tooltip label={characterName(c)} hint={t("castTab.listHint")} side="left" delay={600}>
                    <ContextArea
                      menu={
                        <>
                          <ContextMenuItem onSelect={() => onFocus(focus === c.id ? null : c.id)}>
                            <UserFocus /> {focus === c.id ? t("timeline.stopHighlighting") : t("timeline.highlight")}
                          </ContextMenuItem>
                          <ContextMenuItem
                            onSelect={() => {
                              const f = next(l)
                              if (f != null) onGo(f)
                            }}
                          >
                            <SkipForward /> {t("timeline.nextAppearance")}
                          </ContextMenuItem>
                          <ContextMenuSeparator />
                          <ContextMenuItem onSelect={() => void navigate({ to: "/p/$pid/cast/$cid", params: { pid, cid: c.id } })}>
                            <User /> {t("timeline.openInCast")}
                          </ContextMenuItem>
                        </>
                      }
                    >
                      <button
                        onClick={() => onFocus(focus === c.id ? null : c.id)}
                        onDoubleClick={() => {
                          const f = next(l)
                          if (f != null) onGo(f)
                        }}
                        className={cn(
                          "flex w-full items-center gap-2.5 rounded-xl px-1.5 py-1.5 text-left transition-colors hover:bg-white/[0.06]",
                          focus === c.id && "bg-accent-subtle",
                        )}
                      >
                        <Avatar path={c.coverSmall} ring={on} className={cn(!on && "opacity-50")} />
                        <span className={cn("min-w-0 flex-1 truncate text-sm font-medium", on ? "text-fg-1" : "text-fg-3")}>{characterName(c)}</span>
                        {on && <span className="text-xs text-accent">{t("castTab.inShot")}</span>}
                      </button>
                    </ContextArea>
                  </Tooltip>
                </li>
              )
            })}
            {canShowMore && (
              <li>
                <button
                  onClick={() => setAll((v) => !v)}
                  className="mt-0.5 w-full rounded-xl px-1.5 py-1.5 text-left text-xs font-medium text-fg-3 hover:bg-white/[0.06] hover:text-fg-1"
                >
                  {all ? t("castTab.showFewer") : t("castTab.moreInEpisode", { count: hiddenCount })}
                </button>
              </li>
            )}
          </ul>
        ))}
      <Button size="sm" variant="ghost" className="mt-5 -ml-2" asChild>
        <Link to="/p/$pid/cast" params={{ pid }}>
          <Users /> {t("castTab.openCast")}
        </Link>
      </Button>
    </TabBody>
  )
}
