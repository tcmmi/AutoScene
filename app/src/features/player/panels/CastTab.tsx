import { useCallback, useMemo, useState } from "react"
import { Link, useNavigate } from "@tanstack/react-router"
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
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { Progress } from "@/components/ui/progress"
import { Tooltip } from "@/components/ui/tooltip"
import { formatPercent } from "@/lib/format"
import { commands, errorMessage } from "@/lib/ipc"
import { assetUrl } from "@/lib/media"
import { copyText } from "@/lib/os"
import { type Lane, charactersIn, nextAppearance } from "@/lib/presence"
import { nextShotStart, prevShotStart, segmentIndexAt, shotAt, shotNumbers } from "@/lib/segments"
import { useShortcutKeys } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"
import { useMediaJob } from "@/state/activity"
import { type FrameStore, useFrameSelector } from "../frame-store"
import { ShotHeader } from "./ShotHeader"
import { TabBody } from "./TabBody"

/** What the Cast tab says, and offers, while shots or characters aren't found and no analysis runs. */
type Step = {
  /** Where it stands, in a sentence. */
  text: string
  /** A failure: the sentence is in the danger colour. */
  failed: boolean
  /** The error the stage left, under the sentence. */
  detail: string | null
  /** The one button: it goes on with the first stage that isn't ready. */
  action: "Find shots and characters" | "Find characters" | "Try again" | "Continue"
  /** The same in the tab's menu, where no sentence says what it goes on with. */
  entry: string
}

/**
 * The analysis block's state, from the episode's two stages. Shots come
 * first: the one analysis job finds them, then the characters, so a missing
 * shot detection is the only thing said even when the characters are missing
 * too. `null` once both are found.
 */
function analysisStep(m: MediaItem): Step | null {
  if (m.shots !== "ready") {
    if (m.shots === "failed")
      return { text: "Shot detection failed.", failed: true, detail: m.shotsError, action: "Try again", entry: "Find shots again" }
    if (m.shots === "stopped")
      return { text: "Finding shots was stopped.", failed: false, detail: null, action: "Continue", entry: "Continue finding shots" }
    const find = "Find shots and characters"
    return { text: "Shots haven't been detected yet.", failed: false, detail: null, action: find, entry: find }
  }
  if (m.faces === "ready") return null
  if (m.faces === "failed") {
    const text = m.facesError ?? "Finding characters failed."
    return { text, failed: true, detail: null, action: "Try again", entry: "Find characters again" }
  }
  if (m.faces === "stopped")
    return { text: "Finding characters was stopped.", failed: false, detail: null, action: "Continue", entry: "Continue finding characters" }
  const find = "Find characters"
  return { text: "Characters haven't been found in this episode yet.", failed: false, detail: null, action: find, entry: find }
}

/** The block's button and menu entry: Try again turns back, the others find. */
function StepIcon({ step }: { step: Step }) {
  return step.action === "Try again" ? <ArrowCounterClockwise /> : <UserFocus />
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
      .catch((e) => toast.error("Couldn't start the analysis", { description: errorMessage(e) }))
  const step = job ? null : analysisStep(m)

  const menu = (
    <>
      {seg && (
        <>
          {seg.kind === "shot" && (
            <ContextMenuItem onSelect={onMarkShot} shortcut={keys("player.markShot")}>
              <FilmStrip /> Mark the shot as the clip
            </ContextMenuItem>
          )}
          <ContextMenuItem onSelect={prevShot} shortcut={keys("player.shot", 0)}>
            <CaretLeft /> Previous shot
          </ContextMenuItem>
          <ContextMenuItem onSelect={nextShot} shortcut={keys("player.shot", 1)}>
            <CaretRight /> Next shot
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => void copyText(`${tc(seg.start)} - ${tc(seg.end - 1)}`, "timecodes")}>
            <Copy /> Copy its timecodes
          </ContextMenuItem>
          <ContextMenuSeparator />
        </>
      )}
      {step && (
        <>
          <ContextMenuItem onSelect={analyze}>
            <StepIcon step={step} /> {step.entry}
          </ContextMenuItem>
          <ContextMenuSeparator />
        </>
      )}
      <ContextMenuItem onSelect={openCast}>
        <Users /> Open the cast
      </ContextMenuItem>
      {canShowMore && (
        <ContextMenuItem onSelect={() => setAll((v) => !v)}>
          <CaretDown className={cn(all && "rotate-180")} /> {all ? "Show fewer" : `Show ${hiddenCount} more`}
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
            {job.state === "queued" ? "Waiting…" : (job.stage ?? (shotsReady ? "Finding characters" : "Finding shots"))}
          </span>
          <span className="timecode">{formatPercent(job.progress)}</span>
        </div>
        <Progress className="mt-1.5" value={job.state === "running" ? job.progress : null} />
      </div>
    ) : step ? (
      <div>
        <p className={cn("text-sm break-words", step.failed ? "text-danger" : "text-fg-3")}>{step.text}</p>
        {step.detail && <p className="mt-0.5 text-xs break-words text-fg-3">{step.detail}</p>}
        <Button size="sm" variant="ghost" className="mt-2 -ml-2" onClick={analyze}>
          <StepIcon step={step} /> {step.action}
        </Button>
      </div>
    ) : null

  return (
    <TabBody label="Cast" menu={menu}>
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
          <p className={cn("text-sm text-fg-3", (seg || analysis) && "mt-5")}>Nobody the cast knows appears here.</p>
        ) : (
          <ul className={cn("-mx-1.5 flex flex-col gap-0.5", (seg || analysis) && "mt-5")}>
            {listed.map((l) => {
              const c = l.character
              const on = hereIds.has(c.id)
              return (
                <li key={c.id}>
                  <Tooltip label={c.label} hint="Click to highlight · Double-click for the next appearance" side="left" delay={600}>
                    <ContextArea
                      menu={
                        <>
                          <ContextMenuItem onSelect={() => onFocus(focus === c.id ? null : c.id)}>
                            <UserFocus /> {focus === c.id ? "Stop highlighting" : "Highlight"}
                          </ContextMenuItem>
                          <ContextMenuItem
                            onSelect={() => {
                              const f = next(l)
                              if (f != null) onGo(f)
                            }}
                          >
                            <SkipForward /> Next appearance
                          </ContextMenuItem>
                          <ContextMenuSeparator />
                          <ContextMenuItem onSelect={() => void navigate({ to: "/p/$pid/cast/$cid", params: { pid, cid: c.id } })}>
                            <User /> Open in the cast
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
                        <span className={cn("size-8 shrink-0 overflow-hidden rounded-full bg-inset", on ? "ring-2 ring-accent/70" : "opacity-50")}>
                          {c.coverSmall && <img src={assetUrl(c.coverSmall)} alt="" className="size-full object-cover" />}
                        </span>
                        <span className={cn("min-w-0 flex-1 truncate text-sm font-medium", on ? "text-fg-1" : "text-fg-3")}>{c.label}</span>
                        {on && <span className="text-xs text-accent">in shot</span>}
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
                  {all ? "Show fewer" : `${hiddenCount} more in this episode`}
                </button>
              </li>
            )}
          </ul>
        ))}
      <Button size="sm" variant="ghost" className="mt-5 -ml-2" asChild>
        <Link to="/p/$pid/cast" params={{ pid }}>
          <Users /> Open the cast
        </Link>
      </Button>
    </TabBody>
  )
}
