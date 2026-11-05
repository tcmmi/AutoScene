import { assetUrl } from "@/lib/media"
import { cn } from "@/lib/utils"

/**
 * A character's round face, beside their name, so the picture itself says
 * nothing to a screen reader. Empty until they have a picture. `ring` marks
 * who is in the shot. Its size comes from `className` (32 px without one).
 */
export function Avatar({ path, ring, className }: { path?: string | null; ring?: boolean; className?: string }) {
  return (
    <span className={cn("size-8 shrink-0 overflow-hidden rounded-full bg-inset", ring && "ring-2 ring-accent/70", className)}>
      {path && <img src={assetUrl(path)} alt="" className="size-full object-cover" />}
    </span>
  )
}
