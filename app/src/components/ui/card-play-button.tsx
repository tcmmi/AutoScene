import type * as React from "react"
import { Play } from "@/components/icons"
import { cn } from "@/lib/utils"

/**
 * The play button in the corner of a shot's picture: out of sight until the
 * pointer is over the card (a `group` around it) or the keyboard reaches it.
 */
export function CardPlayButton({
  className,
  ...props
}: Omit<React.ComponentProps<"button">, "children" | "title"> & { "aria-label": string }) {
  return (
    <button
      type="button"
      className={cn(
        "absolute top-2 right-2 flex size-7 items-center justify-center rounded-full bg-black/55 text-white backdrop-blur-md",
        "opacity-0 transition-opacity group-hover:opacity-100 hover:bg-black/75 focus-visible:opacity-100",
        className,
      )}
      {...props}
    >
      <Play weight="fill" className="size-3.5" />
    </button>
  )
}
