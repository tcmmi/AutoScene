import { PuzzlePiece } from "@/components/icons"
import { cn } from "@/lib/utils"
import { pluginPicture } from "./plugin-items"

/**
 * A plugin's icon, else the puzzle piece. Only ever a picture: an SVG in an
 * `<img>` runs no script and loads nothing, so it is never drawn inline.
 */
export function PluginIcon({ src, className }: { src: string | null | undefined; className?: string }) {
  const url = pluginPicture(src)
  if (url) {
    return <img src={url} alt="" draggable={false} className={cn("size-4 shrink-0 object-contain", className)} />
  }
  return <PuzzlePiece className={className} />
}
