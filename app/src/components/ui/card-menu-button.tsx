import type * as React from "react"
import { DotsThree } from "@/components/icons"
import { DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { cn } from "@/lib/utils"

/**
 * The ⋯ on a card's picture, inside a `DropdownMenu`: out of sight until the
 * pointer is over the picture (a `group` or `group/art` around it) or the
 * keyboard reaches it, and up while its menu is. `over` says what the pointer
 * has to be over: the whole `group` around it (default) or only the picture,
 * a `group/art` (then a plain `group` further out doesn't show it). Lifting
 * the card on focus is the card's own business.
 */
const OVER = {
  card: "group-hover:opacity-100",
  picture: "group-hover/art:opacity-100",
}

export function CardMenuButton({
  over = "card",
  className,
  ...props
}: Omit<React.ComponentProps<typeof DropdownMenuTrigger>, "children" | "asChild"> & { "aria-label": string; over?: keyof typeof OVER }) {
  return (
    <DropdownMenuTrigger
      className={cn(
        "absolute top-3 right-3 flex size-8 items-center justify-center rounded-full border border-white/15 bg-black/40 text-white opacity-0 backdrop-blur-md transition-opacity",
        OVER[over],
        "hover:bg-black/60 focus-visible:opacity-100 data-[state=open]:opacity-100",
        className,
      )}
      {...props}
    >
      <DotsThree className="size-5" weight="bold" />
    </DropdownMenuTrigger>
  )
}
