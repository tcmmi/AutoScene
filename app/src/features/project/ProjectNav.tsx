import { useNavigate, useRouterState } from "@tanstack/react-router"
import { CaretDown } from "@/components/icons"
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { SegmentedNav, SegmentedNavButton } from "@/components/ui/segmented"
import { PluginIcon } from "@/features/plugins/PluginIcon"
import { type PageItem, pageLabel, pageLink, pagesFor } from "@/features/plugins/page-items"
import { usePlugins, useProject } from "@/lib/queries"

/**
 * A project's sections, in the middle of its top bar, and More when plugins
 * add pages to projects: the bar never grows past five.
 */
export function ProjectNav({ pid }: { pid: string }) {
  const { data: project } = useProject(pid)
  const { data: plugins } = usePlugins()
  const path = useRouterState({ select: (s) => s.location.pathname })
  const base = `/p/${pid}`
  const items = [
    { to: base, label: "Overview", active: path === base || path === `${base}/` },
    {
      to: `${base}/episodes`,
      label: project?.kind === "movie" ? "Files" : "Episodes",
      active: path === `${base}/episodes`,
    },
    { to: `${base}/cast`, label: "Cast", active: path.startsWith(`${base}/cast`) },
    { to: `${base}/exports`, label: "Exports", active: path === `${base}/exports` },
  ]
  const pages = pagesFor(plugins, "project")
  return (
    <SegmentedNav aria-label="Project" items={items}>
      {pages.length > 0 && <MorePages pid={pid} pages={pages} active={path.startsWith(`${base}/x/`)} />}
    </SegmentedNav>
  )
}

/** The pages plugins add to each project, each named after its plugin. */
function MorePages({ pid, pages, active }: { pid: string; pages: PageItem[]; active: boolean }) {
  const navigate = useNavigate()
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <SegmentedNavButton active={active}>
          More <CaretDown className="-mr-1 size-3.5!" weight="bold" />
        </SegmentedNavButton>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="center">
        {pages.map((pg) => (
          <DropdownMenuItem key={pg.key} onSelect={() => void navigate({ to: pageLink(pg, pid) })}>
            <PluginIcon src={pg.icon} />
            {pageLabel(pg)}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
