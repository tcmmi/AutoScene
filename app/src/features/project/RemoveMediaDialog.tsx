import { useState } from "react"
import type { MediaItem } from "@/bindings"
import { ConfirmDialog } from "@/components/ui/confirm-dialog"
import { commands } from "@/lib/ipc"
import { stopsFirst } from "@/lib/jobs"
import { releaseTitle } from "@/lib/media"
import { unfinishedJobs } from "@/state/activity"

/**
 * Ask before a file leaves the project: its shots, faces and the decisions
 * made about those faces go with it, and can't be brought back by adding the
 * file again (that analyses it from scratch). Work running on it stops
 * first, and the dialog says which, last.
 */
export function RemoveMediaDialog({
  projectId,
  media,
  onOpenChange,
}: {
  projectId: string
  media: MediaItem | null
  onOpenChange: (open: boolean) => void
}) {
  return media && <RemoveForm projectId={projectId} media={media} onDone={() => onOpenChange(false)} />
}

const WHAT_GOES =
  "Its shots and faces go, and with them your decisions about those faces. Characters you haven't named who appear only here leave the cast."
const NOTHING_YET = "Nothing has been found in it yet."

/** Whether shots or faces of the episode are stored, so removing it loses them. */
function hasAnalysis(m: MediaItem): boolean {
  return m.shotCount != null || m.faceCount != null || m.shots === "ready" || m.faces === "ready"
}

function RemoveForm({ projectId, media: m, onDone }: { projectId: string; media: MediaItem; onDone: () => void }) {
  // Said once, as it opens: a job ending meanwhile never moves the dialog
  // under the pointer, and the removal stops what runs then anyway.
  const [stops] = useState(() => stopsFirst(unfinishedJobs((j) => j.mediaId === m.id).map((j) => j.kind)))
  // A file still being indexed (often the one added by mistake) has nothing
  // to lose yet, so the dialog doesn't list shots and faces it never had.
  // Counts, not the status: an episode indexed again or relinked keeps its
  // analysis while it is queued. Fixed at open, like the sentence above.
  const [found] = useState(() => hasAnalysis(m))
  const name = m.label ? `${m.label} · ${releaseTitle(m.fileName)}` : releaseTitle(m.fileName)
  return (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && onDone()}
      title={`Remove “${name}”?`}
      description={`${found ? WHAT_GOES : NOTHING_YET} The video file itself isn't touched.${stops ? ` ${stops}` : ""}`}
      answer="Remove from project"
      failure="Couldn't remove it"
      onConfirm={() => commands.mediaRemove(projectId, m.id)}
    />
  )
}
