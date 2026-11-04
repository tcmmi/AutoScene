import type * as React from "react"
import { useState } from "react"
import { Link, useNavigate, useRouterState } from "@tanstack/react-router"
import {
  AppWindow,
  ArrowCounterClockwise,
  DownloadSimple,
  FileText,
  FilmStrip,
  GearSix,
  Info,
  Keyboard,
  MagnifyingGlass,
  Plus,
  PuzzlePiece,
  Queue,
  SlidersHorizontal,
  SquaresFour,
  Television,
  Trash,
} from "@/components/icons"
import type { ProjectSummary } from "@/bindings"
import { AutoSceneMark } from "@/components/brand/AutoSceneMark"
import { Artwork } from "@/components/ui/artwork"
import { ContextArea, type MenuContent } from "@/components/ui/context-area"
import { ContextMenuItem, ContextMenuSeparator } from "@/components/ui/context-menu"
import { ProgressRing } from "@/components/ui/progress"
import { Tooltip } from "@/components/ui/tooltip"
import { contextKit } from "@/components/ui/menu-actions"
import { DeleteProjectDialog, ProjectDetailsDialog } from "@/features/library/ProjectDialogs"
import { NewProjectItems, ProjectItems } from "@/features/library/project-actions"
import { PluginIcon } from "@/features/plugins/PluginIcon"
import { type PageItem, pageLink, railPages, railTooltip } from "@/features/plugins/page-items"
import { useAppActions } from "@/lib/actions"
import { commands } from "@/lib/ipc"
import { reveal } from "@/lib/os"
import { usePlugins, useProjects } from "@/lib/queries"
import { useShortcutKeys } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"
import {
  describeActive,
  useActiveCount,
  useActiveProgress,
  useActivity,
  useProjectIndexProgress,
  useProjectIndexWaiting,
  useProjectIndexing,
  useRunningCount,
} from "@/state/activity"
import { useNewProject } from "@/state/new-project"
import { useUi } from "@/state/ui"

/** How many projects the rail shows (most recently touched first, plus the open one). */
const RECENT = 7

/**
 * The window's left edge, on the native material where there is one: the
 * mark, the library and up to four plugin pages, the projects as their
 * pictures, and at the bottom search, background work and settings.
 */
