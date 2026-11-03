import type { PageScope, PluginInfo } from "@/bindings"

/** The most plugin pages the rail shows; the others are in the palette. */
export const MAX_RAIL_PAGES = 4

/** A page a plugin that may run now adds, where the app offers it. */
export type PageItem = {
  key: string
  pluginId: string
  pluginName: string
  icon: string | null
  pageId: string
  title: string
  scope: PageScope
  /** A global page that asks for a tile in the rail. */
  rail: boolean
}

/** The pages of `scope` of the plugins that may run now, plugin by plugin, in the order each lists them. */
export function pagesFor(plugins: PluginInfo[] | undefined, scope: PageScope): PageItem[] {
  return (plugins ?? [])
    .filter((p) => p.active)
    .flatMap((p) =>
      p.pages
        .filter((pg) => pg.scope === scope)
        .map((pg) => ({
          key: `${p.id}.${pg.id}`,
          pluginId: p.id,
          pluginName: p.name,
          icon: p.icon,
          pageId: pg.id,
          title: pg.title,
          scope: pg.scope,
          rail: pg.rail,
        })),
    )
}

/** The pages with a tile in the rail: global ones that ask for it, at most four. */
export function railPages(plugins: PluginInfo[] | undefined): PageItem[] {
  return pagesFor(plugins, "global")
    .filter((pg) => pg.rail)
    .slice(0, MAX_RAIL_PAGES)
}

/** Where a page opens: on its own, or in the project `pid` (a project page). */
export function pageLink(item: Pick<PageItem, "pluginId" | "pageId">, pid?: string): string {
  const page = `/x/${item.pluginId}/${item.pageId}`
  return pid ? `/p/${pid}${page}` : page
}

/**
 * A page as the app names it, in its menus and the window's title: always
 * after a word the plugin can't choose, so a page called "Settings" still
 * reads as a plugin's.
 */
export function pageLabel(item: Pick<PageItem, "pluginName" | "title">): string {
  return `Plugin “${item.pluginName}” · ${item.title}`
}

/** A rail tile's tooltip: `Plugin “<name>”: <title>`. */
export function railTooltip(item: Pick<PageItem, "pluginName" | "title">): string {
  return `Plugin “${item.pluginName}”: ${item.title}`
}

/** Whether `s` may be a plugin's id: 2 to 5 parts of lowercase letters, digits and dashes, joined by dots. */
export function isPluginId(s: string | undefined): s is string {
  const parts = s?.split(".") ?? []
  return parts.length >= 2 && parts.length <= 5 && parts.every((p) => /^[a-z0-9](?:[a-z0-9-]{0,37}[a-z0-9])?$/.test(p))
}

/** Whether `s` may be the id of a page a plugin adds: lowercase letters, digits and dashes, at most 32. */
export function isPageId(s: string | undefined): s is string {
  return s != null && /^[a-z0-9][a-z0-9-]{0,31}$/.test(s)
}

/** Whether `path` (the window's) is a plugin page's: `/x/…` or `/p/<project>/x/…`. */
export function isPluginPagePath(path: string): boolean {
  return /^(?:\/p\/[^/]+)?\/x\//.test(path)
}
