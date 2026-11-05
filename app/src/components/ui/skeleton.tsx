import type * as React from "react"
import { cn } from "@/lib/utils"

export interface SkeletonProps extends React.ComponentProps<"div"> {
  /** The sweep of light. Off, it is a still block. */
  shimmer?: boolean
}

/** Where a card, portrait or line of text will be once it has loaded. Its shape comes from `className`. */
export function Skeleton({ className, shimmer = true, ...props }: SkeletonProps) {
  return <div className={cn("rounded-lg", shimmer ? "skeleton" : "bg-hover", className)} {...props} />
}