export function Rail() {
  const keys = useShortcutKeys()
  const actions = useAppActions()
  const path = useRouterState({ select: (s) => s.location.pathname })
  const { data: projects } = useProjects()
  const currentPid = path.match(/^\/p\/([^/]+)/)?.[1]
  const recent = (projects ?? []).slice(0, RECENT)
  const current = projects?.find((p) => p.id === currentPid)
  const listed = current && !recent.includes(current) ? [...recent, current] : recent

  const [renaming, setRenaming] = useState<ProjectSummary | null>(null)
  const [deleting, setDeleting] = useState<ProjectSummary | null>(null)

  return (
    <nav
      aria-label="Main"
      className="relative z-10 flex w-[var(--rail-w)] shrink-0 flex-col items-center border-r border-white/[0.06] bg-rail pt-[calc(var(--traffic-inset)+12px)] pb-3"
    >
      {/* The window is dragged by the rail's top, around the mark. */}
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-[var(--topbar-h)]" />
      <Tooltip label="Library" side="right">
        <ContextArea tail={false} menu={<LibraryItems />}>
          <Link to="/" aria-label="AutoScene library" className="relative mb-4 rounded-xl">
            <AutoSceneMark className="size-9 drop-shadow-[0_6px_16px_rgb(224_180_92/0.25)]" />
          </Link>
        </ContextArea>
      </Tooltip>

      <RailLink
        to="/"
        icon={<SquaresFour />}
        label="Library"
        active={path === "/"}
        shortcut={keys("global.library")}
        menu={<LibraryItems />}
      />
      <PluginRailLinks path={path} />

      <div className="my-3 h-px w-8 bg-white/10" />

      <div className="flex min-h-0 w-full flex-1 flex-col items-center gap-3 overflow-y-auto pt-1 pb-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {listed.map((p) => (
          <ProjectTile
            key={p.id}
            project={p}
            open={p.id === currentPid}
            onRename={() => setRenaming(p)}
            onDelete={() => setDeleting(p)}
          />
        ))}
        <Tooltip label="New project" shortcut={keys("global.newProject")} side="right">
          <ContextArea tail={false} menu={<NewProjectItems kit={contextKit} />}>
            <button
              onClick={() => useNewProject.getState().show()}
              aria-label="New project"
              className="flex size-11 shrink-0 items-center justify-center rounded-[14px] border border-dashed border-white/20 text-fg-3 transition-colors hover:border-white/40 hover:bg-white/[0.05] hover:text-fg-1 [&_svg]:size-[18px]"
            >
              <Plus />
            </button>
          </ContextArea>
        </Tooltip>
      </div>

      <div className="flex flex-col items-center gap-1 pt-2">
        <RailButton
          icon={<MagnifyingGlass />}
          label="Search"
          onClick={actions.openPalette}
          shortcut={keys("global.palette")}
          menu={
            <ContextMenuItem onSelect={actions.openPalette} shortcut={keys("global.palette")}>
              <MagnifyingGlass /> Command palette…
            </ContextMenuItem>
          }
        />
        <ActivityButton />
        <RailLink
          to="/plugins"
          icon={<PuzzlePiece />}
          label="Plugins"
          active={path === "/plugins"}
        />
        <RailLink
          to="/settings/general"
          icon={<GearSix />}
          label="Settings"
          active={path.startsWith("/settings")}
          shortcut={keys("global.settings")}
          menu={
            <>
              <ContextMenuItem onSelect={() => actions.goSettings("general")}>
                <GearSix /> General
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => actions.goSettings("shortcuts")}>
                <Keyboard /> Shortcuts
              </ContextMenuItem>
              <ContextMenuItem onSelect={() => actions.goSettings("updates")}>
                <ArrowCounterClockwise /> Updates
              </ContextMenuItem>
              <ContextMenuItem onSelect={actions.checkUpdates}>
                <DownloadSimple /> Check for updates
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem onSelect={() => actions.goSettings("about")}>
                <Info /> About AutoScene
              </ContextMenuItem>
            </>
          }
        />
      </div>

      <ProjectDetailsDialog project={renaming} onOpenChange={(o) => !o && setRenaming(null)} />
      <DeleteProjectDialog project={deleting} onOpenChange={(o) => !o && setDeleting(null)} />
    </nav>
  )
}

/** The library, and a new project: what the mark and the Library tile offer. */
function LibraryItems() {
  const keys = useShortcutKeys()
  const actions = useAppActions()
  return (
    <>
      <ContextMenuItem onSelect={actions.goLibrary} shortcut={keys("global.library")}>
        <SquaresFour /> Library
      </ContextMenuItem>
      <ContextMenuItem onSelect={actions.newProject} shortcut={keys("global.newProject")}>
        <Plus /> New project…
      </ContextMenuItem>
    </>
  )
}

/**
 * The plugin pages that asked for a tile, under Library: each with its
 * plugin's icon and AutoScene's plugin mark, and a tooltip that starts with
 * "Plugin", whatever the page is called.
 */
function PluginRailLinks({ path }: { path: string }) {
  const { data: plugins } = usePlugins()
  return (
    <>
      {railPages(plugins).map((pg) => {
        const to = pageLink(pg)
        return (
          <RailLink
            key={pg.key}
            to={to}
            icon={<PageTileIcon src={pg.icon} />}
            label={pg.title}
            tooltip={railTooltip(pg)}
            name={railTooltip(pg)}
            active={path === to || path.startsWith(`${to}/`)}
            menu={<PageTileItems page={pg} to={to} />}
          />
        )
      })}
    </>
  )
}

/** A plugin page's icon in the rail, with AutoScene's mark for a plugin in its corner. */
function PageTileIcon({ src }: { src: string | null }) {
  return (
    <span className="relative flex size-5 items-center justify-center [&_svg]:size-5">
      <PluginIcon src={src} className="size-5" />
      <span
        aria-hidden
        className="absolute -right-1.5 -bottom-1.5 flex size-3 items-center justify-center rounded-full bg-rail text-fg-2 [&_svg]:size-2.5"
      >
        <PuzzlePiece weight="fill" />
      </span>
    </span>
  )
}

