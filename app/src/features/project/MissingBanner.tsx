import { useState } from "react"
import { FolderOpen, Warning } from "@/components/icons"
import type { MediaItem } from "@/bindings"
import { Banner } from "@/components/ui/banner"
import { Button } from "@/components/ui/button"
import { plural } from "@/features/cast/cast"
import { locateFolder } from "./relink"

/**
 * Episodes whose files can't be found (moved, renamed, on a drive that
 * isn't connected): one place to show AutoScene where they are now. Their
 * analysis and the decisions about them wait meanwhile.
 */
export function MissingBanner({
  pid,
  media,
  movie,
  className,
}: {
  pid: string
  media: MediaItem[] | undefined
  movie?: boolean
  className?: string
}) {
  const missing = media?.filter((m) => m.status === "missing").length ?? 0
  // Looking probes a few files per episode: one search at a time.
  const [looking, setLooking] = useState(false)
  if (missing === 0) return null
  const what = movie ? plural(missing, "file") : plural(missing, "episode")
  return (
    <Banner
      tone="accent"
      icon={<Warning weight="fill" />}
      title={`${what} can't be found`}
      description="Moved, renamed or on a drive that isn't connected? Their analysis and your decisions wait for them."
      actions={
        <Button
          variant="secondary"
          busy={looking}
          onClick={() => {
            setLooking(true)
            void locateFolder(pid, missing, movie).finally(() => setLooking(false))
          }}
        >
          <FolderOpen /> Locate folder…
        </Button>
      }
      className={className}
    />
  )
}
