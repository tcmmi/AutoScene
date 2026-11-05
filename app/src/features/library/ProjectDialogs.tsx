import { useState } from "react"
import { useNavigate, useRouterState } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
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

function DetailsForm({ project, onDone }: { project: ProjectSummary; onDone: () => void }) {
  const { t } = useTranslation(["library", "common"])
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
  // Anime is analysed differently from the start (see `Project::set_content`).
  const apart = t("details.animeApart")
  const locked: Partial<Record<ContentKind, string>> =
    project.content === "anime" ? { live: apart, animation: apart, mixed: apart } : { anime: apart }
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
      const what = t(changedNow ? "details.partlyChanged" : "details.changeFailed")
      toast.error(what, { description: errorMessage(e) })
      if (changedNow) onDone()
    } finally {
      setBusy(false)
    }
  }
  return (
    <DialogContent title={t("details.title")} description={t("details.description")}>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          void save()
        }}
      >
        <Field label={t("details.titleLabel")} htmlFor="details-title">
          <Input id="details-title" autoFocus value={title} onChange={(e) => setTitle(e.target.value)} />
        </Field>
        <Field
          label={t("footage.label")}
          hint={researched ? t("details.refilmedResearched") : refilmed ? t("details.refilmed") : footageBlurb(content)}
          className="mt-4 items-start"
        >
          <FootagePicker value={content} onChange={setContent} locked={locked} />
        </Field>
        <Field
          label={t("newProject.fastScan")}
          htmlFor="details-fast-scan"
          hint={fastScanHint(content, rescanned, fast, scanDefaults)}
          className="mt-4 items-start"
        >
          <Switch id="details-fast-scan" checked={fast} onCheckedChange={setFast} />
        </Field>
        <DialogFooter>
          <Button variant="ghost" onClick={onDone}>
            {t("common:action.cancel")}
          </Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!title.trim() || !(retitled || refilmed || rescanned)}>
            {t("details.save")}
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
  const { t } = useTranslation("library")
  const [deleteFiles, setDeleteFiles] = useState(true)
  // Its exports and plugin runs stop too, which would otherwise go without a
  // word. Said once, as it opens, so the dialog never moves under the pointer.
  const [stops] = useState(() => stopsFirst(unfinishedJobs((j) => j.projectId === project.id).map((j) => j.kind)))
  const navigate = useNavigate()
  const inProject = useRouterState({ select: (s) => s.location.pathname.startsWith(`/p/${project.id}`) })
  const remove = async () => {
    await commands.projectDelete(project.id, deleteFiles)
    if (inProject) void navigate({ to: "/" })
    toast(t("remove.removed", { title: project.title }))
  }
  return (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && onDone()}
      title={t("remove.title", { title: project.title })}
      description={stops ? t("remove.descriptionStops", { stops }) : t("remove.description")}
      answer={t(deleteFiles ? "remove.answerDelete" : "remove.answerKeep")}
      failure={t("remove.failed")}
      onConfirm={remove}
    >
      <OptionRow
        label={t("remove.deleteAnalysis")}
        description={t("remove.deleteAnalysisHint")}
        checked={deleteFiles}
        onCheckedChange={setDeleteFiles}
      />
    </ConfirmDialog>
  )
}
