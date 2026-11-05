import type * as React from "react"
import { useState } from "react"
import { Link } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { FilmSlate, Plus, Scissors, UserFocus, Users } from "@/components/icons"
import type { ProjectSummary } from "@/bindings"
import { AutoSceneMark } from "@/components/brand/AutoSceneMark"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ErrorState } from "@/components/ui/error-state"
import { Hero } from "@/components/ui/hero"
import { contextKit } from "@/components/ui/menu-actions"
import { Page } from "@/components/ui/page"
import { Section } from "@/components/ui/shelf"
import { Skeleton } from "@/components/ui/skeleton"
import { TopBar } from "@/components/ui/topbar"
import { useAmbient } from "@/lib/ambient"
import { formatDuration } from "@/lib/format"
import { CARD_GRID } from "@/lib/grid"
import { projectFacts } from "@/lib/projects"
import { useProjects } from "@/lib/queries"
import { useNewProject } from "@/state/new-project"
import { ProjectCard } from "./ProjectCard"
import { DeleteProjectDialog, ProjectDetailsDialog } from "./ProjectDialogs"
import { NewProjectItems, ProjectItems } from "./project-actions"

export function LibraryPage() {
  const { t } = useTranslation("library")
  const { data: projects, isPending, isError, error, refetch } = useProjects()
  const newProject = useNewProject((s) => s.show)
  const [renaming, setRenaming] = useState<ProjectSummary | null>(null)
  const [deleting, setDeleting] = useState<ProjectSummary | null>(null)
  // Only a library that loaded and has nothing in it is new; a failed load says so.
  const empty = projects?.length === 0
  const latest = projects?.[0]

  return (
    <Page
      menu={<NewProjectItems kit={contextKit} page />}
      topbar={
        <TopBar title={t("page.title")} overlay>
          {!empty && (
            <Button size="sm" variant={latest?.poster ? "glass" : "secondary"} onClick={() => newProject()}>
              <Plus weight="bold" /> {t("page.newProject")}
            </Button>
          )}
        </TopBar>
      }
    >
      {isError && projects === undefined ? (
        <ErrorState error={error} title={t("page.loadFailed")} className="h-full" onRetry={() => void refetch()} />
      ) : empty ? (
        <Welcome onCreate={() => newProject()} />
      ) : (
        <>
          {latest ? (
            <Featured project={latest} onRename={() => setRenaming(latest)} onDelete={() => setDeleting(latest)} />
          ) : (
            <div className="h-[clamp(380px,56vh,600px)]" />
          )}
          <Section title={t("page.allProjects")} count={projects?.length} className="-mt-2 pb-14">
            <div className={CARD_GRID}>
              {isPending
                ? Array.from({ length: 3 }, (_, i) => <Skeleton key={i} className="aspect-video rounded-2xl" />)
                : projects!.map((p) => (
                    <ProjectCard key={p.id} project={p} onRename={() => setRenaming(p)} onDelete={() => setDeleting(p)} />
                  ))}
              <ContextArea menu={<NewProjectItems kit={contextKit} />}>
                <button
                  onClick={() => newProject()}
                  className="group flex aspect-video flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-white/15 text-fg-3 transition-colors hover:border-white/30 hover:bg-white/[0.03] hover:text-fg-1"
                >
                  <span className="flex size-12 items-center justify-center rounded-full bg-white/[0.06] transition-colors group-hover:bg-white/[0.12] [&_svg]:size-5">
                    <Plus weight="bold" />
                  </span>
                  <span className="text-md font-medium">{t("page.newProject")}</span>
                  <span className="text-xs text-fg-3">{t("page.dropHint")}</span>
                </button>
              </ContextArea>
            </div>
          </Section>
        </>
      )}
      <ProjectDetailsDialog project={renaming} onOpenChange={(o) => !o && setRenaming(null)} />
      <DeleteProjectDialog project={deleting} onOpenChange={(o) => !o && setDeleting(null)} />
    </Page>
  )
}

/** The project touched last, big: pick up where you left off. */
function Featured({ project: p, onRename, onDelete }: { project: ProjectSummary; onRename: () => void; onDelete: () => void }) {
  const { t } = useTranslation("library")
  // The colour of a picture is the same at any size: read it from the small one.
  const ambient = useAmbient(p.posterSmall ?? p.poster)
  return (
    <ContextArea menu={<ProjectItems kit={contextKit} project={p} onRename={onRename} onDelete={onDelete} />}>
      <Hero path={p.poster} ambient={ambient}>
        <div className="max-w-2xl">
          <p className="eyebrow text-white/70">{t("page.continue")}</p>
          <h1 className="display mt-2 text-6xl text-white">{p.title}</h1>
          <p className="mt-3 text-lg text-white/75">
            {projectFacts(p)}
            {p.durationSec ? ` · ${formatDuration(p.durationSec)}` : ""}
          </p>
          <div className="mt-7 flex items-center gap-3">
            <Button size="lg" variant="primary" asChild>
              <Link to="/p/$pid" params={{ pid: p.id }}>
                {t("project.open")}
              </Link>
            </Button>
            <Button size="lg" variant="glass" asChild>
              <Link to="/p/$pid/cast" params={{ pid: p.id }}>
                <UserFocus /> {t("project.cast")}
              </Link>
            </Button>
          </div>
        </div>
      </Hero>
    </ContextArea>
  )
}

export function Welcome({ onCreate }: { onCreate: () => void }) {
  const { t } = useTranslation("library")
  const steps: { icon: React.ReactNode; title: string; body: string }[] = [
    { icon: <FilmSlate />, title: t("welcome.steps.add.title"), body: t("welcome.steps.add.body") },
    { icon: <Users />, title: t("welcome.steps.cast.title"), body: t("welcome.steps.cast.body") },
    { icon: <Scissors />, title: t("welcome.steps.export.title"), body: t("welcome.steps.export.body") },
  ]
  return (
    <div className="relative flex min-h-full flex-col items-center px-10 pt-[18vh] pb-16 text-center">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-x-0 top-0 h-[70vh] bg-[radial-gradient(50%_60%_at_50%_20%,rgb(224_180_92/0.14),transparent_70%)]"
      />
      <AutoSceneMark className="relative size-20 drop-shadow-[0_12px_40px_rgb(224_180_92/0.35)]" />
      <h1 className="display relative mt-8 text-6xl text-fg-1">{t("welcome.title")}</h1>
      <p className="relative mt-4 max-w-xl text-lg text-fg-2">
        {t("welcome.intro")}
      </p>
      <div className="relative mt-9 flex items-center gap-4">
        <Button variant="primary" size="lg" onClick={onCreate}>
          <Plus weight="bold" /> {t("page.newProject")}
        </Button>
        <span className="text-sm text-fg-3">{t("page.dropHint")}</span>
      </div>
      <ol className="relative mt-16 grid w-full max-w-4xl grid-cols-1 gap-4 text-left md:grid-cols-3">
        {steps.map((s, i) => (
          <li key={s.title} className="rounded-2xl border border-white/[0.08] bg-white/[0.03] p-6 backdrop-blur-xl">
            <div className="flex items-center gap-3">
              <span className="flex size-10 items-center justify-center rounded-xl bg-white/[0.07] text-fg-1 [&_svg]:size-5">
                {s.icon}
              </span>
              <span className="timecode text-sm text-fg-3">0{i + 1}</span>
            </div>
            <h3 className="mt-4 text-lg font-semibold text-fg-1">{s.title}</h3>
            <p className="mt-1.5 text-sm leading-[21px] text-fg-2">{s.body}</p>
          </li>
        ))}
      </ol>
    </div>
  )
}
