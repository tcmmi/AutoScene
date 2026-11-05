import { memo } from "react"
import { ArrowCounterClockwise, Check, Copy, Play, Prohibit, UserCircleDashed } from "@/components/icons"
import type { MediaItem, PackClip, PackFilter } from "@/bindings"
import { Badge } from "@/components/ui/badge"
import { CardPlayButton } from "@/components/ui/card-play-button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { Tooltip } from "@/components/ui/tooltip"
import { NoFaceBadge } from "@/features/shots/NoFaceBadge"
import { ShotFrames } from "@/features/shots/ShotFrames"
import { useScrub } from "@/features/shots/scrub"
import { type BodyVerdict, formatShotLength } from "@/features/shots/shots"
import { copyText } from "@/lib/os"
import { matchShortcut, PACK_SHORTCUTS } from "@/lib/shortcuts"
import { timecode } from "@/lib/timecode"
import { cn } from "@/lib/utils"
import { REASONS, SIZES, filterReason, pickable } from "./spec"

const SIZE_SHORT = new Map(SIZES.map((s) => [s.value, s.short]))
const SIZE_LABEL = new Map(SIZES.map((s) => [s.value, s.label]))

/** A filter's word on a shot it hasn't checked again since its settings changed. */
const before = (f: PackFilter) =>
  f.checked === 0 ? `“${f.title}” said so with its settings before` : `“${f.title}” may have said so with its settings before`

/**
 * A shot of the pack: bright with a gold tick when the pack takes it,
 * dimmed with the reason when it doesn't. A click (or Space) takes it in
 * or out, with Shift every shot from the last one clicked; Enter or the
 * corner button plays it. A shot without the character's face says so;
 * its menu can say the bodies there aren't them. The menu acts on the clip
 * itself (`onWatch`, `onPick`), never on its place in the grid, which may
 * have moved by the time an entry is chosen. One a plugin's filter leaves
 * out wears the filter's own words (`filter`), and says so in its tooltip
 * when it was taken back in by hand. Until the filter has checked every
 * shot with its settings as they are, the word may be one it gave with its
 * settings before: the tooltip says so (for sure when it has checked none).
 */
