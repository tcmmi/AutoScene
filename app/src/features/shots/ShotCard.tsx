import { memo } from "react"
import { useTranslation } from "react-i18next"
import { Copy, Play, Prohibit, UserCircleDashed } from "@/components/icons"
import type { CharacterShot, MediaItem } from "@/bindings"
import { Badge } from "@/components/ui/badge"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { Tooltip } from "@/components/ui/tooltip"
import { copyText } from "@/lib/os"
import { timecode } from "@/lib/timecode"
import { cn } from "@/lib/utils"
import { NoFaceBadge } from "./NoFaceBadge"
import { useScrub } from "./scrub"
import { ShotFrames } from "./ShotFrames"
import { type BodyVerdict, formatShotLength } from "./shots"

/**
 * A shot as a card: its sprite — frames spread over the shot — scrubbed by
 * moving the pointer across, the character's face in a corner, its length
 * and where it is. Click opens the moment in the player.
 */
export const ShotCard = memo(function ShotCard({
  shot,
  media,
  name,
  onOpen,
  onBody,
}: {
  shot: CharacterShot
  media?: MediaItem
  /** Whose shot it is. */
  name?: string
  onOpen: (shot: CharacterShot) => void
  onBody?: (bodies: number[], verdict: BodyVerdict) => void
}) {
  const { t } = useTranslation(["library", "common"])
  const scrub = useScrub(shot)
  const fps = media?.fps
  const length = fps ? formatShotLength(((shot.end - shot.start) * fps.den) / fps.num) : ""
  const where = fps ? `${media?.label ? `${media.label} · ` : ""}${timecode(shot.start, fps, false)}` : ""
  // Drop-frame where the rate counts it, like the files' names.
  const at = fps ? timecode(shot.start, fps, true) : ""
  const bodies = shot.faceless ? (shot.bodies ?? []) : []
  const menu = (
    <>
      <ContextMenuItem onSelect={() => onOpen(shot)}>
        <Play /> {t("shots.watchShot")}
      </ContextMenuItem>
      {at && (
        <ContextMenuItem onSelect={() => void copyText(at, "timecode")}>
          <Copy /> {t("shots.copyTimecode")}
        </ContextMenuItem>
      )}
      <ContextMenuSeparator />
      {bodies.length > 0 && onBody && (
        <>
          <ContextMenuItem onSelect={() => onBody(bodies, "not_them")}>
            <UserCircleDashed /> {name != null ? t("shots.notThem", { name }) : t("shots.notThemUnnamed")}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => onBody(bodies, "not_a_person")}>
            <Prohibit /> {t("shots.notAPerson")}
          </ContextMenuItem>
        </>
      )}
    </>
  )

  return (
    <ContextArea menu={menu}>
      <Tooltip
        label={where}
        hint={t(shot.faceless ? "shots.hintFaceless" : "shots.hintWatch")}
        side="top"
        delay={600}
        disabled={!where}
      >
        <button
          type="button"
          onClick={() => onOpen(shot)}
          {...scrub.handlers}
          aria-label={[
            where ? t("shots.labelAt", { where }) : t("shots.label"),
            length,
            shot.faceless ? t("shots.withoutFace") : "",
          ]
            .filter(Boolean)
            .join(", ")}
          className={cn(
            "group relative block aspect-video w-full overflow-hidden rounded-xl bg-inset text-left outline-none picture-edge",
            "transition-[transform,box-shadow] duration-[var(--dur-med)] ease-out",
            "hover:-translate-y-0.5 hover:shadow-lift focus-visible:ring-2 focus-visible:ring-accent-line",
          )}
        >
          <ShotFrames shot={shot} scrub={scrub} />
          {length && (
            <Badge tone="picture" size="sm" className="timecode absolute right-2 bottom-2">
              {length}
            </Badge>
          )}
          <span className="absolute top-2 left-2 flex items-center gap-1.5">
            {shot.faceless && <NoFaceBadge />}
            {media?.label && (
              <Badge tone="picture" size="sm" className="timecode opacity-0 transition-opacity group-hover:opacity-100">
                {media.label}
              </Badge>
            )}
          </span>
          <Play
            weight="fill"
            className="absolute top-2 right-2 size-4 text-white opacity-0 drop-shadow transition-opacity group-hover:opacity-90"
            aria-hidden
          />
        </button>
      </Tooltip>
    </ContextArea>
  )
})
