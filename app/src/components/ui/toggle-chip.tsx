import type * as React from "react"
import { cn } from "@/lib/utils"

/** A pill that is on or off, for a filter. Its children are the label, with any icon. */
export function ToggleChip({
  pressed,
  className,
  ...props
}: Omit<React.ComponentProps<"button">, "title"> & { pressed: boolean }) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      className={cn(
        "inline-flex h-8 items-center gap-1.5 rounded-full border px-3 text-sm font-medium transition-[color,background-color,border-color] duration-[var(--dur-fast)]",
        pressed
          ? "border-accent-line bg-accent-subtle text-fg-1"
          : "border-hairline bg-hover text-fg-2 hover:border-white/20 hover:text-fg-1",
        className,
      )}
      {...props}
    />
  )
}
