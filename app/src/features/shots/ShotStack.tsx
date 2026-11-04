import { memo, useRef, useState } from "react"
import { CaretDown, Copy, Play, X } from "@/components/icons"
import type { CharacterShot, MediaItem } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { Tooltip } from "@/components/ui/tooltip"
import type { GridLayout } from "@/components/ui/virtual-grid"
import { formatDuration } from "@/lib/format"
import { assetUrl } from "@/lib/media"
import { copyText } from "@/lib/os"
import { matchShortcut, SHOTS_SHORTCUTS, useShortcutKeys } from "@/lib/shortcuts"
import { timecode } from "@/lib/timecode"
import { cn } from "@/lib/utils"
import { groupCellAt, groupSeconds, shotKey, type StackCard } from "./groups"
import { NoFaceBadge } from "./NoFaceBadge"
import { ShotCard } from "./ShotCard"
import { ShotFrames } from "./ShotFrames"
import { type BodyVerdict, formatShotLength, restingCell } from "./shots"

/** Up to this many shots, the scrub bar has a segment for each. */
const SEGMENTED = 12

/** The open stack's band, for what points at it. */
export const BAND_ID = "shot-band"

/** "5 shots · 11 s" and where the group starts, as a stack and its band both say it. */
function facts(card: StackCard, media?: MediaItem) {
  const fps = media?.fps
  const clock = fps ? formatDuration((card.start * fps.den) / fps.num).padStart(5, "0") : ""
  const length = fps ? formatShotLength(groupSeconds(card.shots, fps)) : ""
  return {
    clock,
    where: clock && `${media?.label ? `${media.label} · ` : ""}${clock}`,
    count: `${card.shots.length} shots${length && ` · ${length}`}`,
    length,
  }
}

/**
 * Moving the pointer across a stack scrubs all its shots. The picture only
 * changes shot once that shot's sprite is decoded, so it never flashes; the
 * neighbours' sprites load as the pointer gets near, so the work grows with
 * what the pointer passes, not with the size of the stack.
 */
function useStackScrub(card: StackCard) {
  const { shots, lead } = card
  const [at, setAt] = useState<{ index: number; cell: number } | null>(null)
  // The shot on the card while scrubbing.
  const [shown, setShown] = useState<number | null>(null)
  const wanted = useRef<number | null>(null)
  const decoded = useRef(new Map<string, Promise<void>>())

  const warm = (shot: CharacterShot | undefined) => {
    if (!shot?.sprite) return Promise.resolve()
    let ready = decoded.current.get(shot.sprite)
    if (!ready) {
      const img = new Image()
      img.src = assetUrl(shot.sprite)
      ready = img.decode().catch(() => {})
      decoded.current.set(shot.sprite, ready)
    }
    return ready
  }

  // (An edit can take the shot away while the pointer is on it.)
  const shot = (shown !== null && shots[shown]) || lead
  const cells = shot.sprite ? Math.max(1, shot.cells) : 0
  return {
    /** Which shot the pointer is on. */
    index: at?.index ?? null,
    shot,
    frames: { cells, cell: at && shown !== null ? Math.min(at.cell, Math.max(0, cells - 1)) : restingCell(cells), hover: null },
    handlers: {
      onPointerMove: (e: React.PointerEvent<HTMLElement>) => {
        const r = e.currentTarget.getBoundingClientRect()
        const { index, cell } = groupCellAt(e.clientX - r.left, r.width, shots)
        setAt((old) => (old?.index === index && old.cell === cell ? old : { index, cell }))
        if (wanted.current === index) return
        wanted.current = index
        void warm(shots[index]).then(() => {
          if (wanted.current === index) setShown(index)
        })
        void warm(shots[index - 1])
        void warm(shots[index + 1])
      },
      onPointerLeave: () => {
        wanted.current = null
        setAt(null)
        setShown(null)
      },
    },
  }
}

