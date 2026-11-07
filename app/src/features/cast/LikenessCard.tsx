import type * as React from "react"
import { useTranslation } from "react-i18next"
import { ArrowsMerge, Check, Info, User, Users, X } from "@/components/icons"
import type { Character, FaceTrackItem, RescanPair } from "@/bindings"
import { Artwork } from "@/components/ui/artwork"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { assetUrl } from "@/lib/media"
import { cn } from "@/lib/utils"
import { characterLabel } from "./cast"

/** One of the two: their portrait, leaning in, and their clearest faces. */
function Person({ who, faces, side }: { who: Character; faces: FaceTrackItem[]; side: "left" | "right" }) {
  return (
    <div className="min-w-0 flex-1">
      <Artwork
        path={who.cover}
        className={cn(
          "aspect-[3/4] rounded-2xl picture-edge transition-transform duration-[var(--dur-slow)] ease-out",
          side === "left" ? "origin-bottom-right -rotate-1 group-hover/pair:rotate-0" : "origin-bottom-left rotate-1 group-hover/pair:rotate-0",
        )}
        imgClassName="object-[50%_30%]"
        fallback={
          <div className="flex size-full items-center justify-center artwork-fallback text-fg-3 [&_svg]:size-12">
            <User />
          </div>
        }
      >
        <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/85 via-transparent to-transparent" />
        <div className={cn("display absolute inset-x-0 bottom-0 truncate p-4 text-3xl text-white", !who.name && "text-white/75 italic")}>
          {characterLabel(who)}
        </div>
      </Artwork>
      <div className="mt-3 grid grid-cols-6 gap-1.5">
        {faces.map((f) => (
          <img key={f.id} src={assetUrl(f.thumb)} alt="" loading="lazy" className="aspect-square w-full rounded-md object-cover picture-edge" />
        ))}
      </div>
    </div>
  )
}

/** One thing that speaks for or against them being one person. */
function Evidence({ good, children }: { good: boolean | null; children: React.ReactNode }) {
  return (
    <li className="flex items-center gap-2.5 text-sm text-fg-2 [&_svg]:size-4 [&_svg]:shrink-0">
      <span className={good === null ? "text-fg-3" : good ? "text-accent" : "text-fg-3"}>
        {good === null ? <Info /> : good ? <Check weight="bold" /> : <X weight="bold" />}
      </span>
      {children}
    </li>
  )
}

/**
 * "Are these two one person?": both side by side over a gold seam, what speaks
 * for it, and the two answers. Nothing happens until one is given.
 */
export function LikenessCard({
  pair,
  a,
  b,
  onSame,
  onApart,
}: {
  pair: RescanPair
  a: Character
  b: Character
  onSame: () => void
  onApart: () => void
}) {
  const { t } = useTranslation("cast")
  const likely = pair.likeness === "likely"
  return (
    <section className="group/pair rounded-3xl border border-accent-line/60 bg-[linear-gradient(135deg,rgb(224_180_92/0.1),rgb(255_255_255/0.02)_60%)] p-6">
      <div className="flex flex-wrap gap-8">
        <div className="relative flex min-w-[320px] flex-[3] gap-4">
          <Person who={a} faces={pair.aFaces} side="left" />
          <span
            aria-hidden
            className="pointer-events-none absolute top-[6%] bottom-[22%] left-1/2 w-px -translate-x-1/2 bg-gradient-to-b from-transparent via-accent to-transparent shadow-[0_0_14px_1px_var(--accent)]"
          />
          <Person who={b} faces={pair.bFaces} side="right" />
        </div>
        <div className="flex min-w-[240px] flex-[2] flex-col justify-center">
          <Badge tone={likely ? "accent" : "neutral"} className="self-start">
            {likely ? t("rescan.pair.likely") : t("rescan.pair.maybe")}
          </Badge>
          <h3 className="display mt-3 text-4xl leading-tight text-fg-1">{t("rescan.pair.question")}</h3>
          <ul className="mt-5 space-y-2.5">
            <Evidence good={likely}>{likely ? t("rescan.pair.facesMatch") : t("rescan.pair.facesClose")}</Evidence>
            {pair.secondAgrees === null ? (
              <Evidence good={null}>{t("rescan.pair.oneCheck")}</Evidence>
            ) : (
              <Evidence good={pair.secondAgrees}>
                {pair.secondAgrees ? t("rescan.pair.secondAgrees") : t("rescan.pair.secondDoubts")}
              </Evidence>
            )}
            <Evidence good={pair.together === 0}>
              {pair.together === 0 ? t("rescan.pair.neverTogether") : t("rescan.pair.together", { count: pair.together })}
            </Evidence>
          </ul>
          <div className="mt-6 flex flex-wrap gap-2">
            <Button variant="accent" onClick={onSame}>
              <ArrowsMerge weight="bold" /> {t("rescan.pair.same")}
            </Button>
            <Button variant="secondary" onClick={onApart}>
              <Users /> {t("rescan.pair.apart")}
            </Button>
          </div>
        </div>
      </div>
    </section>
  )
}
