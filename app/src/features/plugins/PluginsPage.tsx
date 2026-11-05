import { useState } from "react"
import { useNavigate, useSearch } from "@tanstack/react-router"
import type { PluginInfo } from "@/bindings"
import { ArrowsClockwise, FolderOpen, MagnifyingGlass, Plus, PuzzlePiece } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/empty-state"
import { ErrorState } from "@/components/ui/error-state"
import { Page } from "@/components/ui/page"
import { SearchField } from "@/components/ui/search-field"
import { Segmented } from "@/components/ui/segmented"
import { Section } from "@/components/ui/shelf"
import { Switch } from "@/components/ui/switch"
import { TopBar } from "@/components/ui/topbar"
import { plural } from "@/features/cast/cast"
import { BrowseView } from "@/features/plugins/BrowseView"
import { updateAll, useUpdatingAll } from "@/features/plugins/catalog-actions"
import { asksNothingNew, catalogLine, updatesOf } from "@/features/plugins/catalog-items"
import { InstallItems } from "@/features/plugins/InstallItems"
import { usePluginInstall } from "@/features/plugins/install-plugin"
import { useLoadAgain } from "@/features/plugins/load-again"
import { PluginCard } from "@/features/plugins/PluginCard"
import { PluginSkeletons } from "@/features/plugins/PluginSkeletons"
import { allowUnreviewed } from "@/features/plugins/plugin-actions"
import { FIND_FROM, pluginMatches } from "@/features/plugins/plugin-items"
import { UninstallDialog } from "@/features/plugins/UninstallDialog"
import { WithdrawnBanner } from "@/features/plugins/WithdrawnBanner"
import { SettingRow, SettingsGroup } from "@/features/settings/primitives"
import { CARD_GRID } from "@/lib/grid"
import { useCatalog, usePlugins, useSettings } from "@/lib/queries"

const ALLOW = "Allow plugins that AutoScene has not reviewed"

/** How many withdrawn plugins get a banner each: the rest are counted. */
const BANNERS = 3

type View = "installed" | "browse"

/**
 * Plugins, in two views (`?view=browse`). Installed: every installed plugin
 * as a card with its artwork, the ones the catalog has a newer version of
 * under Updates above the rest, a search from six of them, and who may run
 * code here below them. Browse: what AutoScene's catalog offers
 * (`BrowseView`), with its search while it has cards to find. Plugins
 * AutoScene withdrew have a banner each at the top of both, offering to go.
 */