/** How far along the stack the pointer is: a segment per shot, or one bar for a big stack. */
function ScrubBar({ index, count }: { index: number; count: number }) {
  if (count > SEGMENTED) {
    return (
      <span
        className="pointer-events-none absolute bottom-0 left-0 h-[3px] bg-accent shadow-[0_0_8px_var(--accent)]"
        style={{ width: `${((index + 1) * 100) / count}%` }}
      />
    )
  }
  return (
    <span className="pointer-events-none absolute inset-x-0 bottom-0 flex h-[3px] gap-0.5">
      {Array.from({ length: count }, (_, i) => (
        <span
          key={i}
          className={cn(
            "flex-1",
            i === index ? "bg-accent shadow-[0_0_8px_var(--accent)]" : i < index ? "bg-white/50" : "bg-white/20",
          )}
        />
      ))}
    </span>
  )
}

/**
 * Shots close together on one card, like prints in a pile: hover scrubs them
 * all. A click (or Space) opens them in a band under the row, Enter or the
 * corner button watches from the first frame, as on a pack card a click does
 * one thing and watching is its own button.
 */
export const ShotStack = memo(function ShotStack({
  card,
  media,
  open,
  onToggle,
  onWatch,
}: {
  card: StackCard
  media?: MediaItem
  open: boolean
  /** Open it (`true`) or close it. */
  onToggle: (card: StackCard, show: boolean) => void
  onWatch: (card: StackCard) => void
}) {
  const scrub = useStackScrub(card)
  const keys = useShortcutKeys()
  const { shots } = card
  const fps = media?.fps
  const { where, length, count } = facts(card, media)
  // Drop-frame where the rate counts it, like the files' names.
  const at = fps ? timecode(card.start, fps, true) : ""
  const faceless = shots.every((s) => s.faceless)

  const menu = (
    <>
      <ContextMenuItem onSelect={() => onToggle(card, !open)} shortcut={keys(open ? "shots.close" : "shots.toggle")}>
        {open ? <X /> : <CaretDown />} {open ? "Hide its shots" : "Show its shots"}
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => onWatch(card)} shortcut={keys("shots.watch")}>
        <Play /> Watch from the first shot
      </ContextMenuItem>
      <ContextMenuSeparator />
      {at && (
        <ContextMenuItem onSelect={() => void copyText(at, "timecode")}>
          <Copy /> Copy timecode
        </ContextMenuItem>
      )}
    </>
  )

  return (
    <ContextArea menu={menu}>
      <div {...scrub.handlers} className="group relative aspect-video w-full">
        {/* The prints under it: what says "a pile" before any text does. */}
        <span className="pointer-events-none absolute inset-x-4 -top-3 h-full rounded-xl bg-white/[0.05] picture-edge" aria-hidden />
        <span className="pointer-events-none absolute inset-x-2 -top-1.5 h-full rounded-xl bg-white/[0.08] picture-edge" aria-hidden />
        {/* Open, there's nothing to tell. */}
        <Tooltip label={where} hint={`${shots.length} shots close together · Click to see them`} side="top" delay={600} disabled={!where || open}>
          <button
            type="button"
            aria-expanded={open}
            aria-controls={open ? BAND_ID : undefined}
            data-stack={card.key}
            onClick={() => onToggle(card, !open)}
            onKeyDown={(e) => {
              const id = matchShortcut(SHOTS_SHORTCUTS, e.nativeEvent)
              if (id === "toggle") {
                e.preventDefault()
                // A held key would open and close it again and again.
                if (!e.repeat) onToggle(card, !open)
              } else if (id === "watch") {
                e.preventDefault()
                onWatch(card)
              } else if (open && e.key === "Tab" && !e.shiftKey) {
                // The band comes after the rest of the row: Tab goes straight to it.
                const first = document.getElementById(BAND_ID)?.querySelector("button")
                if (first) {
                  e.preventDefault()
                  first.focus()
                }
              }
            }}
            aria-label={`${shots.length} shots${where ? ` at ${where}` : ""}${length ? `, ${length}` : ""}${faceless ? ", without their face" : ""}`}
            className={cn(
              "relative block size-full overflow-hidden rounded-xl bg-inset text-left outline-none picture-edge",
              "transition-[transform,box-shadow] duration-[var(--dur-med)] ease-out",
              "hover:-translate-y-0.5 hover:shadow-lift focus-visible:ring-2 focus-visible:ring-accent-line",
            )}
          >
            <ShotFrames shot={scrub.shot} scrub={scrub.frames} />
            {open && <span className="absolute inset-0 rounded-xl inset-ring inset-ring-accent/70" />}
            {scrub.index !== null && <ScrubBar index={scrub.index} count={shots.length} />}
            <span className="timecode absolute right-2 bottom-2 rounded-full bg-black/50 px-2 text-[11px] leading-5 text-white backdrop-blur-md">
              {scrub.index !== null && shots.length > SEGMENTED ? `${scrub.index + 1} / ${shots.length}` : count}
            </span>
            <span className="absolute top-2 left-2 flex items-center gap-1.5">
              {faceless && <NoFaceBadge />}
              {media?.label && (
                <span className="timecode rounded-full bg-black/50 px-2 text-[11px] leading-5 text-white/90 opacity-0 backdrop-blur-md transition-opacity group-hover:opacity-100">
                  {media.label}
                </span>
              )}
            </span>
          </button>
        </Tooltip>
        <button
          type="button"
          onClick={() => onWatch(card)}
          aria-label={`Watch from ${where}`}
          className={cn(
            "absolute top-2 right-2 flex size-7 items-center justify-center rounded-full bg-black/55 text-white backdrop-blur-md",
            "opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/75 focus-visible:opacity-100",
          )}
        >
          <Play weight="fill" className="size-3.5" />
        </button>
      </div>
    </ContextArea>
  )
})

