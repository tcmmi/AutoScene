import type * as React from "react"
import { useMemo, useRef, useState } from "react"
import { Link, useNavigate, useParams } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import {
  CaretDown,
  Eye,
  EyeSlash,
  Info,
  ListBullets,
  MagnifyingGlass,
  Scan,
  SelectionAll,
  SquaresFour,
  User,
  UserFocus,
  Users,
  X,
} from "@/components/icons"
import type { Character } from "@/bindings"
import { Button } from "@/components/ui/button"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { EmptyState } from "@/components/ui/empty-state"
import { Page } from "@/components/ui/page"
import { Segmented } from "@/components/ui/segmented"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleChip } from "@/components/ui/toggle-chip"
import { TopBar } from "@/components/ui/topbar"
import { AnalysisStatus } from "@/features/project/AnalysisStatus"
import { ProjectLoadError } from "@/features/project/ProjectLoadError"
import { ProjectNav } from "@/features/project/ProjectNav"
import { fmt } from "@/lib/i18n"
import { modKey } from "@/lib/platform"
import { firstFailure, useCast, useMediaList, useProject, useSettings } from "@/lib/queries"
import { CAST_SHORTCUTS, useShortcuts } from "@/lib/shortcuts"
import { useRescan } from "@/state/rescan"
import { useUi } from "@/state/ui"
import { cn } from "@/lib/utils"
import { CharacterCard, CharacterRow } from "./CharacterCard"
import { AniListDialog } from "./AniListDialog"
import { AniListNames } from "./AniListNames"
import { CastFilters } from "./CastFilters"
import { ChoosePictureDialog } from "./ChoosePictureDialog"
import { EveryoneElseCard } from "./EveryoneElseCard"
import { RenameCharacterDialog } from "./RenameCharacterDialog"
import { RescanBar } from "./RescanBar"
import { RescanWaiting } from "./RescanWaiting"
import { startRescan } from "./rescan"
import {
  ALL_CAST,
  type CastFind,
  cardClick,
  findCharacters,
  isNarrowed,
  namesSource,
  setHiddenAll,
  splitHidden,
  splitTiers,
  unplacedNote,
  useProjectAnalysis,
} from "./cast"

/** Fewer characters than a row of cards: nothing to look for, so no search. */
const FIND_FROM = 6
/** How each project's cast was narrowed last: coming back from a character finds it so again. */
const lastFind = new Map<string, CastFind>()

