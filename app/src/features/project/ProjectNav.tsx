import { useNavigate, useRouterState } from "@tanstack/react-router"
import { useTranslation } from "react-i18next"
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
  const { t } = useTranslation("project")
  const { data: project } = useProject(pid)
  const { data: plugins } = usePlugins()
  const path = useRouterState({ select: (s) => s.location.pathname })
  const base = `/p/${pid}`
  const items = [
    { to: base, label: t("nav.overview"), active: path === base || path === `${base}/` },
    {
      to: `${base}/episodes`,
      label: project?.kind === "movie" ? t("nav.files") : t("nav.episodes"),
      active: path === `${base}/episodes`,
    },
    { to: `${base}/cast`, label: t("nav.cast"), active: path.startsWith(`${base}/cast`) },
    { to: `${base}/exports`, label: t("nav.exports"), active: path === `${base}/exports` },
  ]
  const pages = pagesFor(plugins, "project")
  return (
    <SegmentedNav aria-label={t("nav.label")} items={items}>
      {pages.length > 0 && <MorePages pid={pid} pages={pages} active={path.startsWith(`${base}/x/`)} />}
    </SegmentedNav>
  )
}

/** The pages plugins add to each project, each named after its plugin. */
function MorePages({ pid, pages, active }: { pid: string; pages: PageItem[]; active: boolean }) {
  const { t } = useTranslation("project")
  const navigate = useNavigate()
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <SegmentedNavButton active={active}>
          {t("nav.more")} <CaretDown className="-mr-1 size-3.5!" weight="bold" />
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
