import type * as React from "react"
import { useState } from "react"
import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { CaretRight, DotsThree, FilePlus, FolderOpen, FolderPlus, Package, PencilSimple, Plus } from "@/components/icons"
import type { MediaItem, ProjectSummary } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Hero } from "@/components/ui/hero"
import { Page } from "@/components/ui/page"
import { Section, Shelf } from "@/components/ui/shelf"
import { TopBar } from "@/components/ui/topbar"
import { AniListNames } from "@/features/cast/AniListNames"
import { CharacterCard } from "@/features/cast/CharacterCard"
import { splitHidden } from "@/features/cast/cast"
import { PickCharacter } from "@/features/cast/PickCharacter"
import { ProjectDetailsDialog } from "@/features/library/ProjectDialogs"
import { useAmbient } from "@/lib/ambient"
import { formatDuration } from "@/lib/format"
import { reveal } from "@/lib/os"
import { pickFolder, pickVideoFiles } from "@/lib/media"
import { addToProject, projectFacts } from "@/lib/projects"
import { firstFailure, useCast, useMediaList, useProject } from "@/lib/queries"
import { AnalysisStatus } from "./AnalysisStatus"
import { EpisodeCard } from "./EpisodeCard"
import { MissingBanner } from "./MissingBanner"
import { ProjectLoadError } from "./ProjectLoadError"
import { ProjectNav } from "./ProjectNav"
import { RemoveMediaDialog } from "./RemoveMediaDialog"

