import { useTranslation } from "react-i18next"
import { CaretLeft, CaretRight } from "@/components/icons"
import type { Segment } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Kbd } from "@/components/ui/kbd"
import { Tooltip } from "@/components/ui/tooltip"
import { useShortcutKeys } from "@/lib/shortcuts"
import { segmentLabel } from "../words"

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
  const { t } = useTranslation("player")
  const keys = useShortcutKeys()
  const shot = seg.kind === "shot"
  return (
    <div>
      <div className="flex items-baseline justify-between gap-2">
        <span className="min-w-0 truncate text-md font-medium text-fg-1">
          {shot ? t(seg.tiny ? "shot.ofTiny" : "shot.of", { number: number + 1, total }) : segmentLabel(seg.kind)}
        </span>
        <span className="timecode shrink-0 text-sm text-fg-2">{t("shot.length", { count: seg.end - seg.start, length: seg.end - seg.start })}</span>
      </div>
      <p className="timecode mt-0.5 text-xs text-fg-3">
        {tc(seg.start)} – {tc(seg.end - 1)}
      </p>
      <div className="mt-3 flex items-center gap-1.5">
        {shot ? (
          <Button size="sm" variant="secondary" className="flex-1" onClick={onMark}>
            {t("shot.mark")} <Kbd keys={keys("player.markShot")} className="ml-auto" />
          </Button>
        ) : (
          <span className="flex-1" />
        )}
        <Tooltip label={t("shot.previous")} shortcut={keys("player.shot", 0)}>
          <Button size="icon-sm" variant="ghost" aria-label={t("shot.previous")} onClick={onPrev}>
            <CaretLeft />
          </Button>
        </Tooltip>
        <Tooltip label={t("shot.next")} shortcut={keys("player.shot", 1)}>
          <Button size="icon-sm" variant="ghost" aria-label={t("shot.next")} onClick={onNext}>
            <CaretRight />
          </Button>
        </Tooltip>
      </div>
    </div>
  )
}
