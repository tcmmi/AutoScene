import { useRef } from "react"
import { Keyboard } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Kbd } from "@/components/ui/kbd"
import { useAppActions } from "@/lib/actions"
import { PLAYER_SHORTCUTS, capGroups, useKeymap } from "@/lib/shortcuts"

/**
 * The player's keys, from the shortcut registry and the keymap (what is
 * listed is what works). A dialog over the player, not a trip to Settings:
 * the playhead stays where it is, and the keys can be tried right after.
 */
export function PlayerKeysDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const { goSettings } = useAppActions()
  const km = useKeymap()
  const content = useRef<HTMLDivElement>(null)
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      {/* The list scrolls when the window is short (1024×640); the title and "Change keys…" stay. */}
      <DialogContent
        ref={content}
        title="Player keys"
        // Nothing to fill in or choose: the dialog itself takes the focus, so
        // no button is ringed on opening; Tab still reaches them, Esc closes.
        onOpenAutoFocus={(e) => {
          e.preventDefault()
          content.current?.focus()
        }}
        className="flex max-h-[calc(100vh-48px)] w-[min(520px,calc(100vw-32px))] flex-col"
      >
        <ul className="-mx-1 min-h-0 space-y-2.5 overflow-y-auto px-1 text-sm text-fg-2">
          {PLAYER_SHORTCUTS.map((s) => {
            const groups = capGroups(s, km)
            return (
              <li key={s.id} className="flex items-center justify-between gap-4">
                <span className="min-w-0">{s.label}</span>
                <span className="flex shrink-0 items-center gap-2">
                  {groups.length > 0 ? (
                    groups.map((g, i) => (g.length > 0 ? <Kbd key={i} keys={g} /> : <NoKey key={i} />))
                  ) : (
                    <NoKey />
                  )}
                </span>
              </li>
            )
          })}
        </ul>
        <DialogFooter className="shrink-0">
          {/* The label ends where the keys do. */}
          <Button
            variant="ghost"
            size="sm"
            className="-mr-3.5"
            onClick={() => {
              onOpenChange(false)
              goSettings("shortcuts")
            }}
          >
            <Keyboard /> Change keys…
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** A key the person took away. */
function NoKey() {
  return <span className="text-xs text-fg-3">No key</span>
}
