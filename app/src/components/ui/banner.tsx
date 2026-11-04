import type * as React from "react"
import { cn } from "@/lib/utils"

/**
 * Something the person has to know, with the buttons that answer it, at the
 * top of the view it is about. Gold (`accent`) asks for an action that is
 * theirs to take; red (`danger`) says something is wrong or was taken away.
 * `icon` is a filled Phosphor icon; the banner colours it.
 */
export function Banner({
  tone,
  icon,
  title,
  description,
  actions,
  className,
}: {
  tone: "accent" | "danger"
  icon: React.ReactNode
  title: React.ReactNode
  description?: React.ReactNode
  actions?: React.ReactNode
  className?: string
}) {
  return (
    <div
      role="status"
      className={cn(
        "flex flex-wrap items-center gap-x-4 gap-y-3 rounded-2xl border px-5 py-4",
        tone === "accent" ? "border-accent-line/50 bg-accent-subtle" : "border-danger/30 bg-danger-subtle",
        className,
      )}
    >
      <span aria-hidden className={cn("flex shrink-0 [&_svg]:size-5", tone === "accent" ? "text-accent" : "text-danger")}>
        {icon}
      </span>
      <div className="min-w-0 flex-1">
        <p className="text-md font-medium text-fg-1">{title}</p>
        {description && <p className="text-sm text-fg-2">{description}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  )
}
