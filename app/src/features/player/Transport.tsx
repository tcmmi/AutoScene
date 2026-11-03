import type * as React from "react"
import { useEffect, useRef } from "react"
import {
  ArrowsOutLineHorizontal,
  CaretLineLeft,
  CaretLineRight,
  Crosshair,
  Pause,
  Play,
  Scissors,
  SkipBack,
  SkipForward,
  SpeakerHigh,
  SpeakerLow,
  SpeakerSlash,
  X,
} from "@/components/icons"
import { toast } from "sonner"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuRadioGroup, ContextMenuRadioItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { Slider } from "@/components/ui/slider"
import { Tooltip } from "@/components/ui/tooltip"
import { keysText, useShortcutKeys } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"
import { type FrameStore, useFrame } from "./frame-store"
import { rateLabel } from "./shuttle"
import { type TimecodeDisplayMode, useTimecodeMode } from "./timecode-mode"
import { type Tier, counterChars, readoutChars, useTransportTier } from "./transport-tier"

/**
 * Under the picture, in one row round Play: the volume at the left edge, the
 * timecode against the transport buttons, In, Out and the clip's length on
 * their other side, and Export at the right edge. Play stays under the
 * middle of the picture, and nothing in the row moves while the episode
 * plays or shuttles: the speed has its own slot and the numbers keep one
 * width. How much shows depends on the row's width (`Tier`).
 */
