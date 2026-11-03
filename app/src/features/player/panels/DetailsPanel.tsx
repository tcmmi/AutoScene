import { useState } from "react"
import { Keyboard } from "@/components/icons"
import type { MediaItem, Rational, Segment } from "@/bindings"
import { Button } from "@/components/ui/button"
import { Segmented } from "@/components/ui/segmented"
import { Tooltip } from "@/components/ui/tooltip"
import type { Lane } from "@/lib/presence"
import { cn } from "@/lib/utils"
import { type PlayerTab, playerTabOf, useUi } from "@/state/ui"
import type { FrameStore } from "../frame-store"
import { CastTab } from "./CastTab"
import { FileTab } from "./FileTab"

const TABS: { value: PlayerTab; label: string }[] = [
  { value: "cast", label: "Cast" },
  { value: "file", label: "File" },
]

/**
 * The player's details: one panel beside the picture with two tabs, Cast
 * (the shot and who is in it) and File, and the Player keys button. It
 * remembers its tab with its open or closed state (`useUi`, per user).
 *
 * The tabs are a `Segmented` pill: it switches on click, not on mousedown
 * (Radix Tabs would, and the player cancels mousedown on buttons so Space
 * keeps playing). Closed, the panel isn't rendered. It opens at once; its
 * content fades in only when it is opened later, not when the player mounts
 * (the player mounts again with every episode and audio track).
 */
export function DetailsPanel({
  open,
  pid,
  media,
  fps,
  frames,
  lanes,
  segments,
  store,
  tc,
  focus,
  onFocus,
  onMarkShot,
  onSeek,
  onGo,
  onKeys,
}: {
  open: boolean
  pid: string
  media: MediaItem
  fps: Rational
  /** The episode's frame count. */
  frames: number
  lanes: Lane[]
  segments: Segment[]
  store: FrameStore
  tc: (f: number) => string
  focus: string | null
  onFocus: (id: string | null) => void
  onMarkShot: () => void
  onSeek: (f: number) => void
  onGo: (f: number) => void
  /** Open the Player keys dialog. */
  onKeys: () => void
}) {
  const tab = playerTabOf(useUi((s) => s.playerTab))
  const setTab = useUi((s) => s.setPlayerTab)
  // Closed at some point since the player mounted: opening it now fades it in.
  const [wasClosed, setWasClosed] = useState(!open)
  if (!open && !wasClosed) setWasClosed(true)
  if (!open) return null
  return (
    <aside
      id="player-details"
      aria-label="Details"
      className={cn(
        "flex w-[320px] shrink-0 flex-col border-l border-white/[0.06] bg-canvas",
        wasClosed && "motion-safe:animate-fade-in",
      )}
    >
      <div className="flex shrink-0 items-center gap-2 px-4 pt-3 pb-2">
        <Segmented
          aria-label="Details sections"
          className="flex flex-1 [&>*]:flex-1 [&>*]:justify-center"
          value={tab}
          onChange={setTab}
          options={TABS}
        />
        <Tooltip label="Player keys">
          <Button size="icon-sm" variant="ghost" aria-label="Player keys" onClick={onKeys}>
            <Keyboard />
          </Button>
        </Tooltip>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-4 pt-2 pb-5">
        {tab === "cast" ? (
          <CastTab
            pid={pid}
            media={media}
            lanes={lanes}
            segments={segments}
            store={store}
            tc={tc}
            focus={focus}
            onFocus={onFocus}
            onMarkShot={onMarkShot}
            onSeek={onSeek}
            onGo={onGo}
          />
        ) : (
          <FileTab pid={pid} media={media} fps={fps} frames={frames} tc={tc} />
        )}
      </div>
    </aside>
  )
}
