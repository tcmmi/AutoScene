import { useCallback, useEffect, useState } from "react"
import { useParams, useRouter } from "@tanstack/react-router"
import type { PageSpec, PluginInfo } from "@/bindings"
import { AppWindow, ArrowCounterClockwise, DotsThree, FileText, SlidersHorizontal, X } from "@/components/icons"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { EmptyState } from "@/components/ui/empty-state"
import { type MenuKit, contextKit, dropdownKit } from "@/components/ui/menu-actions"
import { TopBar } from "@/components/ui/topbar"
import { NotFound } from "@/features/not-found"
import { ProjectNav } from "@/features/project/ProjectNav"
import { useAppActions } from "@/lib/actions"
import { errorMessage } from "@/lib/ipc"
import { reveal } from "@/lib/os"
import { goBackTo, placesBefore, whileOpen } from "@/lib/places"
import { usePlugins, useProject } from "@/lib/queries"
import { PluginFrame } from "./PluginFrame"
import { PluginIcon } from "./PluginIcon"
import { isPageId, isPluginId, pageLabel } from "./page-items"

type PageParams = { plugin?: string; page?: string; _splat?: string; pid?: string }

/**
 * A plugin's page, full screen under AutoScene's bar: `/x/<plugin>/<page>/…`
 * for a page of its own, `/p/<project>/x/<plugin>/<page>/…` for a project's.
 * The bar is AutoScene's (its title says "Plugin" in a badge the plugin
 * can't word); everything in the frame under it is the plugin's. A bad
 * link, a page that isn't there, or a project page outside a project isn't
 * found; a page of a plugin that can't run now says why, and so does one
 * whose plugins couldn't be read.
 */
export function PluginPageView() {
  const { plugin, page, _splat, pid } = useParams({ strict: false }) as PageParams
  const { data: plugins, isPending, error } = usePlugins()
  if (!isPluginId(plugin) || !isPageId(page)) return <NotFound />
  if (isPending) return null
  if (error) return <Unread error={error} />
  const p = plugins?.find((x) => x.id === plugin)
  const pg = p?.pages.find((x) => x.id === page)
  if (!p || !pg || (pg.scope === "project") !== (pid != null)) return <NotFound />
  // Another page is a new screen: where Close goes back to, and its reloads, start again.
  const key = `${p.id}/${pg.id}/${pid ?? ""}`
  return pid != null ? (
    <ProjectPluginPage key={key} p={p} pg={pg} pid={pid} path={_splat ?? ""} />
  ) : (
    <PluginPage key={key} p={p} pg={pg} path={_splat ?? ""} />
  )
}

/** A project's page, with the project's bar and the way back to it. */
function ProjectPluginPage({ p, pg, pid, path }: { p: PluginInfo; pg: PageSpec; pid: string; path: string }) {
  const { data: project } = useProject(pid)
  return <PluginPage p={p} pg={pg} pid={pid} path={path} back={{ label: project?.title ?? "the project", to: `/p/${pid}` }} />
}

function PluginPage({
  p,
  pg,
  pid,
  path,
  back,
}: {
  p: PluginInfo
  pg: PageSpec
  pid?: string
  path: string
  back?: { label: string; to: string }
}) {
  const [reloads, setReloads] = useState(0)
  const close = useLeave(pid)
  const reload = () => setReloads((n) => n + 1)
  const items = (kit: MenuKit) => <PageItems kit={kit} p={p} onReload={reload} onClose={close} />
  return (
    <div className="flex h-full flex-col">
      <TopBar back={back} title={<PageTitle p={p} pg={pg} />} nav={pid != null ? <ProjectNav pid={pid} /> : undefined}>
        <DropdownMenu>
          <DropdownMenuTrigger
            aria-label={`${pg.title} options`}
            className="flex size-8 items-center justify-center rounded-full text-fg-3 transition-colors hover:bg-white/[0.08] hover:text-fg-1 data-[state=open]:bg-white/[0.08]"
          >
            <DotsThree className="size-5" weight="bold" />
          </DropdownMenuTrigger>
          <DropdownMenuContent>{items(dropdownKit)}</DropdownMenuContent>
        </DropdownMenu>
      </TopBar>
      {p.active ? (
        <PluginFrame
          key={`${p.id}/${pg.id}/${pid ?? ""}/${p.version}/${reloads}`}
          pluginId={p.id}
          pageId={pg.id}
          projectId={pid ?? null}
          path={path}
          menu={items(contextKit)}
          onReload={reload}
          onClose={close}
        />
      ) : (
        <Unavailable p={p} />
      )}
    </div>
  )
}

