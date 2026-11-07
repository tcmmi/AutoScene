import type * as React from "react"
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Check, X } from "@/components/icons"
import type { Character, RescanFace } from "@/bindings"
import { Avatar } from "@/components/ui/avatar"
import { Badge } from "@/components/ui/badge"
import { Tooltip } from "@/components/ui/tooltip"
import { characterLabel } from "./cast"
import { RescanFaceTile } from "./RescanFaceTile"

/** Faces shown before "Show more": each one's picture is cut from its exact frame. */
const SHOWN = 12

/**
 * Faces that may go to one person: who, what is said of them, and the answers
 * for all of them (`actions`) and for each face, on the face.
 */
export function FaceLane({
  pid,
  who,
  heading,
  about,
  faces,
  actions,
  from,
  yes,
  no,
  onYes,
  onNo,
}: {
  pid: string
  /** Whom the faces go to; none: they go to no one. */
  who?: Character
  heading: React.ReactNode
  about: React.ReactNode
  faces: RescanFace[]
  actions: React.ReactNode
  /** Where a face is now, for its corner. */
  from?: (f: RescanFace) => Character | undefined
  yes: string
  no: string
  onYes: (f: RescanFace) => void
  onNo: (f: RescanFace) => void
}) {
  const { t } = useTranslation("cast")
  const [all, setAll] = useState(false)
  const shown = all ? faces : faces.slice(0, SHOWN)
  return (
    <section className="rounded-3xl border border-hairline bg-white/[0.02] p-6">
      <div className="mb-5 flex flex-wrap items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-4">
          {who && <Avatar path={who.coverSmall ?? who.cover} className="size-12 ring-1 ring-white/15" />}
          <div className="min-w-0">
            <h3 className="display truncate text-3xl text-fg-1">{heading}</h3>
            <p className="mt-0.5 text-sm text-fg-2">{about}</p>
          </div>
        </div>
        <div className="flex shrink-0 gap-2">{actions}</div>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(116px,1fr))] gap-3">
        {shown.map((f) => {
          const on = from?.(f)
          return (
            <RescanFaceTile key={f.face.id} pid={pid} face={f.face}>
              {f.verdict === "maybe" && (
                <Badge tone="picture" size="sm" className="absolute top-1.5 left-1.5">
                  {t("rescan.lane.maybe")}
                </Badge>
              )}
              {on && (
                <Tooltip label={t("rescan.lane.onNow", { name: characterLabel(on) })} side="top">
                  <span className="absolute top-1.5 right-1.5 flex">
                    <Avatar path={on.coverSmall ?? on.cover} className="size-6 ring-1 ring-white/40" />
                  </span>
                </Tooltip>
              )}
              <div className="absolute inset-x-1 bottom-1 flex justify-between gap-1 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
                <Tooltip label={yes} side="top">
                  <button
                    onClick={() => onYes(f)}
                    aria-label={yes}
                    className="flex h-7 flex-1 items-center justify-center rounded-lg bg-black/60 text-white backdrop-blur-md transition-transform hover:bg-accent hover:text-on-accent active:scale-95"
                  >
                    <Check className="size-3.5" />
                  </button>
                </Tooltip>
                <Tooltip label={no} side="top">
                  <button
                    onClick={() => onNo(f)}
                    aria-label={no}
                    className="flex h-7 flex-1 items-center justify-center rounded-lg bg-black/60 text-white backdrop-blur-md transition-transform hover:bg-danger hover:text-white active:scale-95"
                  >
                    <X className="size-3.5" />
                  </button>
                </Tooltip>
              </div>
            </RescanFaceTile>
          )
        })}
      </div>
      {faces.length > SHOWN && (
        <button onClick={() => setAll((v) => !v)} className="mt-4 text-sm font-medium text-fg-2 hover:text-fg-1">
          {all ? t("maybe.showFewer") : t("maybe.showMore", { count: faces.length - shown.length })}
        </button>
      )}
    </section>
  )
}