/**
 * An open stack: every shot of it as a card of its own, in a band under the
 * row of the stack. What the player can do is say so: it opens at one frame
 * and plays on, with the other person's shots, hence "Watch from".
 */
export const ShotBand = memo(function ShotBand({
  card,
  media,
  name,
  columns,
  notch,
  onOpen,
  onBody,
  onWatch,
  onClose,
}: {
  card: StackCard
  media?: MediaItem
  /** Whose shots they are. */
  name?: string
  columns: GridLayout["columns"]
  /** Where the stack's middle is, from the band's left edge. */
  notch: number
  onOpen: (shot: CharacterShot) => void
  onBody?: (bodies: number[], verdict: BodyVerdict) => void
  onWatch: (card: StackCard) => void
  onClose: () => void
}) {
  const keys = useShortcutKeys()
  const { clock, where, count } = facts(card, media)
  return (
    <section
      id={BAND_ID}
      aria-label={`${count}${where ? ` at ${where}` : ""}`}
      className="relative animate-rise-in rounded-3xl bg-white/[0.04] p-5 ring-1 ring-white/[0.06] [animation-duration:var(--dur-med)]"
    >
      <span
        className="absolute bottom-full h-2 w-4 -translate-x-1/2 bg-white/[0.06] [clip-path:polygon(50%_0,100%_100%,0_100%)]"
        style={{ left: notch }}
        aria-hidden
      />
      <header className="mb-4 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <p className="flex min-w-0 flex-wrap items-baseline gap-x-3">
          <span className="timecode text-md text-fg-1">{where}</span>
          <span className="text-sm text-fg-3">{count}</span>
        </p>
        <div className="flex shrink-0 items-center gap-2">
          <Button size="sm" variant="glass" onClick={() => onWatch(card)}>
            <Play weight="fill" /> {clock ? `Watch from ${clock}` : "Watch from the first shot"}
          </Button>
          <Tooltip label="Close" shortcut={keys("shots.close")}>
            <Button size="icon-sm" variant="glass" onClick={onClose} aria-label="Close">
              <X />
            </Button>
          </Tooltip>
        </div>
      </header>
      <div className="grid gap-4" style={{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }}>
        {card.shots.map((s) => (
          <div key={s.start} data-card={shotKey(s)} className="min-w-0">
            <ShotCard shot={s} media={media} name={name} onOpen={onOpen} onBody={onBody} />
          </div>
        ))}
      </div>
    </section>
  )
})
