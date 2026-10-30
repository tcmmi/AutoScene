import { Link } from "@tanstack/react-router"
import { User, UserFocus } from "@/components/icons"
import type { Character } from "@/bindings"
import { Artwork } from "@/components/ui/artwork"
import { ContextArea } from "@/components/ui/context-area"
import { contextKit } from "@/components/ui/menu-actions"
import { Tooltip } from "@/components/ui/tooltip"
import { cn } from "@/lib/utils"
import { formatScreenTime, plural } from "./cast"
import { CharacterItems } from "./character-actions"

/** A character as a portrait: the face fills the card, the name sits on it. */
export function CharacterCard({ pid, character: c, className }: { pid: string; character: Character; className?: string }) {
  // Their episode is being analysed again: the faces come back when it's done.
  const refinding = c.tracks === 0 && c.waiting > 0
  return (
    <ContextArea menu={<CharacterItems kit={contextKit} pid={pid} character={c} />}>
        <Link
          to="/p/$pid/cast/$cid"
          params={{ pid, cid: c.id }}
          className={cn(
            "group block rounded-2xl transition-transform duration-[var(--dur-med)] ease-out hover:-translate-y-1 focus-visible:-translate-y-1",
            refinding && "opacity-60",
            className,
          )}
        >
          <Artwork
            path={c.cover}
            className="aspect-[3/4] rounded-2xl picture-edge transition-shadow duration-[var(--dur-med)] group-hover:shadow-lift"
            imgClassName="group-hover:scale-[1.05] duration-[600ms]"
            fallback={
              <div className="flex size-full items-center justify-center artwork-fallback text-fg-3 [&_svg]:size-12">
                {refinding ? <UserFocus /> : <User />}
              </div>
            }
          >
            <div className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/90 via-black/15 to-transparent" />
            {c.unsure > 0 && (
              <Tooltip label={`${c.unsure} faces to review`}>
                <span className="absolute top-2.5 right-2.5 flex items-center gap-1.5 rounded-full border border-accent/40 bg-black/60 px-2 py-0.5 text-xs font-semibold text-accent backdrop-blur-md">
                  <span className="size-1.5 rounded-full bg-accent animate-pulse" />
                  {c.unsure}
                </span>
              </Tooltip>
            )}
            <div className="absolute inset-x-0 bottom-0 p-3.5">
              <div className={cn("display truncate text-2xl leading-7", c.name ? "text-white" : "text-white/70 italic")}>{c.label}</div>
              <div className="mt-0.5 truncate text-xs text-white/70">
                {refinding ? (
                  "Being found again…"
                ) : (
                  <>
                    {formatScreenTime(c.screenSec ?? 0)} · {plural(c.shots, "shot")}
                  </>
                )}
              </div>
            </div>
          </Artwork>
        </Link>
    </ContextArea>
  )
}

/** A compact row for list view: avatar thumbnail, name, screen time, shots. */
export function CharacterRow({ pid, character: c }: { pid: string; character: Character }) {
  const refinding = c.tracks === 0 && c.waiting > 0
  return (
    <ContextArea menu={<CharacterItems kit={contextKit} pid={pid} character={c} />}>
      <Link
        to="/p/$pid/cast/$cid"
        params={{ pid, cid: c.id }}
        className="group flex items-center justify-between gap-4 rounded-xl px-4 py-2.5 transition-colors hover:bg-white/[0.04]"
      >
        <div className="flex min-w-0 items-center gap-3.5">
          <Artwork
            path={c.coverSmall ?? c.cover}
            className="size-10 shrink-0 rounded-lg picture-edge"
            fallback={
              <div className="flex size-full items-center justify-center artwork-fallback text-fg-3 [&_svg]:size-5">
                <User />
              </div>
            }
          />
          <div className="min-w-0">
            <div className={cn("truncate text-md font-medium", c.name ? "text-fg-1" : "text-fg-2 italic")}>
              {c.label}
            </div>
            <div className="text-xs text-fg-3">
              {refinding ? "Being found again…" : `${formatScreenTime(c.screenSec ?? 0)} · ${plural(c.shots, "shot")}`}
            </div>
          </div>
        </div>
        {c.unsure > 0 && (
          <span className="flex items-center gap-1.5 rounded-full border border-accent/30 bg-accent/10 px-2.5 py-0.5 text-xs font-semibold text-accent">
            <span className="size-1.5 rounded-full bg-accent" />
            {c.unsure} to review
          </span>
        )}
      </Link>
    </ContextArea>
  )
}
