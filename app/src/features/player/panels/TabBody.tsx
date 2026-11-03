import type * as React from "react"
import { ContextArea, type MenuContent } from "@/components/ui/context-area"

/**
 * One tab of the player's details panel: a section named by its tab, with
 * the tab's right-click menu. No card around it: the panel is the surface,
 * and space, not lines, separates what is in it.
 */
export function TabBody({ label, menu, children }: { label: string; menu: MenuContent; children: React.ReactNode }) {
  return (
    <ContextArea menu={menu}>
      <section aria-label={label} className="min-h-full">
        {children}
      </section>
    </ContextArea>
  )
}
