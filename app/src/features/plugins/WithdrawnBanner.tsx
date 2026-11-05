import { useState } from "react"
import { useTranslation } from "react-i18next"
import type { PluginInfo } from "@/bindings"
import { ArrowsClockwise, ShieldWarning, Trash } from "@/components/icons"
import { Banner } from "@/components/ui/banner"
import { Button } from "@/components/ui/button"
import { useCatalog } from "@/lib/queries"
import { getsPlugin, usePluginReview } from "@/state/plugin-review"
import { updatePlugin } from "./catalog-actions"
import { updatesOf } from "./catalog-items"
import { withdrawnLine } from "./plugin-items"
import { UninstallDialog } from "./UninstallDialog"

/**
 * AutoScene took a plugin back: the plugin doesn't run, and its banner says
 * so in AutoScene's words (the catalog's reason, never the plugin's own),
 * wherever the plugin's name shows: Plugins, its settings and its pages.
 * "Uninstall…" asks `onUninstall` to open the question; "Update…" shows
 * while the catalog offers a version that isn't withdrawn. None for a
 * plugin that isn't withdrawn.
 */
export function WithdrawnBanner({ p, onUninstall, className }: { p: PluginInfo; onUninstall: () => void; className?: string }) {
  const { t } = useTranslation("plugins")
  const { data: catalog } = useCatalog()
  const update = updatesOf(catalog?.plugins).find((u) => u.id === p.id)
  // Its update is being got, or waits its turn: a second click would only ask again.
  const getting = usePluginReview((s) => getsPlugin(s, p.id))
  if (p.withdrawn == null) return null
  return (
    <Banner
      tone="danger"
      icon={<ShieldWarning weight="fill" />}
      title={t("withdrawn.title", { name: p.name })}
      description={withdrawnLine(p.withdrawn)}
      actions={
        <>
          {update && (
            <Button variant="secondary" busy={getting} onClick={() => void updatePlugin(update)}>
              <ArrowsClockwise /> {t("menu.update")}
            </Button>
          )}
          <Button variant="danger" onClick={onUninstall}>
            <Trash /> {t("menu.uninstall")}
          </Button>
        </>
      }
      className={className}
    />
  )
}

/**
 * The banner with the question it opens, for a screen that has none of its
 * own. `onUninstalled` runs once the plugin has gone, before the list
 * forgets it: the screen leaves there, so it never says the plugin isn't installed.
 */
export function WithdrawnNotice({
  p,
  onUninstalled,
  className,
}: {
  p: PluginInfo
  onUninstalled?: () => Promise<unknown> | void
  className?: string
}) {
  const [removing, setRemoving] = useState(false)
  if (p.withdrawn == null) return null
  return (
    <>
      <WithdrawnBanner p={p} onUninstall={() => setRemoving(true)} className={className} />
      <UninstallDialog p={removing ? p : null} onClose={() => setRemoving(false)} onUninstalled={onUninstalled} />
    </>
  )
}