/** A project's front page: its picture and title, its cast, its episodes. */
export function OverviewPage() {
  const { t } = useTranslation("project")
  const { pid } = useParams({ from: "/p/$pid/" })
  const projectQuery = useProject(pid)
  const mediaQuery = useMediaList(pid)
  const castQuery = useCast(pid)
  const { data: project } = projectQuery
  const { data: media } = mediaQuery
  const { data: cast } = castQuery
  const [renaming, setRenaming] = useState<ProjectSummary | null>(null)
  const [choosing, setChoosing] = useState(false)
  const [removing, setRemoving] = useState<MediaItem | null>(null)
  const navigate = useNavigate()
  const pictured = project?.poster ? project : media?.find((m) => m.poster)
  const backdrop = pictured?.poster ?? null
  const ambient = useAmbient(pictured?.posterSmall ?? backdrop)
  const { shown } = splitHidden(cast)
  const movie = project?.kind === "movie"
  const failure = firstFailure(projectQuery, mediaQuery, castQuery)

  if (failure) {
    return (
      <Page topbar={<TopBar title={project?.title} nav={<ProjectNav pid={pid} />} />}>
        <ProjectLoadError error={failure.error} onRetry={failure.retry} />
      </Page>
    )
  }

  return (
    <Page
      topbar={<TopBar title={project?.title} overlay nav={<ProjectNav pid={pid} />} />}
      menu={
        <>
          <ContextMenuItem
            onSelect={() => (shown.length > 0 ? setChoosing(true) : void navigate({ to: "/p/$pid/cast", params: { pid } }))}
          >
            <Package /> {t("overview.buildPackMenu")}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => void pickFolder().then((p) => (p ? addToProject(pid, [p]) : undefined))}>
            <FolderPlus /> {movie ? t("overview.addFolderMenu") : t("overview.addEpisodesMenu")}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => void pickVideoFiles().then((p) => addToProject(pid, p))}>
            <FilePlus /> {t("menu.addFiles")}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem onSelect={() => project && setRenaming(project)}>
            <PencilSimple /> {t("menu.editDetails")}
          </ContextMenuItem>
          <ContextMenuItem onSelect={() => project && reveal(project.dir)}>
            <FolderOpen /> {t("menu.showProjectFolder")}
          </ContextMenuItem>
        </>
      }
    >
      <Hero path={backdrop} ambient={ambient}>
        <div className="max-w-3xl">
          <h1 className="display text-6xl leading-[0.95] text-white">{project?.title ?? " "}</h1>
          {project && (
            <p className="mt-4 text-lg text-white/75">
              {projectFacts(project)}
              {project.durationSec ? ` · ${formatDuration(project.durationSec)}` : ""}
              {shown.length > 0 ? ` · ${t("overview.characters", { count: shown.length })}` : ""}
            </p>
          )}
          <div className="mt-7 flex flex-wrap items-center gap-3">
            {shown.length > 0 ? (
              <Button size="lg" variant="primary" onClick={() => setChoosing(true)}>
                <Package weight="bold" /> {t("overview.buildPack")}
              </Button>
            ) : (
              <Button size="lg" variant="primary" asChild>
                <Link to="/p/$pid/cast" params={{ pid }}>
                  <Package weight="bold" /> {t("overview.buildPack")}
                </Link>
              </Button>
            )}
            <Button size="lg" variant="glass" onClick={() => void pickFolder().then((p) => (p ? addToProject(pid, [p]) : undefined))}>
              <FolderPlus /> {movie ? t("overview.addFolder") : t("overview.addEpisodes")}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button size="icon-lg" variant="glass" aria-label={t("overview.more")}>
                  <DotsThree weight="bold" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem onSelect={() => void pickVideoFiles().then((p) => addToProject(pid, p))}>
                  <FilePlus /> {t("menu.addFiles")}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => project && setRenaming(project)}>
                  <PencilSimple /> {t("menu.editDetails")}
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem onSelect={() => project && reveal(project.dir)}>
                  <FolderOpen /> {t("menu.showProjectFolder")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
          <AnalysisStatus pid={pid} className="mt-8" />
        </div>
      </Hero>

      <div className="flex flex-col gap-10 pb-16">
        <MissingBanner pid={pid} media={media} movie={movie} className="mx-10" />
        <AniListNames pid={pid} cast={cast} className="mx-10" />
        <Section
          title={t("overview.cast")}
          count={shown.length || undefined}
          action={
            shown.length > 0 && (
              <SeeAll
                to={`/p/${pid}/cast`}
                label={cast && cast.unsure > 0 ? t("overview.facesToReview", { count: cast.unsure }) : t("overview.allCharacters")}
              />
            )
          }
        >
          {shown.length > 0 ? (
            <Shelf>
              {shown.map((c) => (
                <CharacterCard key={c.id} pid={pid} character={c} className="w-[176px] shrink-0 snap-start" />
              ))}
            </Shelf>
          ) : (
            <Placeholder>
              {(media?.length ?? 0) === 0
                ? t("overview.castEmptyNoEpisodes")
                : t("overview.castEmpty")}
            </Placeholder>
          )}
        </Section>

        <Section
          title={movie ? t("overview.files") : t("overview.episodes")}
          count={media?.length || undefined}
          action={(media?.length ?? 0) > 0 && <SeeAll to={`/p/${pid}/episodes`} label={t("overview.details")} />}
        >
          {(media?.length ?? 0) > 0 ? (
            <Shelf>
              {media!.map((m) => (
                <EpisodeCard
                  key={m.id}
                  pid={pid}
                  media={m}
                  onOpen={() => void navigate({ to: "/p/$pid/timeline/$mid", params: { pid, mid: m.id } })}
                  onRemove={() => setRemoving(m)}
                  className="w-[300px] shrink-0 snap-start"
                />
              ))}
            </Shelf>
          ) : (
            <button
              onClick={() => void pickFolder().then((p) => (p ? addToProject(pid, [p]) : undefined))}
              className="flex h-40 w-full max-w-md flex-col items-center justify-center gap-2 rounded-2xl border border-dashed border-white/15 text-fg-3 transition-colors hover:border-white/30 hover:text-fg-1"
            >
              <Plus weight="bold" className="size-5" />
              <span className="text-md font-medium">{movie ? t("overview.addMovieFolder") : t("overview.addSeasonFolder")}</span>
            </button>
          )}
        </Section>
      </div>
      <ProjectDetailsDialog project={renaming} onOpenChange={(o) => !o && setRenaming(null)} />
      <RemoveMediaDialog projectId={pid} media={removing} onOpenChange={(o) => !o && setRemoving(null)} />
      <PickCharacter
        pid={pid}
        open={choosing}
        onOpenChange={setChoosing}
        title={t("overview.whoseTitle")}
        description={t("overview.whoseDescription")}
        onPick={(p) => {
          setChoosing(false)
          if (p.kind === "existing") void navigate({ to: "/p/$pid/cast/$cid/pack", params: { pid, cid: p.id } })
        }}
      />
    </Page>
  )
}

function SeeAll({ to, label }: { to: string; label: string }) {
  return (
    <Link to={to} className="group flex items-center gap-1 text-sm font-medium text-fg-2 transition-colors hover:text-fg-1">
      {label}
      <CaretRight weight="bold" className="size-3.5 transition-transform group-hover:translate-x-0.5" />
    </Link>
  )
}

function Placeholder({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-32 max-w-xl items-center rounded-2xl border border-white/[0.07] bg-white/[0.02] px-6 text-md text-fg-2">
      {children}
    </div>
  )
}
