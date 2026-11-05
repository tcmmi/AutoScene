import { ArrowCounterClockwise, ArrowsClockwise, DownloadSimple, MagnifyingGlass, PuzzlePiece, Warning } from "@/components/icons"
import { Banner } from "@/components/ui/banner"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/empty-state"
import { ErrorState } from "@/components/ui/error-state"
import { Switch } from "@/components/ui/switch"
import { SettingRow, SettingsGroup } from "@/features/settings/primitives"
import { useAppActions } from "@/lib/actions"
import { CARD_GRID } from "@/lib/grid"
import { useCatalog, useSettings } from "@/lib/queries"
import { CatalogCard } from "./CatalogCard"
import { checkCatalog, setDailyChecks, useCatalogCheck } from "./catalog-actions"
import { catalogMatches, stuckSince } from "./catalog-items"
import { PluginSkeletons } from "./PluginSkeletons"

const DAILY = "Check the plugin catalog every day"

/**
 * Browse: what AutoScene's catalog offers, each plugin as a card, found by
 * the header's search (`query`). Above the cards, the catalog itself: when
 * it was checked, Check now, and whether it is checked every day.
 */
export function BrowseView({ query, onClear }: { query: string; onClear: () => void }) {
  const { data: view, error, isFetching, refetch } = useCatalog()
  const { data: settings } = useSettings()
  const checking = useCatalogCheck((s) => s.checking)
  const { checkUpdates } = useAppActions()

  if (error && view === undefined)
    return (
      <ErrorState
        error={error}
        icon={<PuzzlePiece />}
        title="Couldn't read the plugin catalog"
        retrying={isFetching}
        onRetry={() => void refetch()}
      />
    )
  if (view === undefined) return <PluginSkeletons />
  const { status } = view
  if (status.needsNewerApp)
    return (
      <EmptyState
        icon={<DownloadSimple />}
        title="Update AutoScene to use the plugin catalog"
        description="A later AutoScene checked the catalog, or its index needs one. Plugins still install from their .asp file."
        actions={
          <Button onClick={checkUpdates}>
            <DownloadSimple /> Check for updates
          </Button>
        }
      />
    )
  if (!status.ready)
    return (
      <EmptyState
        icon={<PuzzlePiece />}
        title="The plugin catalog isn't open yet"
        description="This AutoScene has no plugin catalog. Plugins still install from their .asp file."
      />
    )

  const daily = settings?.pluginCatalogChecks ?? status.daily
  const shown = view.plugins.filter((p) => catalogMatches(p, query))
  const since = stuckSince(status)
  return (
    <>
      {status.outOfDate && (
        <Banner
          tone="accent"
          icon={<Warning weight="fill" />}
          title="The plugin catalog is out of date"
          description={`${since ? `AutoScene couldn't check it since ${since}.` : "AutoScene couldn't check it yet."} Plugins can't be installed from it until it can.`}
          actions={
            <Button busy={checking} onClick={() => void checkCatalog()}>
              <ArrowCounterClockwise /> Try again
            </Button>
          }
          className="mb-10 max-w-4xl"
        />
      )}

      <div className="max-w-4xl">
        <SettingsGroup title="Catalog">
          <SettingRow
            label="AutoScene's plugin catalog"
            description={
              // The banner says it while it is up: once is enough.
              status.error != null && !status.outOfDate ? (
                <span className="text-danger">{status.error}</span>
              ) : (
                "Plugins AutoScene reviewed, each built from its public source."
              )
            }
          >
            <Button size="sm" busy={checking} onClick={() => void checkCatalog()}>
              <ArrowsClockwise /> Check now
            </Button>
          </SettingRow>
          <SettingRow
            label={DAILY}
            description={
              daily
                ? "AutoScene asks GitHub once a day for new versions and withdrawn plugins."
                : "Off: AutoScene doesn't learn of withdrawn plugins."
            }
          >
            <Switch aria-label={DAILY} checked={daily} disabled={!settings} onCheckedChange={(on) => void setDailyChecks(on)} />
          </SettingRow>
        </SettingsGroup>
      </div>

      {status.outOfDate ? null : view.plugins.length === 0 ? (
        <EmptyState icon={<PuzzlePiece />} title="The catalog lists no plugins yet" description="Check again later." />
      ) : shown.length === 0 ? (
        <EmptyState
          icon={<MagnifyingGlass />}
          title="Nothing matches"
          description={`No plugin in the catalog has “${query.trim()}” in its name, author, summary or id.`}
          actions={<Button onClick={onClear}>Clear the search</Button>}
        />
      ) : (
        <div className={CARD_GRID}>
          {shown.map((p) => (
            <CatalogCard key={p.id} p={p} />
          ))}
        </div>
      )}
    </>
  )
}
