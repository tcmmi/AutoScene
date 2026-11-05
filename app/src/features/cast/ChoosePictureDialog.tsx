import { useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { Check, CircleNotch } from "@/components/icons"
import type { Character, FaceTrackItem } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip } from "@/components/ui/tooltip"
import { assetUrl } from "@/lib/media"
import { useCastTracks, useMediaList } from "@/lib/queries"
import { timecode } from "@/lib/timecode"
import { cn } from "@/lib/utils"
import { castEdit, characterLabel, pictureChoices } from "./cast"

/** Faces shown at first, and added by each "Show more faces". */
const CHOICES = 24
const NO_TRACKS: FaceTrackItem[] = []

/** Choose the picture a character's card and page show, from their clearest faces. */
export function ChoosePictureDialog({
  pid,
  character,
  open,
  onOpenChange,
}: {
  pid: string
  character: Character | null
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  if (!open || !character) return null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <Choices pid={pid} character={character} onDone={() => onOpenChange(false)} />
    </Dialog>
  )
}

function Choices({ pid, character: c, onDone }: { pid: string; character: Character; onDone: () => void }) {
  const { t } = useTranslation(["cast", "common"])
  const tracks = useCastTracks(pid, { kind: "character", id: c.id })
  const { data: media } = useMediaList(pid)
  const [count, setCount] = useState(CHOICES)
  // The face being made their picture: the others wait meanwhile.
  const [busy, setBusy] = useState<number | null>(null)
  const list = tracks.data ?? NO_TRACKS
  const faces = useMemo(() => pictureChoices(list, c.coverSmall, count), [list, c.coverSmall, count])
  const mediaById = useMemo(() => new Map((media ?? []).map((m) => [m.id, m])), [media])

  const pick = async (face: FaceTrackItem) => {
    if (face.thumb === c.coverSmall) return onDone()
    setBusy(face.id)
    const r = await castEdit(pid, { op: "set_cover", characterId: c.id, track: face.id })
    setBusy(null)
    if (r !== undefined) onDone()
  }

  return (
    <DialogContent
      title={t("choosePicture.title", { name: characterLabel(c) })}
      description={t("choosePicture.description")}
      className="w-[min(620px,calc(100vw-32px))]"
    >
      {tracks.isPending ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(84px,1fr))] gap-2.5">
          {Array.from({ length: 12 }, (_, i) => (
            <Skeleton key={i} className="aspect-square rounded-xl" />
          ))}
        </div>
      ) : tracks.isError && tracks.data === undefined ? (
        <div className="flex items-center justify-between gap-4">
          <p className="text-md text-fg-3">{t("choosePicture.loadFailed")}</p>
          <Button variant="ghost" size="sm" onClick={() => void tracks.refetch()}>
            {t("common:action.tryAgain")}
          </Button>
        </div>
      ) : faces.length === 0 ? (
        <p className="text-md text-fg-3">{t("choosePicture.none")}</p>
      ) : (
        // A little room around the faces: the ring of the picture now isn't cut off.
        <div className="-m-1 max-h-[min(440px,60vh)] overflow-y-auto p-1">
          <div className="grid grid-cols-[repeat(auto-fill,minmax(84px,1fr))] gap-2.5">
            {faces.map((face) => {
              const m = mediaById.get(face.mediaId)
              const where = m?.fps ? `${m.label ? `${m.label} · ` : ""}${timecode(face.start, m.fps, false)}` : ""
              const now = face.thumb === c.coverSmall
              return (
                <Tooltip key={face.id} label={where} hint={now ? t("choosePicture.now") : undefined} side="top" delay={600} disabled={!where}>
                  <button
                    type="button"
                    onClick={() => void pick(face)}
                    disabled={busy != null}
                    aria-pressed={now}
                    aria-label={
                      where
                        ? t(now ? "faces.ariaAtNow" : "faces.ariaAt", { where })
                        : t(now ? "faces.ariaNow" : "faces.aria")
                    }
                    className={cn(
                      "relative block aspect-square w-full overflow-hidden rounded-xl bg-inset outline-none picture-edge",
                      "transition-[box-shadow,opacity] duration-[var(--dur-fast)] focus-visible:ring-2 focus-visible:ring-accent-line",
                      busy != null && busy !== face.id && "opacity-50",
                      now ? "ring-2 ring-accent" : "hover:ring-1 hover:ring-white/30",
                    )}
                  >
                    <img
                      src={assetUrl(face.thumb)}
                      alt=""
                      loading="lazy"
                      decoding="async"
                      draggable={false}
                      className="size-full object-cover"
                    />
                    {now && (
                      <span className="absolute top-1.5 right-1.5 flex size-5 items-center justify-center rounded-full bg-accent text-on-accent">
                        <Check className="size-3" weight="bold" />
                      </span>
                    )}
                    {busy === face.id && (
                      <span className="absolute inset-0 flex items-center justify-center bg-black/45 text-white [&_svg]:size-5">
                        <CircleNotch className="animate-spin" />
                      </span>
                    )}
                  </button>
                </Tooltip>
              )
            })}
          </div>
        </div>
      )}
      {list.length > count && (
        <DialogFooter className="mt-4 justify-start">
          <Button variant="ghost" size="sm" onClick={() => setCount((n) => n + CHOICES)}>
            {t("choosePicture.showMore")}
          </Button>
        </DialogFooter>
      )}
    </DialogContent>
  )
}