export function PluginsPage() {
  const { data: settings } = useSettings()
  const { data: plugins, error, isFetching, refetch } = usePlugins()
  const { data: catalog, error: catalogError } = useCatalog()
  const search: { view?: unknown } = useSearch({ strict: false })
  const view: View = search.view === "browse" ? "browse" : "installed"
  const navigate = useNavigate()
  const flow = usePluginInstall()
  const { allowed, checking, install, loadFolder } = flow
  const [removing, setRemoving] = useState<PluginInfo | null>(null)
  const [query, setQuery] = useState("")
  // Each view starts unsearched, however it was reached (the pill, the palette, a link).
  const [searched, setSearched] = useState(view)
  if (searched !== view) {
    setSearched(view)
    setQuery("")
  }
  // A card's folder is read again on its own: the header's buttons don't spin for it.
  const again = useLoadAgain()
  // Browse finds among the catalog's cards: none show while it is out of date, closed or failed.
  const cards =
    catalog === undefined
      ? catalogError == null
      : catalog.status.ready && !catalog.status.needsNewerApp && !catalog.status.outOfDate && catalog.plugins.length > 0
  // The search stays while it narrows, however few plugins are left.
  const finding = (view === "browse" ? cards : (plugins?.length ?? 0) >= FIND_FROM) || query !== ""
  const line = catalog && catalogLine(catalog.status)
  const shown = plugins?.filter((p) => pluginMatches(p, query))
  const any = plugins != null && plugins.length > 0
  // The catalog's newer versions of installed plugins, by plugin.
  const updates = new Map(updatesOf(catalog?.plugins).map((u) => [u.id, u]))
  const updating = shown?.filter((p) => updates.has(p.id)) ?? []
  const rest = shown?.filter((p) => !updates.has(p.id)) ?? []
  // Update all takes only the updates that ask for nothing new.
  const quiet = [...updates.values()].filter(asksNothingNew)
  const updatingAll = useUpdatingAll((s) => s.updating)
  const withdrawn = plugins?.filter((p) => p.withdrawn != null) ?? []
  const go = (next: View) => {
    void navigate({ to: "/plugins", search: next === "browse" ? { view: "browse" } : {} })
  }

  // Browse installs from each card: the file is the other way, so it sits back.
  const installButton = (
    <Button size="lg" busy={checking === "file"} disabled={checking === "folder"} onClick={install}>
      <Plus weight="bold" /> Install from a file…
    </Button>
  )
  const folderButton = allowed && (
    <Button variant="ghost" size="lg" busy={checking === "folder"} disabled={checking === "file"} onClick={loadFolder}>
      <FolderOpen /> Load a folder…
    </Button>
  )
  const card = (p: PluginInfo) => (
    // A broken folder is loaded again through its review, the one way to agree to a changed program.
    <PluginCard
      key={p.id}
      p={p}
      update={updates.get(p.id)}
      onLoadAgain={() => void again.loadAgain(p)}
      onUninstall={() => setRemoving(p)}
    />
  )

  return (
    <Page topbar={<TopBar />} menu={<InstallItems flow={flow} />}>
      <header className="flex flex-wrap items-end justify-between gap-6 px-10 pt-10 pb-8">
        <div>
          <h1 className="display text-5xl text-fg-1">Plugins</h1>
          {view === "browse"
            ? line && <p className="mt-2 text-md text-fg-2">{line}</p>
            : any && (
                <p className="mt-2 text-md text-fg-2">
                  {/* What runs, not what is switched on: a broken or blocked one doesn't. */}
                  {plural(plugins.length, "plugin")} · {plugins.filter((p) => p.active).length.toLocaleString("en-GB")} running
                </p>
              )}
        </div>
        {/* With none installed, the empty state offers these: nothing is there twice. */}
        {view === "installed" && any && (
          <div className="flex items-center gap-2.5">
            {folderButton}
            {installButton}
          </div>
        )}
        <div className="flex basis-full flex-wrap items-center gap-3">
          <Segmented<View>
            aria-label="Plugins to show"
            value={view}
            onChange={go}
            options={[
              { value: "installed", label: "Installed" },
              { value: "browse", label: "Browse" },
            ]}
          />
          {(view === "browse" || any) && finding && (
            <SearchField value={query} onChange={setQuery} label="Find a plugin" />
          )}
        </div>
      </header>

      <div className="px-10 pb-16">
        {withdrawn.length > 0 && (
          <div className="mb-10 flex max-w-4xl flex-col gap-3">
            {withdrawn.slice(0, BANNERS).map((p) => (
              <WithdrawnBanner key={p.id} p={p} onUninstall={() => setRemoving(p)} />
            ))}
            {withdrawn.length > BANNERS && (
              <p className="px-1 text-sm text-fg-2">
                and {(withdrawn.length - BANNERS).toLocaleString("en-GB")} more {withdrawn.length - BANNERS === 1 ? "plugin" : "plugins"} AutoScene
                withdrew, marked on {withdrawn.length - BANNERS === 1 ? "its card" : "their cards"} under Installed.
              </p>
            )}
          </div>
        )}
        {view === "browse" ? (
          <BrowseView query={query} onClear={() => setQuery("")} />
        ) : error && plugins === undefined ? (
          <ErrorState
            error={error}
            icon={<PuzzlePiece />}
            title="Couldn't read the plugins"
            retrying={isFetching}
            onRetry={() => void refetch()}
          />
        ) : plugins === undefined ? (
          <PluginSkeletons />
        ) : !any ? (
          <EmptyState
            icon={<PuzzlePiece />}
            title="No plugins yet"
            description={
              allowed
                ? "Browse the plugins AutoScene reviewed, install one from its .asp file, or load one from its folder. Its actions then show in each export's menu, and its commands in the palette."
                : "Browse the plugins AutoScene reviewed, or install one from its .asp file. One AutoScene hasn't reviewed needs the setting below first. Its actions then show in each export's menu, and its commands in the palette."
            }
            actions={
              <>
                <Button variant="primary" size="lg" onClick={() => go("browse")}>
                  <PuzzlePiece /> Browse plugins
                </Button>
                {installButton}
                {folderButton}
              </>
            }
          />
        ) : shown?.length === 0 ? (
          <EmptyState
            icon={<MagnifyingGlass />}
            title="Nothing matches"
            description={`No plugin's name, author, description or id has “${query.trim()}”.`}
            actions={<Button onClick={() => setQuery("")}>Clear the search</Button>}
          />
        ) : updating.length === 0 ? (
          <div className={CARD_GRID}>{rest.map(card)}</div>
        ) : (
          <>
            <Section
              title="Updates"
              count={updating.length.toLocaleString("en-GB")}
              className="px-0"
              action={
                quiet.length > 0 && (
                  <Button size="sm" busy={updatingAll} onClick={() => void updateAll(quiet)}>
                    <ArrowsClockwise /> Update all ({quiet.length.toLocaleString("en-GB")})
                  </Button>
                )
              }
            >
              <div className={CARD_GRID}>{updating.map(card)}</div>
            </Section>
            {rest.length > 0 && (
              <Section title="Installed" count={rest.length.toLocaleString("en-GB")} className="mt-12 px-0">
                <div className={CARD_GRID}>{rest.map(card)}</div>
              </Section>
            )}
          </>
        )}

        {view === "installed" && (
          <div className="mt-14 max-w-4xl">
            <SettingsGroup title="Who may run code here">
              <SettingRow
                label={ALLOW}
                description="Unsigned plugins, and ones only their author signed, and plugins loaded from a folder. AutoScene can't check what they do."
              >
                <Switch aria-label={ALLOW} checked={allowed} disabled={!settings} onCheckedChange={(on) => void allowUnreviewed(on)} />
              </SettingRow>
            </SettingsGroup>
          </div>
        )}

        <UninstallDialog p={removing} onClose={() => setRemoving(null)} />
      </div>
    </Page>
  )
}
