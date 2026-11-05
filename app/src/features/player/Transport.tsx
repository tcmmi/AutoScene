import type * as React from "react"
import { useEffect, useRef } from "react"
import { useTranslation } from "react-i18next"
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
import { fmt, i18n } from "@/lib/i18n"
import { keysText, useShortcutKeys } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"
import { type FrameStore, useFrame } from "./frame-store"
import { rateLabel } from "./shuttle"
import { type TimecodeDisplayMode, useTimecodeMode } from "./timecode-mode"
import { type Tier, counterChars, readoutChars, useTransportTier } from "./transport-tier"
import { percentText, readoutWords } from "./words"

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
  const { t } = useTranslation("player")
  const keys = useShortcutKeys()
  const { mode } = useTimecodeMode()
  const hasMarks = inFrame != null || outFrame != null
  const row = useRef<HTMLDivElement>(null)
  const tier = useTransportTier(row, [hasMarks, goto, mode, i18n.language])
  const playing = rate !== 0
  const frames = t("frames", { count: length })
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
      <TransportButton label={t("transport.firstFrame")} keys={keys("player.ends", 0)} onClick={() => onSeek(0)}>
        <SkipBack weight="fill" />
      </TransportButton>
      <TransportButton label={t("transport.previousFrame")} keys={keys("player.frame", 0)} onClick={() => onStep(-1)}>
        <CaretLineLeft weight="bold" />
      </TransportButton>
      <Tooltip label={playing ? t("transport.pause") : t("transport.play")} shortcut={keys("player.playPause")}>
        <button
          onClick={onToggle}
          aria-label={playing ? t("transport.pause") : t("transport.play")}
          className="mx-2 flex size-12 items-center justify-center rounded-full bg-cta text-on-cta shadow-[0_10px_30px_-8px_rgb(255_255_255/0.35)] transition-transform duration-[var(--dur-fast)] hover:scale-105 active:scale-95 [&_svg]:size-5"
        >
          {playing ? <Pause weight="fill" /> : <Play weight="fill" className="translate-x-px" />}
        </button>
      </Tooltip>
      <TransportButton label={t("transport.nextFrame")} keys={keys("player.frame", 1)} onClick={() => onStep(1)}>
        <CaretLineRight weight="bold" />
      </TransportButton>
      <TransportButton label={t("transport.lastFrame")} keys={keys("player.ends", 1)} onClick={() => onSeek(last)}>
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
          <Tooltip label={t("transport.clipLength")} hint={frames}>
            {/* The quiet |<->| says it's a length, not a third position. */}
            <span className="timecode inline-flex items-center gap-1 text-sm whitespace-nowrap text-fg-2">
              <ArrowsOutLineHorizontal weight="bold" aria-hidden className="size-3.5 shrink-0 text-fg-3" />
              {tc(length)}
            </span>
          </Tooltip>
          <Tooltip label={t("transport.clearMarks")} shortcut={keys("player.clearMarks")}>
            <Button variant="ghost" size="icon-sm" aria-label={t("transport.clearMarks")} onClick={onClear}>
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
      label={t("transport.exportClip")}
      hint={
        hasMarks
          ? `${tc(length)} · ${frames}`
          : shotKey.length > 0
            ? t("transport.exportHintKeys", { keys: keysText(shotKey) })
            : t("transport.exportHint")
      }
      shortcut={keys("player.export")}
    >
      <span className="inline-flex shrink-0">
        <Button
          variant="secondary"
          size={tier <= 2 ? "md" : "icon"}
          aria-label={t("transport.exportClip")}
          disabled={!canExport}
          onClick={onExport}
        >
          <Scissors weight="bold" />
          {tier <= 2 && t("transport.export")}
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
          <Tooltip label={muted ? t("transport.unmute") : t("transport.mute")} shortcut={keys("player.mute")}>
            <button
              onClick={onToggleMute}
              aria-label={muted ? t("transport.unmute") : t("transport.mute")}
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
              label={t("transport.volume")}
              format={(v) => t("percent", { value: Math.round(v * 100) })}
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
  const { t } = useTranslation("player")
  // Said once per language: this draws on every frame.
  const words = readoutWords()
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
        return fmt.count(f)
      case "percent": {
        return percentText(last > 0 ? (f / last) * 100 : 0)
      }
    }
  }

  // Go to takes the place of the speed, the readout and the counter. It
  // pauses, so no speed would show beside it.
  if (goto) {
    return (
      <input
        ref={field}
        aria-label={words.goTo}
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
            toast.error(t("readout.badTimecode"), { description: t("readout.badTimecodeTry") })
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
              <ContextMenuRadioItem value="elapsed">{words.menu.elapsed}</ContextMenuRadioItem>
              <ContextMenuRadioItem value="remaining">{words.menu.remaining}</ContextMenuRadioItem>
              <ContextMenuRadioItem value="frames">{words.menu.frames}</ContextMenuRadioItem>
              <ContextMenuRadioItem value="percent">{words.menu.percent}</ContextMenuRadioItem>
            </ContextMenuRadioGroup>
            <ContextMenuSeparator />
            <ContextMenuItem onSelect={() => onGoto(true)} shortcut={keys("player.goTo")}>
              {words.goToMenu}
            </ContextMenuItem>
          </>
        }
      >
        <Tooltip label={words.goTo} hint={words.goToHint} shortcut={keys("player.goTo")}>
          <button
            data-readout=""
            onClick={() => onGoto(true)}
            aria-label={words.aria[mode](formatReadout(frame, mode))}
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
        <Tooltip label={words.counterTip}>
          <span
            data-counter=""
            style={{ minWidth: `${counterChars(mode, last)}ch` }}
            className="timecode inline-block shrink-0 text-right text-sm whitespace-nowrap text-fg-3"
          >
            {mode === "frames"
              ? `/ ${fmt.count(last)}`
              : mode === "percent"
                ? `${fmt.count(frame)} / ${fmt.count(last)} ${words.frameUnit}`
                : `${fmt.count(frame)} / ${fmt.count(last)}`}
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
  const { t } = useTranslation("player")
  const keys = useShortcutKeys()
  const isIn = slot === 0
  const label = isIn ? t("transport.in") : t("transport.out")
  if (value == null) {
    const mark = isIn ? t("transport.markIn") : t("transport.markOut")
    return (
      <Tooltip label={mark} shortcut={keys("player.mark", slot)}>
        <button
          onClick={onMark}
          aria-label={mark}
          className="flex h-9 shrink-0 items-center rounded-full border border-white/10 px-3 text-xs font-semibold whitespace-nowrap text-fg-3 transition-colors hover:border-white/20 hover:text-fg-1"
        >
          {label}
        </button>
      </Tooltip>
    )
  }
  const go = isIn ? t("transport.goToIn") : t("transport.goToOut")
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
          aria-label={isIn ? t("transport.goToInAt", { time: tc(value) }) : t("transport.goToOutAt", { time: tc(value) })}
          className="flex h-9 shrink-0 items-center gap-2 rounded-full border border-accent-line bg-accent-subtle px-3 text-sm whitespace-nowrap text-fg-1 transition-colors hover:bg-accent/25"
        >
          <span className="text-xs font-semibold text-accent">{label}</span>
          {withTime && <span className="timecode">{tc(value)}</span>}
        </button>
      </Tooltip>
    </ContextArea>
  )
}