export function Transport({
  store,
  last,
  rate,
  tc,
  parse,
  goto,
  onGoto,
  onSeek,
  onStep,
  onToggle,
  inFrame,
  outFrame,
  length,
  onMarkIn,
  onMarkOut,
  onClear,
  canExport,
  onExport,
  volume,
  muted,
  onVolume,
  onToggleMute,
}: {
  store: FrameStore
  last: number
  rate: number
  tc: (f: number) => string
  /** A typed timecode or frame number as a frame (relative to the one shown), or null. */
  parse: (text: string, from: number) => number | null
  goto: boolean
  onGoto: (open: boolean) => void
  onSeek: (f: number) => void
  onStep: (delta: number) => void
  onToggle: () => void
  inFrame: number | null
  outFrame: number | null
  /** Frames from in to out, both included (the whole episode while nothing is marked). */
  length: number
  onMarkIn: () => void
  onMarkOut: () => void
  onClear: () => void
  canExport: boolean
  onExport: () => void
  volume: number
  muted: boolean
  onVolume: (v: number) => void
  onToggleMute: () => void
}) {
  const keys = useShortcutKeys()
  const { mode } = useTimecodeMode()
  const hasMarks = inFrame != null || outFrame != null
  const row = useRef<HTMLDivElement>(null)
  const tier = useTransportTier(row, [hasMarks, goto, mode])
  const playing = rate !== 0
  const frames = `${length.toLocaleString("en-GB")} frames`
  const shotKey = keys("player.markShot")

  const readout = (
    <FrameReadout
      store={store}
      last={last}
      tc={tc}
      parse={parse}
      goto={goto}
      onGoto={onGoto}
      onSeek={onSeek}
      speed={rateLabel(rate)}
      tier={tier}
    />
  )

  const buttons = (
    <div className="flex shrink-0 items-center gap-1">
      <TransportButton label="First frame" keys={keys("player.ends", 0)} onClick={() => onSeek(0)}>
        <SkipBack weight="fill" />
      </TransportButton>
      <TransportButton label="Previous frame" keys={keys("player.frame", 0)} onClick={() => onStep(-1)}>
        <CaretLineLeft weight="bold" />
      </TransportButton>
      <Tooltip label={playing ? "Pause" : "Play"} shortcut={keys("player.playPause")}>
        <button
          onClick={onToggle}
          aria-label={playing ? "Pause" : "Play"}
          className="mx-2 flex size-12 items-center justify-center rounded-full bg-cta text-on-cta shadow-[0_10px_30px_-8px_rgb(255_255_255/0.35)] transition-transform duration-[var(--dur-fast)] hover:scale-105 active:scale-95 [&_svg]:size-5"
        >
          {playing ? <Pause weight="fill" /> : <Play weight="fill" className="translate-x-px" />}
        </button>
      </Tooltip>
      <TransportButton label="Next frame" keys={keys("player.frame", 1)} onClick={() => onStep(1)}>
        <CaretLineRight weight="bold" />
      </TransportButton>
      <TransportButton label="Last frame" keys={keys("player.ends", 1)} onClick={() => onSeek(last)}>
        <SkipForward weight="fill" />
      </TransportButton>
    </div>
  )

  const clip = (
    <div className="flex shrink-0 items-center gap-2">
      <MarkChip slot={0} value={inFrame} tc={tc} withTime={tier === 1} onMark={onMarkIn} onGo={onSeek} />
      <MarkChip slot={1} value={outFrame} tc={tc} withTime={tier === 1} onMark={onMarkOut} onGo={onSeek} />
      {hasMarks && tier <= 3 && (
        <>
          <Tooltip label="Length of the clip" hint={frames}>
            {/* The quiet |<->| says it's a length, not a third position. */}
            <span className="timecode inline-flex items-center gap-1 text-sm whitespace-nowrap text-fg-2">
              <ArrowsOutLineHorizontal weight="bold" aria-hidden className="size-3.5 shrink-0 text-fg-3" />
              {tc(length)}
            </span>
          </Tooltip>
          <Tooltip label="Clear in and out" shortcut={keys("player.clearMarks")}>
            <Button variant="ghost" size="icon-sm" aria-label="Clear in and out" onClick={onClear}>
              <X weight="bold" />
            </Button>
          </Tooltip>
        </>
      )}
    </div>
  )

  // Secondary, so the white Play stays the row's one bright control. The
  // span keeps the tooltip (and why it can't export yet) while it's disabled.
  const exportButton = (
    <Tooltip
      label="Export clip"
      hint={
        hasMarks
          ? `${tc(length)} · ${frames}`
          : `Mark an in or out point, or a whole shot${shotKey.length > 0 ? ` (${keysText(shotKey)})` : ""}`
      }
      shortcut={keys("player.export")}
    >
      <span className="inline-flex shrink-0">
        <Button
          variant="secondary"
          size={tier <= 2 ? "md" : "icon"}
          aria-label="Export clip"
          disabled={!canExport}
          onClick={onExport}
        >
          <Scissors weight="bold" />
          {tier <= 2 && "Export"}
        </Button>
      </span>
    </Tooltip>
  )

  if (tier === 4) {
    return (
      <div ref={row} data-transport="" data-tier={tier} className="flex h-14 items-center justify-center gap-4">
        {readout}
        {buttons}
        {clip}
        {exportButton}
      </div>
    )
  }
  return (
    <div
      ref={row}
      data-transport=""
      data-tier={tier}
      className="grid h-14 grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-x-4"
    >
      <div data-side="left" className="flex min-w-0 items-center justify-between gap-3">
        <div className="flex shrink-0 items-center gap-1.5">
          <Tooltip label={muted ? "Unmute" : "Mute"} shortcut={keys("player.mute")}>
            <button
              onClick={onToggleMute}
              aria-label={muted ? "Unmute" : "Mute"}
              className="flex size-9 items-center justify-center rounded-full text-fg-2 transition-colors hover:bg-white/[0.08] hover:text-fg-1 [&_svg]:size-[18px]"
            >
              {muted || volume === 0 ? <SpeakerSlash /> : volume < 0.5 ? <SpeakerLow /> : <SpeakerHigh />}
            </button>
          </Tooltip>
          {tier <= 2 && (
            <Slider
              value={muted ? 0 : volume}
              min={0}
              max={1}
              step={0.05}
              label="Volume"
              format={(v) => `${Math.round(v * 100)} %`}
              onChange={onVolume}
              className="w-20"
            />
          )}
        </div>
        {readout}
      </div>
      {buttons}
      <div data-side="right" className="flex min-w-0 items-center justify-between gap-3">
        {clip}
        {exportButton}
      </div>
    </div>
  )
}

