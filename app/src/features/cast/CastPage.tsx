import type * as React from "react"
import { useState } from "react"
import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { CaretDown, Eye, EyeSlash, Info, ListBullets, SquaresFour, User, UserFocus, Users } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { EmptyState } from "@/components/ui/empty-state"
import { Page } from "@/components/ui/page"
import { Segmented } from "@/components/ui/segmented"
import { TopBar } from "@/components/ui/topbar"
import { AnalysisStatus } from "@/features/project/AnalysisStatus"
import { ProjectLoadError } from "@/features/project/ProjectLoadError"
import { ProjectNav } from "@/features/project/ProjectNav"
import { firstFailure, useCast, useMediaList, useProject } from "@/lib/queries"
import { cn } from "@/lib/utils"
import { CharacterCard, CharacterRow } from "./CharacterCard"
import { plural, splitHidden, splitTiers, unplacedNote, useProjectAnalysis } from "./cast"

type ViewMode = "grid" | "list"

/** Everyone the analysis found, most screen time first. */
export function CastPage() {
  const { pid } = useParams({ from: "/p/$pid/cast" })
  const projectQuery = useProject(pid)
  const castQuery = useCast(pid)
  const mediaQuery = useMediaList(pid)
  const { data: project } = projectQuery
  const { data: cast, isPending } = castQuery
  const { data: media } = mediaQuery
  const running = useProjectAnalysis(pid)
  const [showHidden, setShowHidden] = useState(false)
  const [view, setView] = useState<ViewMode>("grid")
  const navigate = useNavigate()
  const { shown, hidden } = splitHidden(cast)
  const { main, supporting } = splitTiers(shown)
  const anime = project?.content === "anime"
  const unplaced = anime ? null : unplacedNote(cast?.unplaced ?? 0)
  const failure = firstFailure(projectQuery, castQuery, mediaQuery)

  if (failure) {
    return (
      <Page topbar={<TopBar title={project?.title} nav={<ProjectNav pid={pid} />} />}>
        <ProjectLoadError error={failure.error} onRetry={failure.retry} />
      </Page>
    )
  }

  return (
    <Page
      topbar={<TopBar title={project?.title} nav={<ProjectNav pid={pid} />} />}
      menu={
        <>
          {cast && cast.unsure > 0 && (
            <ContextMenuItem onSelect={() => void navigate({ to: "/p/$pid/cast/review", params: { pid } })}>
              <UserFocus /> Review {plural(cast.unsure, "face")}
            </ContextMenuItem>
          )}
          {cast && cast.unassigned > 0 && (
            <ContextMenuItem onSelect={() => void navigate({ to: "/p/$pid/cast/$cid", params: { pid, cid: "unassigned" } })}>
              <User /> Everyone else
            </ContextMenuItem>
          )}
          {hidden.length > 0 && (
            <ContextMenuItem onSelect={() => setShowHidden((v) => !v)}>
              {showHidden ? <EyeSlash /> : <Eye />} {showHidden ? "Fold the hidden characters" : "Show the hidden characters"}
            </ContextMenuItem>
          )}
        </>
      }
    >
      <header className="flex flex-wrap items-end justify-between gap-6 px-10 pt-10 pb-8">
        <div>
          <h1 className="display text-5xl text-fg-1">Cast</h1>
          {cast && !anime && (
            <p className="mt-2 text-md text-fg-2">
              {plural(shown.length, "character")}
              {cast.mediaTotal > 0 &&
                ` · faces from ${cast.mediaDone} of ${plural(cast.mediaTotal, project?.kind === "movie" ? "file" : "episode")}`}
            </p>
          )}
        </div>
        <div className="flex items-center gap-3">
          {shown.length > 0 && (
            <Segmented
              value={view}
              onChange={setView}
              aria-label="View layout"
              options={[
                { value: "grid", label: <SquaresFour weight="bold" /> },
                { value: "list", label: <ListBullets weight="bold" /> },
              ]}
            />
          )}
          {cast && cast.unsure > 0 && (
            <Button variant="primary" size="lg" asChild>
              <Link to="/p/$pid/cast/review" params={{ pid }}>
                <UserFocus weight="bold" /> Review {plural(cast.unsure, "face")}
              </Link>
            </Button>
          )}
        </div>
      </header>

      <div className="px-10 pb-16">
        <AnalysisStatus pid={pid} className="mb-8" />
        {unplaced && (
          <p className="mb-8 flex max-w-2xl items-start gap-2.5 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-3 text-sm text-fg-2 [&_svg]:mt-0.5 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-fg-1">
            <Info /> {unplaced}
          </p>
        )}
        {anime ? (
          <EmptyState
            icon={<UserFocus />}
            title="Anime characters are coming"
            description="Characters are found in live action for now. Anime gets its own face and head detectors in an upcoming update; shots already work."
          />
        ) : isPending ? (
          <CardGrid>
            {Array.from({ length: 8 }, (_, i) => (
              <div key={i} className="skeleton aspect-[3/4] rounded-2xl" />
            ))}
          </CardGrid>
        ) : shown.length === 0 ? (
          <EmptyState
            icon={<Users />}
            title={running.length > 0 ? "Looking for characters" : "No characters yet"}
            description={
              (media?.length ?? 0) === 0
                ? "Add episodes to this project. Characters appear here as they are analysed."
                : "Characters appear here as soon as the first episode's faces are analysed."
            }
          />
        ) : (
          <>
            {view === "grid" ? (
              supporting.length > 0 ? (
                <div className="space-y-10">
                  <section>
                    <div className="mb-4 flex items-center justify-between">
                      <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">Main Cast</h2>
                      <span className="text-xs text-fg-3">{plural(main.length, "character")}</span>
                    </div>
                    <CardGrid className="grid-cols-[repeat(auto-fill,minmax(200px,1fr))]">
                      {main.map((c) => (
                        <CharacterCard key={c.id} pid={pid} character={c} />
                      ))}
                    </CardGrid>
                  </section>

                  <section>
                    <div className="mb-4 flex items-center justify-between">
                      <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">Supporting & Recurring</h2>
                      <span className="text-xs text-fg-3">{plural(supporting.length, "character")}</span>
                    </div>
                    <CardGrid className="grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-x-3.5 gap-y-4">
                      {supporting.map((c) => (
                        <CharacterCard key={c.id} pid={pid} character={c} />
                      ))}
                      {cast && cast.unassigned > 0 && (
                        <ContextArea
                          menu={
                            <ContextMenuItem onSelect={() => void navigate({ to: "/p/$pid/cast/$cid", params: { pid, cid: "unassigned" } })}>
                              <User /> Open everyone else
                            </ContextMenuItem>
                          }
                        >
                          <Link
                            to="/p/$pid/cast/$cid"
                            params={{ pid, cid: "unassigned" }}
                            className="group flex aspect-[3/4] flex-col items-center justify-center gap-2 rounded-2xl border border-dashed border-white/15 text-center transition-colors hover:border-white/30 hover:bg-white/[0.03]"
                          >
                            <span className="flex size-10 items-center justify-center rounded-full bg-white/[0.06] text-fg-2 transition-colors group-hover:bg-white/[0.1] group-hover:text-fg-1 [&_svg]:size-5">
                              <User />
                            </span>
                            <span className="display text-lg text-fg-1">Everyone else</span>
                            <span className="text-2xs text-fg-3">{plural(cast.unassigned, "face")}</span>
                          </Link>
                        </ContextArea>
                      )}
                    </CardGrid>
                  </section>
                </div>
              ) : (
                <CardGrid>
                  {shown.map((c) => (
                    <CharacterCard key={c.id} pid={pid} character={c} />
                  ))}
                  {cast && cast.unassigned > 0 && (
                    <ContextArea
                      menu={
                        <ContextMenuItem onSelect={() => void navigate({ to: "/p/$pid/cast/$cid", params: { pid, cid: "unassigned" } })}>
                          <User /> Open everyone else
                        </ContextMenuItem>
                      }
                    >
                      <Link
                        to="/p/$pid/cast/$cid"
                        params={{ pid, cid: "unassigned" }}
                        className="group flex aspect-[3/4] flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-white/15 text-center transition-colors hover:border-white/30 hover:bg-white/[0.03]"
                      >
                        <span className="flex size-14 items-center justify-center rounded-full bg-white/[0.06] text-fg-2 transition-colors group-hover:bg-white/[0.1] group-hover:text-fg-1 [&_svg]:size-6">
                          <User />
                        </span>
                        <span className="display text-2xl text-fg-1">Everyone else</span>
                        <span className="text-xs text-fg-3">{plural(cast.unassigned, "face")} nobody claimed</span>
                      </Link>
                    </ContextArea>
                  )}
                </CardGrid>
              )
            ) : (
              supporting.length > 0 ? (
                <div className="space-y-8">
                  <section>
                    <div className="mb-3 flex items-center justify-between">
                      <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">Main Cast</h2>
                      <span className="text-xs text-fg-3">{plural(main.length, "character")}</span>
                    </div>
                    <div className="divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2">
                      {main.map((c) => (
                        <CharacterRow key={c.id} pid={pid} character={c} />
                      ))}
                    </div>
                  </section>

                  <section>
                    <div className="mb-3 flex items-center justify-between">
                      <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">Supporting & Recurring</h2>
                      <span className="text-xs text-fg-3">{plural(supporting.length, "character")}</span>
                    </div>
                    <div className="divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2">
                      {supporting.map((c) => (
                        <CharacterRow key={c.id} pid={pid} character={c} />
                      ))}
                      {cast && cast.unassigned > 0 && (
                        <Link
                          to="/p/$pid/cast/$cid"
                          params={{ pid, cid: "unassigned" }}
                          className="flex items-center justify-between gap-4 rounded-xl px-4 py-2.5 transition-colors hover:bg-white/[0.04]"
                        >
                          <div className="flex items-center gap-3.5">
                            <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border border-dashed border-white/20 text-fg-3">
                              <User />
                            </div>
                            <div>
                              <div className="text-md font-medium text-fg-1">Everyone else</div>
                              <div className="text-xs text-fg-3">{plural(cast.unassigned, "face")} nobody claimed</div>
                            </div>
                          </div>
                        </Link>
                      )}
                    </div>
                  </section>
                </div>
              ) : (
                <div className="divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2">
                  {shown.map((c) => (
                    <CharacterRow key={c.id} pid={pid} character={c} />
                  ))}
                  {cast && cast.unassigned > 0 && (
                    <Link
                      to="/p/$pid/cast/$cid"
                      params={{ pid, cid: "unassigned" }}
                      className="flex items-center justify-between gap-4 rounded-xl px-4 py-2.5 transition-colors hover:bg-white/[0.04]"
                    >
                      <div className="flex items-center gap-3.5">
                        <div className="flex size-10 shrink-0 items-center justify-center rounded-lg border border-dashed border-white/20 text-fg-3">
                          <User />
                        </div>
                        <div>
                          <div className="text-md font-medium text-fg-1">Everyone else</div>
                          <div className="text-xs text-fg-3">{plural(cast.unassigned, "face")} nobody claimed</div>
                        </div>
                      </div>
                    </Link>
                  )}
                </div>
              )
            )}
            {hidden.length > 0 && (
              <div className="mt-12">
                <button
                  onClick={() => setShowHidden((v) => !v)}
                  className="flex items-center gap-2 text-md font-semibold text-fg-2 hover:text-fg-1"
                  aria-expanded={showHidden}
                >
                  <CaretDown weight="bold" className={cn("size-3.5 transition-transform", !showHidden && "-rotate-90")} />
                  Hidden <span className="font-medium text-fg-3">{hidden.length}</span>
                </button>
                {showHidden && (
                  view === "grid" ? (
                    <CardGrid className="mt-5 opacity-60 transition-opacity hover:opacity-100">
                      {hidden.map((c) => (
                        <CharacterCard key={c.id} pid={pid} character={c} />
                      ))}
                    </CardGrid>
                  ) : (
                    <div className="mt-5 divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2 opacity-60 transition-opacity hover:opacity-100">
                      {hidden.map((c) => (
                        <CharacterRow key={c.id} pid={pid} character={c} />
                      ))}
                    </div>
                  )
                )}
              </div>
            )}
          </>
        )}
      </div>
    </Page>
  )
}

function CardGrid({ className, children }: { className?: string; children: React.ReactNode }) {
  return <div className={cn("grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-x-5 gap-y-6", className)}>{children}</div>
}
