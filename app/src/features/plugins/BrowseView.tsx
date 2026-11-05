import { useTranslation } from "react-i18next"
import { ArrowCounterClockwise, ArrowsClockwise, DownloadSimple, MagnifyingGlass, PuzzlePiece, Warning } from "@/components/icons"
import { Banner } from "@/components/ui/banner"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/ui/empty-state"
import { ErrorState } from "@/components/ui/error-state"
import { Switch } from "@/components/ui/switch"
import { SettingRow, SettingsGroup } from "@/features/settings/primitives"
import { useAppActions } from "@/lib/actions"
import { CARD_GRID } from "@/lib/grid"
import { say } from "@/lib/i18n"
import { useCatalog, useSettings } from "@/lib/queries"
import { CatalogCard } from "./CatalogCard"
import { checkCatalog, setDailyChecks, useCatalogCheck } from "./catalog-actions"
import { catalogMatches, stuckSince } from "./catalog-items"
import { PluginSkeletons } from "./PluginSkeletons"

/**
 * Browse: what AutoScene's catalog offers, each plugin as a card, found by
 * the header's search (`query`). Above the cards, the catalog itself: when
 * it was checked, Check now, and whether it is checked every day.
 */
export function BrowseView({ query, onClear }: { query: string; onClear: () => void }) {
  const { t } = useTranslation("plugins")
  const { data: view, error, isFetching, refetch } = useCatalog()
  const { data: settings } = useSettings()
  const checking = useCatalogCheck((s) => s.checking)
  const { checkUpdates } = useAppActions()

  if (error && view === undefined)
    return (
      <ErrorState
        error={error}
        icon={<PuzzlePiece />}
        title={t("browse.readFailed")}
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
        title={t("browse.newer.title")}
        description={t("browse.newer.description")}
        actions={
          <Button onClick={checkUpdates}>
            <DownloadSimple /> {t("browse.newer.check")}
          </Button>
        }
      />
    )
  if (!status.ready)
    return (
      <EmptyState
        icon={<PuzzlePiece />}
        title={t("browse.closed.title")}
        description={t("browse.closed.description")}
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
          title={t("browse.outOfDate.title")}
          description={since ? t("browse.outOfDate.since", { date: since }) : t("browse.outOfDate.never")}
          actions={
            <Button busy={checking} onClick={() => void checkCatalog()}>
              <ArrowCounterClockwise /> {t("browse.outOfDate.tryAgain")}
            </Button>
          }
          className="mb-10 max-w-4xl"
        />
      )}

      <div className="max-w-4xl">
        <SettingsGroup title={t("browse.group")}>
          <SettingRow
            label={t("browse.catalog.label")}
            description={
              // The banner says it while it is up: once is enough.
              status.error != null && !status.outOfDate ? (
                <span className="text-danger">{say(status.error)}</span>
              ) : (
                t("browse.catalog.description")
              )
            }
          >
            <Button size="sm" busy={checking} onClick={() => void checkCatalog()}>
              <ArrowsClockwise /> {t("browse.catalog.checkNow")}
            </Button>
          </SettingRow>
          <SettingRow
            label={t("browse.daily.label")}
            description={daily ? t("browse.daily.on") : t("browse.daily.off")}
          >
            <Switch aria-label={t("browse.daily.label")} checked={daily} disabled={!settings} onCheckedChange={(on) => void setDailyChecks(on)} />
          </SettingRow>
        </SettingsGroup>
      </div>

      {status.outOfDate ? null : view.plugins.length === 0 ? (
        <EmptyState icon={<PuzzlePiece />} title={t("browse.none.title")} description={t("browse.none.description")} />
      ) : shown.length === 0 ? (
        <EmptyState
          icon={<MagnifyingGlass />}
          title={t("browse.noMatch.title")}
          description={t("browse.noMatch.description", { query: query.trim() })}
          actions={<Button onClick={onClear}>{t("browse.noMatch.clear")}</Button>}
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