export const PackCard = memo(function PackCard({
  clip,
  index,
  media,
  name,
  filter,
  onToggle,
  onOpen,
  onWatch,
  onPick,
  onBody,
}: {
  clip: PackClip
  index: number
  media?: MediaItem
  /** Whose pack it is. */
  name?: string
  /** The plugin filter that leaves the clip out (`clip.filter` in the pack's list), if any. */
  filter?: PackFilter
  onToggle: (index: number, range: boolean, byKey?: boolean) => void
  onOpen: (index: number) => void
  onWatch?: (clip: PackClip) => void
  /** Take the clip in (`true`), leave it out (`false`) or hand it back to the filters (`null`). */
  onPick?: (clip: PackClip, include: boolean | null) => void
  onBody?: (bodies: number[], verdict: BodyVerdict) => void
}) {
  const scrub = useScrub(clip.shot)
  const fps = media?.fps
  // The shot itself, as the filters judge it (handles only widen the clip).
  const { start, end } = clip.shot
  const length = fps ? formatShotLength(((end - start) * fps.den) / fps.num) : ""
  // Drop-frame where the rate counts it, like the files' names.
  const at = fps ? timecode(start, fps, true) : ""
  const where = at && `${media?.label ? `${media.label} · ` : ""}${at}`
  const why = clip.included ? null : clip.reason === "plugin" ? filterReason(filter) : clip.reason ? REASONS[clip.reason] : "Left out"
  const canPick = pickable(clip)
  const bodies = clip.shot.faceless ? (clip.shot.bodies ?? []) : []
  // Everything the chips say in short, said in full.
  const hint = [
    SIZE_LABEL.get(clip.size),
    clip.shot.faceless && "their face doesn't show",
    clip.included && filter && `“${filter.title}” would leave it out`,
    !clip.included && clip.reason === "plugin" && filter && filter.checked < filter.total && before(filter),
    canPick && `Click to ${clip.included ? "leave it out" : "take it in"}`,
  ]
    .filter(Boolean)
    .join(" · ")

  const menu = (
    <>
      {onWatch && (
        <ContextMenuItem onSelect={() => onWatch(clip)} shortcut={["Enter"]}>
          <Play /> Watch shot
        </ContextMenuItem>
      )}
      {canPick && onPick && (
        <ContextMenuItem onSelect={() => onPick(clip, !clip.included)} shortcut={["Space"]}>
          <Check /> {clip.included ? "Remove from pack" : "Add to pack"}
        </ContextMenuItem>
      )}
      {canPick && onPick && clip.manual != null && (
        <ContextMenuItem onSelect={() => onPick(clip, null)}>
          <ArrowCounterClockwise /> Hand back to the filters
        </ContextMenuItem>
      )}
      <ContextMenuSeparator />
      {bodies.length > 0 && onBody && (
        <>
          <ContextMenuItem onSelect={() => onBody(bodies, "not_them")}>
            <UserCircleDashed /> Not {name ?? "them"}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => onBody(bodies, "not_a_person")}>
            <Prohibit /> Not a person
          </ContextMenuItem>
        </>
      )}
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
        <div className="group relative aspect-video w-full">
          <Tooltip label={where} hint={hint} side="top" delay={600} disabled={!where}>
            <button
              type="button"
              aria-pressed={clip.included}
              disabled={!canPick}
              onClick={(e) => onToggle(index, e.shiftKey)}
              data-pack-index={index}
              onKeyDown={(e) => {
                const id = matchShortcut(PACK_SHORTCUTS, e.nativeEvent)
                if (id === "toggle") {
                  e.preventDefault()
                  // A held key would take the shot in and out again and again.
                  if (!e.repeat) onToggle(index, e.shiftKey, true)
                } else if (id === "watch") {
                  e.preventDefault()
                  onOpen(index)
                }
              }}
              {...scrub.handlers}
              aria-label={`Shot${where ? ` at ${where}` : ""}${length ? `, ${length}` : ""}${clip.shot.faceless ? ", without their face" : ""}${clip.included ? ", in the pack" : `, left out: ${why}`}`}
              className={cn(
                "relative block size-full overflow-hidden rounded-xl bg-inset text-left outline-none picture-edge",
                "transition-[transform,box-shadow] duration-[var(--dur-med)] ease-out focus-visible:ring-2 focus-visible:ring-accent-line",
                canPick && "hover:-translate-y-0.5 hover:shadow-lift",
                clip.included && "ring-1 ring-accent-line/70",
              )}
            >
              <span
                className={cn(
                  "absolute inset-0 transition-[opacity,filter] duration-[var(--dur-med)]",
                  !clip.included && "opacity-40 grayscale-[70%] group-hover:opacity-70",
                )}
              >
                <ShotFrames shot={clip.shot} scrub={scrub} />
              </span>
              <span className="absolute top-2 left-2 flex items-center gap-1.5">
                {clip.included ? (
                  <span className="flex size-5 items-center justify-center rounded-full bg-accent text-on-accent shadow-1">
                    <Check weight="bold" className="size-3" />
                  </span>
                ) : (
                  <Badge tone="picture" size="sm" className="max-w-[10rem]">
                    <span className="truncate">{why}</span>
                  </Badge>
                )}
                {clip.manual === true && (
                  <Badge tone="picture" size="sm">
                    Added
                  </Badge>
                )}
                {clip.shot.faceless && clip.reason !== "faceless" && <NoFaceBadge />}
              </span>
              <span className="absolute right-2 bottom-2 flex items-center gap-1.5">
                <Badge tone="picture" size="sm">
                  {SIZE_SHORT.get(clip.size)}
                </Badge>
                {length && (
                  <Badge tone="picture" size="sm" className="timecode">
                    {length}
                  </Badge>
                )}
              </span>
            </button>
          </Tooltip>
          <CardPlayButton onClick={() => onOpen(index)} aria-label={`Watch the shot${where ? ` at ${where}` : ""}`} />
        </div>
    </ContextArea>
  )
})