/** The page's name in the bar: its plugin's icon, its title, and "Plugin", which no plugin can word. */
function PageTitle({ p, pg }: { p: PluginInfo; pg: PageSpec }) {
  return (
    <span data-tauri-drag-region className="flex min-w-0 items-center gap-2.5">
      <PluginIcon src={p.icon} className="size-5" />
      <span data-tauri-drag-region className="truncate">
        {pg.title}
      </span>
      <Badge tone="outline">Plugin</Badge>
    </span>
  )
}

/** What can be done with a plugin's page: its ⋯ menu and a right-click on it list the same. */
function PageItems({ kit, p, onReload, onClose }: { kit: MenuKit; p: PluginInfo; onReload: () => void; onClose: () => void }) {
  const { goPlugin } = useAppActions()
  const { Item, Separator } = kit
  return (
    <>
      <Item onSelect={onReload}>
        <ArrowCounterClockwise /> Reload the page
      </Item>
      <Item onSelect={() => goPlugin(p.id)}>
        <SlidersHorizontal /> Its settings
      </Item>
      <Item onSelect={() => reveal(p.logPath)}>
        <FileText /> Show its log
      </Item>
      {p.serviceLogPath && (
        <Item onSelect={() => reveal(p.serviceLogPath!)}>
          <FileText /> Show its service log
        </Item>
      )}
      <Separator />
      <Item onSelect={onClose}>
        <X /> Close
      </Item>
    </>
  )
}

/** A page whose plugins couldn't be read: why, and where to look. */
function Unread({ error }: { error: unknown }) {
  const { goPlugins } = useAppActions()
  return (
    <EmptyState
      icon={<AppWindow />}
      tone="danger"
      title="Couldn't read the plugins"
      description={errorMessage(error)}
      actions={<Button onClick={goPlugins}>Go to Plugins</Button>}
      className="h-full"
    />
  )
}

/** A page of a plugin that can't run now: why, and where to change that. */
function Unavailable({ p }: { p: PluginInfo }) {
  const { goPlugins } = useAppActions()
  return (
    <EmptyState
      icon={<AppWindow />}
      title="This page isn't available"
      description={p.broken ?? p.blockedReason ?? `${p.name} is turned off.`}
      actions={<Button onClick={goPlugins}>Go to Plugins</Button>}
      className="flex-1"
    />
  )
}

/**
 * Leaves the page for where the person was before it opened (its own moves
 * inside it and Back and Forward there don't count), else for the library
 * or its project: in place of the page's entry, never through the window's
 * history, where the page's frame adds entries of its own. Where it goes
 * back to, a page too, is left in turn for where the person was before
 * that. The window menu's Back leaves it so too.
 */
function useLeave(pid?: string): () => void {
  const router = useRouter()
  const [before] = useState(() => placesBefore(router.history))
  const leave = useCallback(() => goBackTo(router.history, before, pid != null ? `/p/${pid}` : "/"), [router, before, pid])
  useEffect(() => whileOpen(leave), [leave])
  return leave
}

/** A plugin page's name in the window's title: `Plugin “<name>” · <title>`. */
export function PluginPageCrumb({ plugin, page }: { plugin: string; page: string }) {
  const { data } = usePlugins()
  const p = data?.find((x) => x.id === plugin)
  const pg = p?.pages.find((x) => x.id === page)
  return <>{p && pg ? pageLabel({ pluginName: p.name, title: pg.title }) : "Plugin"}</>
}