/** What a plugin page's tile offers. */
function PageTileItems({ page, to }: { page: PageItem; to: string }) {
  const actions = useAppActions()
  const navigate = useNavigate()
  const { data: plugins } = usePlugins()
  const log = plugins?.find((p) => p.id === page.pluginId)?.logPath
  return (
    <>
      <ContextMenuItem onSelect={() => void navigate({ to })}>
        <AppWindow /> Open
      </ContextMenuItem>
      <ContextMenuItem onSelect={() => actions.goSettings(`plugin.${page.pluginId}`)}>
        <SlidersHorizontal /> Its settings
      </ContextMenuItem>
      {log && (
        <ContextMenuItem onSelect={() => reveal(log)}>
          <FileText /> Show its log
        </ContextMenuItem>
      )}
    </>
  )
}

const itemClass = "group flex w-[64px] flex-col items-center gap-1 rounded-xl py-1.5"
const pillClass =
  "flex h-8 w-11 items-center justify-center rounded-full transition-[background-color,color] duration-[var(--dur-fast)] [&>svg]:size-5"

function RailLink({
  to,
  icon,
  label,
  tooltip,
  name,
  active,
  shortcut,
  menu,
}: {
  to: string
  icon: React.ReactElement<{ weight?: string }>
  label: string
  /** What its tooltip says, when not its label. */
  tooltip?: string
  /** What a screen reader calls it, when not its label: a plugin's page starts with "Plugin", whatever its title. */
  name?: string
  active: boolean
  shortcut?: string[]
  /** What its right-click offers (only that: a rail tile has no app entries under it). */
  menu?: MenuContent
}) {
  return (
    <Tooltip label={tooltip ?? label} shortcut={shortcut} side="right">
      <ContextArea tail={false} menu={menu ?? null} disabled={!menu}>
        <Link to={to} aria-label={name} aria-current={active ? "page" : undefined} className={itemClass}>
          <span className={cn(pillClass, active ? "bg-white/[0.14] text-fg-1" : "text-fg-2 group-hover:bg-white/[0.07] group-hover:text-fg-1")}>
            {active ? <IconWeight icon={icon} /> : icon}
          </span>
          <span
            className={cn(
              "max-w-full truncate px-1 text-[10.5px] leading-3 font-medium",
              active ? "text-fg-1" : "text-fg-3 group-hover:text-fg-2",
            )}
          >
            {label}
          </span>
        </Link>
      </ContextArea>
    </Tooltip>
  )
}

/** The active place shows its icon filled. */
function IconWeight({ icon }: { icon: React.ReactElement<{ weight?: string }> }) {
  const Comp = icon.type as React.ComponentType<{ weight?: string }>
  return <Comp {...icon.props} weight="fill" />
}

function RailButton({
  icon,
  label,
  onClick,
  shortcut,
  pressed,
  menu,
  children,
}: {
  icon: React.ReactNode
  label: string
  onClick: () => void
  shortcut?: string[]
  pressed?: boolean
  /** What its right-click offers (only that). */
  menu?: MenuContent
  children?: React.ReactNode
}) {
  return (
    <Tooltip label={label} shortcut={shortcut} side="right">
      <ContextArea tail={false} menu={menu ?? null} disabled={!menu}>
        <button onClick={onClick} aria-pressed={pressed} className={itemClass}>
          <span
            className={cn(
              pillClass,
              "relative",
              pressed ? "bg-white/[0.14] text-fg-1" : "text-fg-2 group-hover:bg-white/[0.07] group-hover:text-fg-1",
            )}
          >
            {icon}
            {children}
          </span>
          <span className={cn("text-[10.5px] leading-3 font-medium", pressed ? "text-fg-1" : "text-fg-3 group-hover:text-fg-2")}>
            {label}
          </span>
        </button>
      </ContextArea>
    </Tooltip>
  )
}