/**
 * The speed slot, the timecode of the frame on screen and (tier 1) the frame
 * counter — the part that draws on every frame. Each keeps one width while
 * the episode plays, so nothing beside them moves.
 */
function FrameReadout({
  store,
  last,
  tc,
  parse,
  goto,
  onGoto,
  onSeek,
  speed,
  tier,
}: {
  store: FrameStore
  last: number
  tc: (f: number) => string
  parse: (text: string, from: number) => number | null
  goto: boolean
  onGoto: (open: boolean) => void
  onSeek: (f: number) => void
  /** "2×", "−1×" while shuttling, else null. */
  speed: string | null
  tier: Tier
}) {
  const keys = useShortcutKeys()
  const frame = useFrame(store)
  const { mode, setMode } = useTimecodeMode()
  const field = useRef<HTMLInputElement>(null)
  const large = tier <= 3
  // Select the go-to field once when it opens, not on every frame (that
  // would wipe what is being typed).
  useEffect(() => {
    if (!goto) return
    field.current?.focus()
    field.current?.select()
  }, [goto])

  const formatReadout = (f: number, m: TimecodeDisplayMode) => {
    switch (m) {
      case "elapsed":
        return tc(f)
      case "remaining": {
        const rem = Math.max(0, last - f)
        return `-${tc(rem)}`
      }
      case "frames":
        return `${f.toLocaleString("en-GB")}`
      case "percent": {
        const p = last > 0 ? (f / last) * 100 : 0
        return `${p.toFixed(1)} %`
      }
    }
  }

  const readoutLabel = (m: TimecodeDisplayMode) => {
    switch (m) {
      case "elapsed":
        return "Elapsed Timecode"
      case "remaining":
        return "Remaining Timecode"
      case "frames":
        return "Current Frame"
      case "percent":
        return "Percentage"
    }
  }

  // Go to takes the place of the speed, the readout and the counter. It
  // pauses, so no speed would show beside it.
  if (goto) {
    return (
      <input
        ref={field}
        aria-label="Go to timecode or frame"
        defaultValue={tc(frame)}
        spellCheck={false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        className={cn(
          "timecode h-10 shrink-0 rounded-xl border border-accent-line bg-white/[0.06] px-3 text-fg-1 outline-none",
          large ? "w-[190px] text-2xl" : "w-[150px] text-lg",
        )}
        onBlur={() => onGoto(false)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onGoto(false)
          if (e.key !== "Enter") return
          const f = parse(e.currentTarget.value, store.get())
          if (f == null) {
            toast.error("That isn't a timecode or frame number", {
              description: "Try 00:12:03:05, a frame like 17099, or +48 to jump ahead.",
            })
            return
          }
          onSeek(f)
          onGoto(false)
        }}
      />
    )
  }

  return (
    <div className="flex shrink-0 items-baseline gap-3">
      {/* Always there, so the readout doesn't move when the speed shows. */}
      <span data-speed="" aria-hidden={speed == null || undefined} className={cn("flex w-11 shrink-0 justify-end", speed == null && "invisible")}>
        <span className="timecode rounded-full border border-accent-line bg-accent-subtle px-2 text-xs leading-5 whitespace-nowrap text-accent">
          {speed ?? "1×"}
        </span>
      </span>
      <ContextArea
        menu={
          <>
            <ContextMenuRadioGroup value={mode} onValueChange={(v) => setMode(v as TimecodeDisplayMode)}>
              <ContextMenuRadioItem value="elapsed">Elapsed Timecode</ContextMenuRadioItem>
              <ContextMenuRadioItem value="remaining">Remaining Timecode</ContextMenuRadioItem>
              <ContextMenuRadioItem value="frames">Frame Number</ContextMenuRadioItem>
              <ContextMenuRadioItem value="percent">Percentage</ContextMenuRadioItem>
            </ContextMenuRadioGroup>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => onGoto(true)} shortcut={keys("player.goTo")}>
              Go to timecode or frame…
            </ContextMenuItem>
          </>
        }
      >
        <Tooltip label="Go to timecode or frame" hint="Right-click to show the time left, frames or percent" shortcut={keys("player.goTo")}>
          <button
            data-readout=""
            onClick={() => onGoto(true)}
            aria-label={`${readoutLabel(mode)}: ${formatReadout(frame, mode)}`}
            // Its padding (14 px in all) hangs out into the gap before it,
            // and into the one after it only where the counter follows: the
            // row's sides must hold all they show.
            style={{ minWidth: `calc(${readoutChars(mode, last, tc)}ch + 14px)` }}
            className={cn(
              "timecode -ml-2 shrink-0 rounded-xl px-2 text-right font-medium tracking-tight whitespace-nowrap text-fg-1 transition-colors hover:bg-white/[0.06]",
              large ? "text-2xl" : "text-lg",
              tier === 1 && "-mr-2",
            )}
          >
            {formatReadout(frame, mode)}
          </button>
        </Tooltip>
      </ContextArea>
      {tier === 1 && (
        <Tooltip label="This frame / the last frame">
          <span
            data-counter=""
            style={{ minWidth: `${counterChars(mode, last)}ch` }}
            className="timecode inline-block shrink-0 text-right text-sm whitespace-nowrap text-fg-3"
          >
            {mode === "frames"
              ? `/ ${last.toLocaleString("en-GB")}`
              : mode === "percent"
                ? `${frame.toLocaleString("en-GB")} / ${last.toLocaleString("en-GB")} f`
                : `${frame.toLocaleString("en-GB")} / ${last.toLocaleString("en-GB")}`}
          </span>
        </Tooltip>
      )}
    </div>
  )
}

