import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/utils"

/**
 * Chips: short facts (format, state, counts). `media` sits on big pictures;
 * `picture`, with `size="sm"`, is the quiet one on a card's picture: a length,
 * an episode, why a shot is left out.
 */
const badgeVariants = cva(
  "inline-flex items-center whitespace-nowrap rounded-full font-medium",
  {
    variants: {
      tone: {
        neutral: "bg-white/[0.07] text-fg-2",
        accent: "bg-accent-subtle text-accent",
        danger: "bg-danger-subtle text-danger",
        outline: "border border-soft text-fg-2",
        media: "border border-white/15 bg-black/35 text-white/90 backdrop-blur-md",
        picture: "bg-black/55 font-normal text-white/90 backdrop-blur-md",
      },
      size: {
        md: "h-6 gap-1.5 px-2.5 text-xs [&_svg]:size-3.5",
        sm: "h-5 gap-1 px-2 text-2xs [&_svg]:size-3",
      },
    },
    defaultVariants: { tone: "neutral", size: "md" },
  },
)

export function Badge({
  className,
  tone,
  size,
  ...props
}: Omit<React.ComponentProps<"span">, "title"> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ tone, size }), className)} {...props} />
}
