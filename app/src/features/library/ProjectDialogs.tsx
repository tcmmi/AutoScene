import { useState } from "react"
import { useNavigate, useRouterState } from "@tanstack/react-router"
import { toast } from "sonner"
import type { ContentKind, ProjectSummary } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ConfirmDialog } from "@/components/ui/confirm-dialog"
import { Dialog, DialogContent, DialogFooter } from "@/components/ui/dialog"
import { Field, Input } from "@/components/ui/input"
import { OptionRow } from "@/components/ui/option-row"
import { Switch } from "@/components/ui/switch"
import { fastScanHint } from "@/lib/fast-scan"
import { footageBlurb } from "@/lib/footage"
import { commands, errorMessage } from "@/lib/ipc"
import { stopsFirst } from "@/lib/jobs"
import { useScanDefaults } from "@/lib/queries"
import { unfinishedJobs } from "@/state/activity"
import { FootagePicker } from "./FootagePicker"

/** A project's title, what its footage is, and how its faces are found. */
export function ProjectDetailsDialog({
  project,
  onOpenChange,
}: {
  project: ProjectSummary | null
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={project != null} onOpenChange={onOpenChange}>
      {project && <DetailsForm project={project} onDone={() => onOpenChange(false)} />}
    </Dialog>
  )
}

/** Anime is analysed differently from the start (see `Project::set_content`). */
const ANIME_APART = "Anime is analysed differently from the start. Add the videos as a new anime project instead."

function DetailsForm({ project, onDone }: { project: ProjectSummary; onDone: () => void }) {
  const [title, setTitle] = useState(project.title)
  const [content, setContent] = useState<ContentKind>(project.content)
  const wasFast = project.fastScan ?? false
  const [fast, setFast] = useState(wasFast)
  const [busy, setBusy] = useState(false)
  // Says when Fast Scan would suit this computer's graphics.
  const { data: scanDefaults } = useScanDefaults()
  const retitled = title.trim() !== project.title
  const refilmed = content !== project.content
  // Fast Scan finds faces with other models: they are all found again.
  const rescanned = fast !== wasFast
  // The full pass searches 3D animation for faces harder; Fast Scan looks the same way (see `Project::faces_plan`).
  const researched = refilmed && !fast && !rescanned && (content === "animation" || project.content === "animation")
  const locked: Partial<Record<ContentKind, string>> =
    project.content === "anime"
      ? { live: ANIME_APART, animation: ANIME_APART, mixed: ANIME_APART }
      : { anime: ANIME_APART }
  const save = async () => {
    setBusy(true)
    // The footage first: it is the one that can be refused.
    let changedNow = false
    try {
      if (refilmed) {
        await commands.projectSetContent(project.id, content)
        changedNow = true
      }
      if (rescanned) {
        await commands.projectSetFastScan(project.id, fast)
        changedNow = true
      }
      if (retitled) await commands.projectRename(project.id, title)
      onDone()
    } catch (e) {
      const what = changedNow ? "Some details changed, but not all of them" : "Couldn't change the project"
      toast.error(what, { description: errorMessage(e) })
      if (changedNow) onDone()
    } finally {
      setBusy(false)
    }
  }
  return (
    <DialogContent title="Project details" description="The project folder on disk keeps its name.">
      <form
        onSubmit={(e) => {
          e.preventDefault()
          void save()
        }}
      >
        <Field label="Title" htmlFor="details-title">
          <Input id="details-title" autoFocus value={title} onChange={(e) => setTitle(e.target.value)} />
        </Field>
        <Field
          label="Footage"
          hint={
            researched
              ? "Faces are found again for it, then the characters regrouped. Your names and decisions stay."
              : refilmed
                ? "The characters are grouped again for it. Your names and decisions stay."
                : footageBlurb(content)
          }
          className="mt-4 items-start"
        >
          <FootagePicker value={content} onChange={setContent} locked={locked} />
        </Field>
        <Field
          label="Fast Scan"
          htmlFor="details-fast-scan"
          hint={fastScanHint(content, rescanned, fast, scanDefaults)}
          className="mt-4 items-start"
        >
          <Switch id="details-fast-scan" checked={fast} onCheckedChange={setFast} />
        </Field>
        <DialogFooter>
          <Button variant="ghost" onClick={onDone}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!title.trim() || !(retitled || refilmed || rescanned)}>
            Save
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  )
}

export function DeleteProjectDialog({
  project,
  onOpenChange,
}: {
  project: ProjectSummary | null
  onOpenChange: (open: boolean) => void
}) {
  return project && <DeleteForm project={project} onDone={() => onOpenChange(false)} />
}

function DeleteForm({ project, onDone }: { project: ProjectSummary; onDone: () => void }) {
  const [deleteFiles, setDeleteFiles] = useState(true)
  // Its exports and plugin runs stop too, which would otherwise go without a
  // word. Said once, as it opens, so the dialog never moves under the pointer.
  const [stops] = useState(() => stopsFirst(unfinishedJobs((j) => j.projectId === project.id).map((j) => j.kind)))
  const navigate = useNavigate()
  const inProject = useRouterState({ select: (s) => s.location.pathname.startsWith(`/p/${project.id}`) })
  const remove = async () => {
    await commands.projectDelete(project.id, deleteFiles)
    if (inProject) void navigate({ to: "/" })
    toast(`Removed “${project.title}”`)
  }
  return (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && onDone()}
      title={`Remove “${project.title}”?`}
      description={`Your source videos and exported clips are never touched.${stops ? ` ${stops}` : ""}`}
      answer={deleteFiles ? "Delete project" : "Remove from library"}
      failure="Couldn't remove the project"
      onConfirm={remove}
    >
      <OptionRow
        label="Delete its analysis too"
        description="Shots, faces, the cast with your names and decisions, posters and previews (the .autoscene folder). Added again later, the videos are analysed from scratch."
        checked={deleteFiles}
        onCheckedChange={setDeleteFiles}
      />
    </ConfirmDialog>
  )
}