function TransportButton({
  label,
  keys,
  onClick,
  children,
}: {
  label: string
  keys: string[]
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <Tooltip label={label} shortcut={keys}>
      <button
        onClick={onClick}
        aria-label={label}
        className="flex size-10 items-center justify-center rounded-full text-fg-2 transition-colors hover:bg-white/[0.08] hover:text-fg-1 [&_svg]:size-[18px]"
      >
        {children}
      </button>
    </Tooltip>
  )
}

/**
 * The in (slot 0) or out point (slot 1): a quiet chip that marks it, gold
 * once set, and then a click goes there. Its timecode shows in the chip on
 * the widest row (`withTime`), and always in its tooltip and its name.
 */
function MarkChip({
  slot,
  value,
  tc,
  withTime,
  onMark,
  onGo,
}: {
  slot: number
  value: number | null
  tc: (f: number) => string
  withTime: boolean
  onMark: () => void
  onGo: (f: number) => void
}) {
  const keys = useShortcutKeys()
  const label = slot === 0 ? "In" : "Out"
  const point = slot === 0 ? "in" : "out"
  if (value == null) {
    return (
      <Tooltip label={`Mark ${point}`} shortcut={keys("player.mark", slot)}>
        <button
          onClick={onMark}
          aria-label={`Mark ${point}`}
          className="flex h-9 shrink-0 items-center rounded-full border border-white/10 px-3 text-xs font-semibold whitespace-nowrap text-fg-3 transition-colors hover:border-white/20 hover:text-fg-1"
        >
          {label}
        </button>
      </Tooltip>
    )
  }
  const go = `Go to the ${point} point`
  return (
    <ContextArea
      menu={
        <ContextMenuItem onSelect={() => onGo(value)} shortcut={keys("player.goToMark", slot)}>
          <Crosshair /> {go}
        </ContextMenuItem>
      }
    >
      <Tooltip label={go} hint={tc(value)} shortcut={keys("player.goToMark", slot)}>
        <button
          onClick={() => onGo(value)}
          aria-label={`${go}, ${tc(value)}`}
          className="flex h-9 shrink-0 items-center gap-2 rounded-full border border-accent-line bg-accent-subtle px-3 text-sm whitespace-nowrap text-fg-1 transition-colors hover:bg-accent/25"
        >
          <span className="text-xs font-semibold text-accent">{label}</span>
          {withTime && <span className="timecode">{tc(value)}</span>}
        </button>
      </Tooltip>
    </ContextArea>
  )
}