/** Background work: a ring fills as it runs; the count rides on the icon. */
function ActivityButton() {
  const keys = useShortcutKeys()
  const actions = useAppActions()
  const open = useUi((s) => s.activityOpen)
  const active = useActiveCount()
  const running = useRunningCount()
  const progress = useActiveProgress()
  return (
    <RailButton
      // Gold while something runs: work that only waits is counted, not lit.
      icon={<Queue className={cn(running > 0 && "text-accent")} />}
      label="Activity"
      onClick={actions.toggleActivity}
      shortcut={keys("global.activity")}
      pressed={open}
      menu={
        <>
          <ContextMenuItem onSelect={actions.toggleActivity} shortcut={keys("global.activity")}>
            <Queue /> {open ? "Hide activity" : "Show activity"}
          </ContextMenuItem>
          <ContextMenuItem
            onSelect={() => {
              void commands.jobsClearFinished()
              useActivity.getState().clearFinished()
            }}
          >
            <Trash /> Clear finished
          </ContextMenuItem>
        </>
      }
    >
      {active > 0 && (
        <>
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <ProgressRing
              aria-hidden
              value={progress}
              waiting={running === 0}
              grid={32}
              className="size-8"
              trackClassName="text-white/[0.08]"
            />
          </div>
          <span
            className={cn(
              "timecode absolute -top-1 -right-1 flex h-4 min-w-4 items-center justify-center rounded-full px-1 text-[10px] leading-none font-semibold",
              running > 0 ? "bg-accent text-on-accent shadow-[0_0_6px_var(--accent)]" : "bg-fg-3 text-canvas",
            )}
          >
            {active}
          </span>
        </>
      )}
      <span className="sr-only">{describeActive(running, active - running)}</span>
    </RailButton>
  )
}

function ProjectTile({
  project: p,
  open,
  onRename,
  onDelete,
}: {
  project: ProjectSummary
  open: boolean
  onRename: () => void
  onDelete: () => void
}) {
  const indexing = useProjectIndexing(p.id)
  const initials = p.title
    .split(/\s+/)
    .slice(0, 2)
    .map((w) => w[0])
    .join("")
    .toUpperCase()
  return (
    <ContextArea tail={false} menu={<ProjectItems kit={contextKit} project={p} onRename={onRename} onDelete={onDelete} />}>
        <div>
          <Tooltip
            side="right"
            label={
              <span className="flex flex-col">
                <span className="font-semibold">{p.title}</span>
                <span className="text-fg-3">
                  {p.kind === "movie" ? "Movie" : `${p.mediaCount} ${p.mediaCount === 1 ? "episode" : "episodes"}`}
                  {indexing && " · indexing"}
                </span>
              </span>
            }
          >
            <Link
              to="/p/$pid"
              params={{ pid: p.id }}
              aria-label={p.title}
              aria-current={open ? "true" : undefined}
              className="group relative flex shrink-0 items-center"
            >
              <span
                aria-hidden
                className={cn(
                  "absolute -left-[16px] w-1 rounded-r-full bg-accent transition-[height,opacity] duration-[var(--dur-med)] ease-out",
                  open ? "h-6 opacity-100" : "h-2 opacity-0 group-hover:opacity-60",
                )}
              />
              <Artwork
                path={p.posterSmall ?? p.poster}
                className={cn(
                  "size-11 rounded-[14px] transition-[transform,box-shadow] duration-[var(--dur-med)] ease-out group-hover:scale-105",
                  open
                    ? "shadow-[0_0_0_2px_var(--bg-canvas),0_0_0_4px_var(--accent)]"
                    : "shadow-[inset_0_0_0_1px_rgb(255_255_255/0.1)]",
                )}
                fallback={
                  <span className="flex size-full items-center justify-center artwork-fallback text-xs font-semibold text-fg-2">
                    {initials || (p.kind === "movie" ? <FilmStrip /> : <Television />)}
                  </span>
                }
              />
              {indexing && <IndexingBadge projectId={p.id} />}
            </Link>
          </Tooltip>
        </div>
    </ContextArea>
  )
}

/**
 * A project being indexed: a ring that fills, on a dark disc at the tile's
 * corner. The disc cuts into the open tile's gold ring instead of sitting
 * on it, so the two never run together.
 */
function IndexingBadge({ projectId }: { projectId: string }) {
  const progress = useProjectIndexProgress(projectId)
  const waiting = useProjectIndexWaiting(projectId)
  return (
    <span
      aria-hidden
      className="pointer-events-none absolute -right-1.5 -bottom-1.5 flex size-[18px] animate-pop-in items-center justify-center rounded-full bg-canvas"
    >
      <ProgressRing value={progress} waiting={waiting} className="size-3" />
    </span>
  )
}
