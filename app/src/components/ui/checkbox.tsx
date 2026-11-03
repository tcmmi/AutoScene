import * as React from "react"
import { Checkbox as C } from "radix-ui"
import { Check } from "@/components/icons"
import { cn } from "@/lib/utils"

/**
 * A tick box with its label, only for an option that matters once ("Don't
 * show this again"); settings use `Switch`. No native checkbox (DESIGN.md: no
 * browser controls). The whole label is the target, Space ticks it, and the
 * focus ring is the app's own.
 */
export function Checkbox({
  label,
  className,
  id,
  ...props
}: Omit<React.ComponentProps<typeof C.Root>, "children"> & { label: React.ReactNode }) {
  const auto = React.useId()
  const box = id ?? auto
  return (
    <label
      htmlFor={box}
      className={cn("group inline-flex select-none items-center gap-2.5 text-sm text-fg-2", className)}
    >
      <C.Root
        id={box}
        className={cn(
          "flex size-[18px] shrink-0 items-center justify-center rounded-[5px] border border-white/25 bg-white/[0.04]",
          "transition-[background-color,border-color] duration-[var(--dur-fast)] ease-out group-hover:border-white/40",
          "data-[state=checked]:border-accent data-[state=checked]:bg-accent disabled:opacity-40",
        )}
        {...props}
      >
        <C.Indicator className="text-on-accent data-[state=checked]:animate-fade-in">
          <Check weight="bold" className="size-3" />
        </C.Indicator>
      </C.Root>
      <span className="transition-colors duration-[var(--dur-fast)] group-hover:text-fg-1">{label}</span>
    </label>
  )
}
