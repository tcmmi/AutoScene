import type * as React from "react"
import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  redirect,
} from "@tanstack/react-router"
import { AppShell } from "@/components/shell/AppShell"
import { CastPage } from "@/features/cast/CastPage"
import { CharacterPage } from "@/features/cast/CharacterPage"
import { ReviewPage } from "@/features/cast/ReviewPage"
import { LibraryPage } from "@/features/library/LibraryPage"
import { PlayerPage } from "@/features/player/PlayerPage"
import { EpisodesPage } from "@/features/project/EpisodesPage"
import { ExportsPage } from "@/features/project/ExportsPage"
import { OverviewPage } from "@/features/project/OverviewPage"
import { CharacterCrumb, MediaCrumb, PackCrumb, ProjectCrumb } from "@/features/project/crumbs"
import { PluginPageCrumb, PluginPageView } from "@/features/plugins/PluginPageView"
import { PluginCrumb, PluginSettingsPage } from "@/features/plugins/PluginSettingsPage"
import { PluginsPage } from "@/features/plugins/PluginsPage"
import { SettingsPage } from "@/features/settings/SettingsPage"
import { settingsLabel } from "@/features/settings/sections"
import { NotFound } from "@/features/not-found"
import { PackPage } from "@/features/pack/PackPage"
import { RouteError } from "@/features/route-error"
import { type LegacySettingsSection, type SettingsSection, legacySettingsTarget, settingsSection } from "@/lib/actions"
import { followPlaces } from "@/lib/places"

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
  interface StaticDataRouteOption {
    /** Name of the view in the window title (see WindowTitle). */
    crumb?: React.ReactNode | ((params: Record<string, string>) => React.ReactNode)
  }
}

const rootRoute = createRootRoute({ component: AppShell })

const libraryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: LibraryPage,
})

const pluginsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/plugins",
  staticData: { crumb: "Plugins" },
  component: PluginsPage,
})

// A plugin's settings: its own screen under Plugins. Whether it is
// installed shows there.
const pluginRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/plugins/$id",
  staticData: { crumb: (p) => <PluginCrumb id={p.id} /> },
  component: PluginSettingsPage,
})

const projectRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/p/$pid",
  staticData: { crumb: (p) => <ProjectCrumb pid={p.pid} /> },
  component: Outlet,
})

const projectIndexRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/",
  component: OverviewPage,
})

const episodesRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/episodes",
  component: EpisodesPage,
})

const playerRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/timeline/$mid",
  staticData: { crumb: (p) => <MediaCrumb pid={p.pid} mid={p.mid} /> },
  // `f`: frame to open at (from a face in the cast).
  validateSearch: (s: Record<string, unknown>): { f?: number } => {
    const f = Number(s.f)
    return Number.isInteger(f) && f >= 0 ? { f } : {}
  },
  component: PlayerPage,
})

const castRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/cast",
  staticData: { crumb: "Cast" },
  component: CastPage,
})

const reviewRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/cast/review",
  staticData: { crumb: "Cast · Review" },
  // `c`: only the faces suggested for this character.
  validateSearch: (s: Record<string, unknown>): { c?: string } => (typeof s.c === "string" ? { c: s.c } : {}),
  component: ReviewPage,
})

const characterRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/cast/$cid",
  staticData: { crumb: (p) => <CharacterCrumb pid={p.pid} cid={p.cid} /> },
  component: CharacterPage,
})

const packRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/cast/$cid/pack",
  staticData: { crumb: (p) => <PackCrumb pid={p.pid} cid={p.cid} /> },
  component: PackPage,
})

const exportsRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: "/exports",
  staticData: { crumb: "Exports" },
  component: ExportsPage,
})

// A plugin's page: on its own, or in a project. The splat is the path inside
// the page; an empty one is its start. Ids are checked by the page, so a bad
// link is no error while routes are matched.
const pluginPagePath = "/x/$plugin/$page/$"

const pluginPageRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: pluginPagePath,
  staticData: { crumb: (p) => <PluginPageCrumb plugin={p.plugin} page={p.page} /> },
  component: PluginPageView,
})

const projectPluginPageRoute = createRoute({
  getParentRoute: () => projectRoute,
  path: pluginPagePath,
  staticData: { crumb: (p) => <PluginPageCrumb plugin={p.plugin} page={p.page} /> },
  component: PluginPageView,
})

const settingsIndexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  beforeLoad: () => {
    throw redirect({ to: "/settings/$section", params: { section: "general" } })
  },
})

// Plugins were a section of Settings, and so were each plugin's settings.
// They have their own place now, and an old link goes there. The parse
// keeps what such a link names, since it runs before `beforeLoad` sees it.
const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings/$section",
  staticData: { crumb: (p) => `Settings · ${settingsLabel(p.section)}` },
  params: {
    parse: (p): { section: SettingsSection | LegacySettingsSection } => ({ section: settingsSection(p.section) }),
    stringify: (p) => ({ section: p.section }),
  },
  beforeLoad: ({ params }) => {
    const to = legacySettingsTarget(params.section)
    if (to) throw redirect({ ...to, replace: true })
  },
  component: SettingsPage,
})

export const routeTree = rootRoute.addChildren([
  libraryRoute,
  pluginsRoute,
  pluginRoute,
  projectRoute.addChildren([
    projectIndexRoute,
    episodesRoute,
    playerRoute,
    castRoute,
    reviewRoute,
    characterRoute,
    packRoute,
    exportsRoute,
    projectPluginPageRoute,
  ]),
  pluginPageRoute,
  settingsIndexRoute,
  settingsRoute,
])

// Hash history: the app is served from tauri://localhost with no server-side routing.
export const router = createRouter({
  routeTree,
  history: createHashHistory(),
  defaultPreload: "intent",
  // Pages remember their own scrolling (they scroll inside the window, see Page).
  scrollRestoration: false,
  defaultNotFoundComponent: NotFound,
  defaultErrorComponent: RouteError,
})

// Where a plugin page was opened from, which its Close goes back to.
followPlaces(router.history)
