import { CaretLeft, CaretRight } from "@/components/icons"
import type { Segment } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"
import { Tooltip } from "@/components/ui/tooltip"
import { useShortcutKeys } from "@/lib/shortcuts"
import { SEGMENT_LABEL } from "../labels"

/**
 * The shot under the playhead, at the top of the Cast tab: which of the
 * episode's shots it is, its length and range, and Mark shot with the steps
 * to the shots around it. A transition or a flash shows its own name.
 */
export function ShotHeader({
  seg,
  number,
  total,
  tc,
  onMark,
  onPrev,
  onNext,
}: {
  seg: Segment
  /** The shot's number among the episode's shots, from 0 (shots only). */
  number: number
  /** How many shots the episode has. */
  total: number
  tc: (f: number) => string
  onMark: () => void
  onPrev: () => void
  onNext: () => void
}) {
  const keys = useShortcutKeys()
  const shot = seg.kind === "shot"
  return (
    <div>
      <div className="flex items-baseline justify-between gap-2">
        <span className="min-w-0 truncate text-md font-medium text-fg-1">
          {shot ? `Shot ${number + 1} of ${total}` : SEGMENT_LABEL[seg.kind]}
          {seg.tiny && <span className="text-fg-3"> · tiny</span>}
        </span>
        <span className="timecode shrink-0 text-sm text-fg-2">{seg.end - seg.start} frames</span>
      </div>
      <p className="timecode mt-0.5 text-xs text-fg-3">
        {tc(seg.start)} – {tc(seg.end - 1)}
      </p>
      <div className="mt-3 flex items-center gap-1.5">
        {shot ? (
          <Button size="sm" variant="secondary" className="flex-1" onClick={onMark}>
            Mark shot <Kbd keys={keys("player.markShot")} className="ml-auto" />
          </Button>
        ) : (
          <span className="flex-1" />
        )}
        <Tooltip label="Previous shot" shortcut={keys("player.shot", 0)}>
          <Button size="icon-sm" variant="ghost" aria-label="Previous shot" onClick={onPrev}>
            <CaretLeft />
          </Button>
        </Tooltip>
        <Tooltip label="Next shot" shortcut={keys("player.shot", 1)}>
          <Button size="icon-sm" variant="ghost" aria-label="Next shot" onClick={onNext}>
            <CaretRight />
          </Button>
        </Tooltip>
      </div>
    </div>
  )
}
