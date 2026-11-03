import { ArrowCounterClockwise, Copy, FolderOpen, MagnifyingGlass, Play, Scissors, Trash, UserFocus, X } from "@/components/icons"
import { toast } from "sonner"
import type { MediaItem } from "@/bindings"
import type { MenuKit } from "@/components/ui/menu-actions"
import { commands, errorMessage } from "@/lib/ipc"
import { cancelJob, cancelLabel } from "@/lib/jobs"
import { copyText, reveal } from "@/lib/os"
import { useMediaJob } from "@/state/activity"
import { locateFile } from "./relink"

/**
 * What an episode (or a movie's file) offers wherever it is shown — its row,
 * its card, their ⋯ menus: open it, its file, stop or redo its work, remove
 * it. Finding a stage again waits (disabled: it can't apply yet) while the
 * analysis is finding that stage anyway; "Cancel analysis" right above it
 * says what runs. Removal stays while it works: the backend stops the
 * episode's work before it removes it.
 */
export function EpisodeItems({
  kit,
  projectId,
  media: m,
  onOpen,
  onRemove,
  openShortcut,
}: {
  kit: MenuKit
  projectId: string
  media: MediaItem
  onOpen: () => void
  onRemove: () => void
  /** The key that opens it where it is shown (Enter on a focused row). */
  openShortcut?: string[]
}) {
  const { Item, Separator } = kit
  // Its unfinished jobs by kind (it can have several at once): each item
  // stops the one it names.
  const ingest = useMediaJob(m.id, "ingest")
  const analysis = useMediaJob(m.id, "analyze")
  const preview = useMediaJob(m.id, "proxy")
  const working = [ingest, analysis, preview].filter((j) => j !== undefined)
  const ready = m.status === "ready"
  // The analysis under way (or waiting) finds every stage that isn't ready
  // yet, from its checkpoints: finding one of those again now would only
  // start it over at the back of the queue. Shots found while characters are
  // being found can still be found again (it starts over from them). A
  // preview or an indexing never holds a redo back.
  const findingShots = analysis !== undefined && m.shots !== "ready"
  const findingFaces = analysis !== undefined && m.faces !== "ready"
  const redo = (stage: "shots" | "faces") =>
    void commands
      .mediaAnalyze(projectId, m.id, stage)
      .catch((e) => toast.error("Couldn't start the analysis", { description: errorMessage(e) }))
  return (
    <>
      <Item disabled={!ready} onSelect={onOpen} shortcut={openShortcut}>
        <Play /> Open
      </Item>
      <Item onSelect={() => reveal(m.path)}>
        <FolderOpen /> Show in folder
      </Item>
      <Item onSelect={() => void copyText(m.path, "file path")}>
        <Copy /> Copy file path
      </Item>
      {m.status === "missing" && (
        <Item onSelect={() => void locateFile(projectId, m)}>
          <MagnifyingGlass /> Locate file…
        </Item>
      )}
      {m.status === "failed" && (
        <Item
          onSelect={() =>
            void commands
              .mediaRetry(projectId, m.id)
              .catch((e) => toast.error("Couldn't index it again", { description: errorMessage(e) }))
          }
        >
          <ArrowCounterClockwise /> Index again
        </Item>
      )}
      {working.map((j) => (
        <Item key={j.id} onSelect={() => cancelJob(j)}>
          <X /> {cancelLabel(j.kind)}
        </Item>
      ))}
      {ready && (
        <Item disabled={findingShots} onSelect={() => redo("shots")}>
          <Scissors /> Find shots again
        </Item>
      )}
      {ready && m.shots === "ready" && (
        <Item disabled={findingFaces} onSelect={() => redo("faces")}>
          <UserFocus /> Find characters again
        </Item>
      )}
      <Separator />
      <Item destructive onSelect={onRemove}>
        <Trash /> Remove from project…
      </Item>
    </>
  )
}
