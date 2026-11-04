import { ArrowCounterClockwise, ArrowsClockwise, CircleNotch, DownloadSimple, MagnifyingGlass, PuzzlePiece, Warning } from "@/components/icons"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/empty-state"
import { Switch } from "@/components/ui/switch"
import { SettingRow, SettingsGroup } from "@/features/settings/primitives"
import { useAppActions } from "@/lib/actions"
import { errorMessage } from "@/lib/ipc"
import { useCatalog, useSettings } from "@/lib/queries"
import { CatalogCard } from "./CatalogCard"
import { checkCatalog, setDailyChecks, useCatalogCheck } from "./catalog-actions"
import { catalogMatches, stuckSince } from "./catalog-items"

const DAILY = "Check the plugin catalog every day"

/** The grid of a project's library, as Installed has it. */
const GRID = "grid grid-cols-[repeat(auto-fill,minmax(300px,1fr))] gap-x-6 gap-y-8"

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
      <EmptyState
        icon={<PuzzlePiece />}
        tone="danger"
        title="Couldn't read the plugin catalog"
        description={errorMessage(error)}
        actions={
          <Button disabled={isFetching} onClick={() => void refetch()}>
            {isFetching ? <CircleNotch className="animate-spin" /> : <ArrowCounterClockwise />} Try again
          </Button>
        }
      />
    )
  if (view === undefined)
    return (
      <div className={GRID}>
        {Array.from({ length: 3 }, (_, i) => (
          <div key={i} className="skeleton aspect-[2/1] rounded-2xl" />
        ))}
      </div>
    )
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
        <div
          role="status"
          className="mb-10 flex max-w-4xl flex-wrap items-center gap-x-4 gap-y-3 rounded-2xl border border-accent-line/50 bg-accent-subtle px-5 py-4"
        >
          <Warning weight="fill" className="size-5 shrink-0 text-accent" />
          <div className="min-w-0 flex-1">
            <p className="text-md font-medium text-fg-1">The plugin catalog is out of date</p>
            <p className="text-sm text-fg-2">
              {since ? `AutoScene couldn't check it since ${since}.` : "AutoScene couldn't check it yet."} Plugins can't be
              installed from it until it can.
            </p>
          </div>
          <Button disabled={checking} onClick={() => void checkCatalog()}>
            {checking ? <CircleNotch className="animate-spin" /> : <ArrowCounterClockwise />} Try again
          </Button>
        </div>
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
            <Button size="sm" disabled={checking} onClick={() => void checkCatalog()}>
              {checking ? <CircleNotch className="animate-spin" /> : <ArrowsClockwise />} Check now
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
        <div className={GRID}>
          {shown.map((p) => (
            <CatalogCard key={p.id} p={p} />
          ))}
        </div>
      )}
    </>
  )
}