/** Everyone the analysis found, most screen time first. */
export function CastPage() {
  const { t, i18n } = useTranslation(["cast", "common"])
  const { pid } = useParams({ from: "/p/$pid/cast" })
  const projectQuery = useProject(pid)
  const castQuery = useCast(pid)
  const mediaQuery = useMediaList(pid)
  const { data: project } = projectQuery
  const { data: cast, isPending } = castQuery
  const { data: media } = mediaQuery
  const { data: settings } = useSettings()
  const running = useProjectAnalysis(pid)
  const [showHidden, setShowHidden] = useState(false)
  const [anilistOpen, setAnilistOpen] = useState(false)
  const [renaming, setRenaming] = useState<Character | null>(null)
  const [picturing, setPicturing] = useState<Character | null>(null)
  const [find, setFindState] = useState(() => lastFind.get(pid) ?? ALL_CAST)
  const setFind = (f: CastFind) => {
    lastFind.set(pid, f)
    setFindState(f)
  }
  const navigate = useNavigate()
  const { shown: everyoneShown, hidden: everyoneHidden } = useMemo(() => splitHidden(cast), [cast])
  const narrowed = isNarrowed(find)
  // Labels come from the catalog, so a selection click doesn't ask it again for every character.
  // oxlint-disable-next-line react-hooks/exhaustive-deps -- the language is why a label, so a search and the name order, differs.
  const shown = useMemo(() => findCharacters(everyoneShown, find), [everyoneShown, find, i18n.language])
  // oxlint-disable-next-line react-hooks/exhaustive-deps -- the same.
  const hidden = useMemo(() => findCharacters(everyoneHidden, find), [everyoneHidden, find, i18n.language])
  const { main: mainShown, supporting: supportingShown } = splitTiers(shown)
  // The order the cards are on screen, main cast first: a Shift-click's range
  // is what lies between on screen, not in the backend's order.
  const ordered = [...mainShown, ...supportingShown]
  const view = useUi((s) => s.castView)
  const setView = useUi((s) => s.setCastView)
  // The search stays while it narrows or orders the cast, however small it got.
  const finding = (cast?.characters.length ?? 0) >= FIND_FROM || narrowed || find.order !== ALL_CAST.order

  // The cards in the order shown, hidden ones after while unfolded: a
  // Shift-click selects every card between.
  const cards = showHidden ? [...ordered, ...hidden] : ordered
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const anchor = useRef<string | null>(null)
  // Only cards on screen count: a character merged away, narrowed out or
  // folded with the hidden ones isn't acted on.
  const chosen = cards.filter((c) => picked.has(c.id))
  const selected = new Set(chosen.map((c) => c.id))
  const toHide = chosen.filter((c) => !c.hidden).map((c) => c.id)
  const toShow = chosen.filter((c) => c.hidden).map((c) => c.id)
  const selectAll = () => setPicked(new Set(cards.map((c) => c.id)))
  const clearSelection = () => setPicked(new Set())
  const pick = (index: number, e: React.MouseEvent) => {
    const ids = cards.map((c) => c.id)
    const next = cardClick(ids, selected, anchor.current, index, { shift: e.shiftKey, toggle: e.ctrlKey || e.metaKey })
    if (!next) return false
    anchor.current = next.anchor
    setPicked(next.selected)
    return true
  }
  const selectCard = (index: number) => {
    const id = cards[index]?.id
    if (!id) return
    anchor.current = id
    setPicked(new Set([...selected, id]))
  }
  const [withEveryone, setWithEveryone] = useState(false)
  const run = useRescan((s) => s.runs[pid])
  const rescan = () => {
    void startRescan(pid, { tracks: [], characters: chosen.map((c) => c.id), everyoneElse: withEveryone })
    setWithEveryone(false)
    clearSelection()
  }
  const setHidden = async (ids: string[], hidden: boolean) => {
    clearSelection()
    const done = await setHiddenAll(pid, ids, hidden)
    // They leave the grid at once: the toast says where they went and takes them back.
    if (hidden && done > 0) {
      toast(t("selection.hid", { count: done }), {
        action: { label: t("common:action.undo"), onClick: () => void setHiddenAll(pid, ids.slice(0, done), false) },
      })
    }
  }
  useShortcuts(CAST_SHORTCUTS, {
    selectAll: () => {
      if (cards.length === 0) return false
      selectAll()
    },
    clearSelection: () => {
      if (selected.size === 0) return false
      clearSelection()
    },
  })
  // What the cards selected can be told (the bar at the bottom says the same).
  const selectionMenu = selected.size > 0 && (
    <>
      {toHide.length > 0 && (
        <ContextMenuItem onSelect={() => void setHidden(toHide, true)}>
          <EyeSlash /> {t("selection.hide", { count: toHide.length })}
        </ContextMenuItem>
      )}
      {toShow.length > 0 && (
        <ContextMenuItem onSelect={() => void setHidden(toShow, false)}>
          <Eye /> {t("selection.showInCast", { count: toShow.length })}
        </ContextMenuItem>
      )}
      <ContextMenuItem onSelect={rescan}>
        <Scan /> {t("faces.rescan")}
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem onSelect={selectAll} shortcut={[modKey, "A"]}>
        <SelectionAll /> {t("selection.selectAll")}
      </ContextMenuItem>
      {/* i18n-ignore: the legend printed on a key; its own name for it belongs to the shortcuts namespace */}
      <ContextMenuItem onSelect={clearSelection} shortcut={["Esc"]}>
        <X /> {t("selection.clear")}
      </ContextMenuItem>
    </>
  )
  const cardProps = (c: Character, index: number) => ({
    pid,
    character: c,
    onRename: () => setRenaming(c),
    onChoosePicture: () => setPicturing(c),
    selected: selected.has(c.id),
    selecting: selected.size > 0,
    scanning: run?.characters.includes(c.id) ? (run.total > 0 ? run.done / run.total : 0) : undefined,
    onPick: (e: React.MouseEvent) => pick(index, e),
    onSelect: () => selectCard(index),
    selectionMenu,
  })
  const unplaced = unplacedNote(cast?.unplaced ?? 0)
  // Names from AniList (anime) or TVmaze (animation) while the opt-in in
  // Settings is on and someone shown is still unnamed.
  const source = namesSource(project?.content, project?.kind)
  const anilist = !!settings?.anilist && source !== null && !!cast?.characters.some((c) => !c.hidden && !c.name)
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
              <UserFocus /> {t("faces.review", { count: cast.unsure })}
            </ContextMenuItem>
          )}
          {cast && cast.unassigned > 0 && (
            <ContextMenuItem onSelect={() => void navigate({ to: "/p/$pid/cast/$cid", params: { pid, cid: "unassigned" } })}>
              <User /> {t("everyoneElse.title")}
            </ContextMenuItem>
          )}
          {anilist && (
            <ContextMenuItem onSelect={() => setAnilistOpen(true)}>
              <MagnifyingGlass /> {t("names.fromSource", { source })}
            </ContextMenuItem>
          )}
          {hidden.length > 0 && (
            <ContextMenuItem onSelect={() => setShowHidden((v) => !v)}>
              {showHidden ? <EyeSlash /> : <Eye />} {showHidden ? t("page.foldHidden") : t("page.showHidden")}
            </ContextMenuItem>
          )}
          {cards.length > 0 && (
            <ContextMenuItem onSelect={selectAll} shortcut={[modKey, "A"]}>
              <SelectionAll /> {t("page.selectEvery")}
            </ContextMenuItem>
          )}
        </>
      }
    >
      <header className="flex flex-wrap items-end justify-between gap-6 px-10 pt-10 pb-8">
        <div>
          <h1 className="display text-5xl text-fg-1">{t("title")}</h1>
          {cast && (
            <p className="mt-2 text-md text-fg-2">
              {narrowed
                ? t("page.narrowed", { shown: fmt.count(shown.length), count: everyoneShown.length })
                : t("count.characters", { count: shown.length })}
              {cast.mediaTotal > 0 &&
                ` · ${
                  project?.kind === "movie"
                    ? t("page.facesFromFiles", { done: cast.mediaDone, count: cast.mediaTotal })
                    : t("page.facesFromEpisodes", { done: cast.mediaDone, count: cast.mediaTotal })
                }`}
            </p>
          )}
        </div>
        <div className="flex items-center gap-2.5">
          {everyoneShown.length > 0 && (
            <Segmented
              value={view}
              onChange={setView}
              aria-label={t("page.viewLayout")}
              options={[
                { value: "grid", label: <SquaresFour weight="bold" /> },
                { value: "list", label: <ListBullets weight="bold" /> },
              ]}
            />
          )}
          {anilist && (
            <Button variant="secondary" size="lg" onClick={() => setAnilistOpen(true)}>
              <MagnifyingGlass weight="bold" /> {t("names.fromSource", { source })}
            </Button>
          )}
          {cast && cast.unsure > 0 && (
            <Button variant="primary" size="lg" asChild>
              <Link to="/p/$pid/cast/review" params={{ pid }}>
                <UserFocus weight="bold" /> {t("faces.review", { count: cast.unsure })}
              </Link>
            </Button>
          )}
        </div>
        {cast && finding && (
          <CastFilters className="basis-full" find={find} review={cast.characters.some((c) => c.unsure > 0)} onChange={setFind} />
        )}
      </header>

      <div className={cn("px-10", selected.size > 0 ? "pb-28" : "pb-16")}>
        <AnalysisStatus pid={pid} className="mb-8" />
        <AniListNames pid={pid} cast={cast} className="mb-8" />
        <RescanWaiting pid={pid} className="mb-8" />
        {unplaced && (
          <p className="mb-8 flex max-w-2xl items-start gap-2.5 rounded-lg border border-white/10 bg-white/[0.04] px-3.5 py-3 text-sm text-fg-2 [&_svg]:mt-0.5 [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-fg-1">
            <Info /> {unplaced}
          </p>
        )}
        {isPending ? (
          <CardGrid>
            {Array.from({ length: 8 }, (_, i) => (
              <Skeleton key={i} className="aspect-[3/4] rounded-2xl" />
            ))}
          </CardGrid>
        ) : everyoneShown.length === 0 ? (
          <EmptyState
            icon={<Users />}
            title={running.length > 0 ? t("page.looking") : t("page.none")}
            description={
              (media?.length ?? 0) === 0
                ? t("page.noEpisodes")
                : t("page.waitingForFaces")
            }
          />
        ) : (
          <>
            {shown.length === 0 && (
              <div className="flex flex-col items-start gap-4 py-4">
                <p className="text-md text-fg-3">
                  {find.query.trim()
                    ? t("page.noMatch", { query: find.query.trim() })
                    : find.show === "named"
                      ? t("page.noNamed")
                      : t("page.noReview")}
                </p>
                <Button onClick={() => setFind({ ...find, query: "", show: "all" })}>{t("page.showEveryone")}</Button>
              </div>
            )}
            {shown.length > 0 && (
              view === "grid" ? (
                supportingShown.length > 0 ? (
                  <div className="space-y-10">
                    <section>
                      <div className="mb-4 flex items-center justify-between">
                        <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">{t("page.mainCast")}</h2>
                        <span className="text-xs text-fg-3">{t("count.characters", { count: mainShown.length })}</span>
                      </div>
                      <CardGrid className="grid-cols-[repeat(auto-fill,minmax(200px,1fr))]">
                        {mainShown.map((c) => {
                          const i = ordered.indexOf(c)
                          return <CharacterCard key={c.id} {...cardProps(c, i)} />
                        })}
                      </CardGrid>
                    </section>

                    <section>
                      <div className="mb-4 flex items-center justify-between">
                        <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">{t("page.supportingCast")}</h2>
                        <span className="text-xs text-fg-3">{t("count.characters", { count: supportingShown.length })}</span>
                      </div>
                      <CardGrid className="grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-x-3.5 gap-y-4">
                        {supportingShown.map((c) => {
                          const i = ordered.indexOf(c)
                          return <CharacterCard key={c.id} {...cardProps(c, i)} />
                        })}
                        {!narrowed && cast && (
                          <EveryoneElseCard pid={pid} unassigned={cast.unassigned} view="grid" compact />
                        )}
                      </CardGrid>
                    </section>
                  </div>
                ) : (
                  <CardGrid>
                    {shown.map((c, i) => (
                      <CharacterCard key={c.id} {...cardProps(c, i)} />
                    ))}
                    {!narrowed && cast && (
                      <EveryoneElseCard pid={pid} unassigned={cast.unassigned} view="grid" />
                    )}
                  </CardGrid>
                )
              ) : (
                supportingShown.length > 0 ? (
                  <div className="space-y-8">
                    <section>
                      <div className="mb-3 flex items-center justify-between">
                        <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">{t("page.mainCast")}</h2>
                        <span className="text-xs text-fg-3">{t("count.characters", { count: mainShown.length })}</span>
                      </div>
                      <div className="divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2">
                        {mainShown.map((c) => {
                          const i = ordered.indexOf(c)
                          return <CharacterRow key={c.id} {...cardProps(c, i)} />
                        })}
                      </div>
                    </section>

                    <section>
                      <div className="mb-3 flex items-center justify-between">
                        <h2 className="text-sm font-semibold tracking-wide uppercase text-fg-2">{t("page.supportingCast")}</h2>
                        <span className="text-xs text-fg-3">{t("count.characters", { count: supportingShown.length })}</span>
                      </div>
                      <div className="divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2">
                        {supportingShown.map((c) => {
                          const i = ordered.indexOf(c)
                          return <CharacterRow key={c.id} {...cardProps(c, i)} />
                        })}
                        {!narrowed && cast && (
                          <EveryoneElseCard pid={pid} unassigned={cast.unassigned} view="list" />
                        )}
                      </div>
                    </section>
                  </div>
                ) : (
                  <div className="divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2">
                    {shown.map((c, i) => (
                      <CharacterRow key={c.id} {...cardProps(c, i)} />
                    ))}
                    {!narrowed && cast && (
                      <EveryoneElseCard pid={pid} unassigned={cast.unassigned} view="list" />
                    )}
                  </div>
                )
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
                  {t("page.hidden")} <span className="font-medium text-fg-3">{hidden.length}</span>
                </button>
                {showHidden && (
                  view === "grid" ? (
                    <CardGrid className="mt-5 opacity-60 transition-opacity hover:opacity-100">
                      {hidden.map((c, i) => (
                        <CharacterCard key={c.id} {...cardProps(c, shown.length + i)} />
                      ))}
                    </CardGrid>
                  ) : (
                    <div className="mt-5 divide-y divide-white/[0.06] rounded-2xl border border-white/10 bg-white/[0.02] p-2 opacity-60 transition-opacity hover:opacity-100">
                      {hidden.map((c, i) => (
                        <CharacterRow key={c.id} {...cardProps(c, shown.length + i)} />
                      ))}
                    </div>
                  )
                )}
              </div>
            )}
          </>
        )}
      </div>
      {selected.size > 0 && (
        <div className="glass-strong fixed bottom-6 left-1/2 z-40 flex w-max max-w-[calc(100vw-3rem)] -translate-x-1/2 animate-pop-in items-center gap-1 rounded-full py-1.5 pr-1.5 pl-5">
          <span className="pr-2 text-md font-semibold whitespace-nowrap text-fg-1">{t("count.characters", { count: selected.size })}</span>
          {toHide.length > 0 && (
            <Button size="sm" variant="ghost" onClick={() => void setHidden(toHide, true)}>
              <EyeSlash /> {t("selection.hide")}
            </Button>
          )}
          {toShow.length > 0 && (
            <Button size="sm" variant="ghost" onClick={() => void setHidden(toShow, false)}>
              <Eye /> {t("selection.showInCast")}
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={rescan}>
            <Scan /> {t("faces.rescan")}
          </Button>
          {(cast?.unassigned ?? 0) > 0 && (
            <ToggleChip pressed={withEveryone} onClick={() => setWithEveryone((v) => !v)} className="ml-1">
              {t("selection.withEveryoneElse")}
            </ToggleChip>
          )}
          <span className="mx-1 h-5 w-px bg-white/15" />
          <Button size="sm" variant="ghost" onClick={clearSelection} aria-label={t("selection.clear")}>
            {t("selection.clearShort")}
          </Button>
        </div>
      )}
      <RescanBar pid={pid} lifted={selected.size > 0} />
      {cast && (
        <AniListDialog pid={pid} characters={cast.characters} open={anilistOpen} onOpenChange={setAnilistOpen} />
      )}
      <RenameCharacterDialog
        pid={pid}
        character={renaming}
        open={renaming != null}
        onOpenChange={(open) => !open && setRenaming(null)}
      />
      <ChoosePictureDialog
        pid={pid}
        character={picturing}
        open={picturing != null}
        onOpenChange={(open) => !open && setPicturing(null)}
      />
    </Page>
  )
}

function CardGrid({ className, children }: { className?: string; children: React.ReactNode }) {
  return <div className={cn("grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-x-5 gap-y-6", className)}>{children}</div>
}
