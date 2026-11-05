import { toast } from "sonner"
import { create } from "zustand"
import type { CatalogPlugin, PluginReview } from "@/bindings"
import { commands, errorMessage, isAppError } from "@/lib/ipc"
import { qk, queryClient } from "@/lib/queries"
import { type ReviewAsk, usePluginReview } from "@/state/plugin-review"
import { asksNothingNew, oneClick } from "./catalog-items"
import { pluginToast } from "./plugin-toast"

type Entry = Pick<CatalogPlugin, "id" | "name">

/** How a catalog package is asked for: the dialog shows it coming, and its Cancel ends the get. */
const ask = (p: Entry, install?: ReviewAsk["install"]) => ({
  label: p.name,
  get: { pluginId: p.id, cancel: () => void commands.catalogCancel(p.id).catch(() => undefined) },
  install,
})

/**
 * Get a plugin from the catalog: AutoScene downloads and checks its
 * package, and its review asks, as for a package picked from a file.
 */
export async function getFromCatalog(p: Entry): Promise<void> {
  await usePluginReview.getState().openReview(() => commands.catalogGet(p.id), ask(p))
}

/**
 * Installs a reviewed update without its review when it asks for nothing new
 * (it says so in a toast): `true`, or "cancelled" when the person stopped its
 * compile. Otherwise `false`, and its review shows.
 */
async function installQuietly(r: PluginReview, name: string): Promise<boolean | "cancelled"> {
  if (!oneClick(r)) return false
  try {
    const p = await commands.pluginInstall(r.ticketId, false)
    void queryClient.invalidateQueries({ queryKey: qk.plugins })
    void queryClient.invalidateQueries({ queryKey: qk.catalog })
    pluginToast(p.name, { text: `updated to ${p.version}` })
  } catch (e) {
    void commands.pluginDiscard(r.ticketId).catch(() => undefined)
    if (isAppError(e) && e.code === "cancelled") return "cancelled"
    pluginToast(name, { text: "couldn't be updated", description: errorMessage(e), tone: "error" })
  } finally {
    usePluginReview.getState().compileEnded()
  }
  return true
}

/**
 * Update a plugin from the catalog: in one click when its review asks for
 * nothing new, else through that review. Whether it was decided without
 * the review (installed, stopped, or failed).
 */
export async function updatePlugin(p: Entry): Promise<boolean> {
  let quiet = false
  await usePluginReview.getState().openReview(
    () => commands.catalogGet(p.id),
    ask(p, async (r) => {
      const installed = await installQuietly(r, p.name)
      quiet = installed !== false
      return installed
    }),
  )
  return quiet
}

/** Update all under way: its button spins, and one runs at a time. */
export const useUpdatingAll = create<{ updating: boolean }>(() => ({ updating: false }))

/**
 * Update every plugin in `list` that asks for nothing new, in order. One
 * whose review asks for more after all stops it there, its review open; a
 * Cancel while one comes stops the rest too.
 */
export async function updateAll(list: CatalogPlugin[]) {
  if (useUpdatingAll.getState().updating) return
  useUpdatingAll.setState({ updating: true })
  try {
    for (const p of list.filter(asksNothingNew)) {
      const outcome = await usePluginReview.getState().openReview(
        () => commands.catalogGet(p.id),
        ask(p, (r) => installQuietly(r, p.name)),
      )
      if (outcome === "shown" || outcome === "cancelled") return
    }
  } finally {
    useUpdatingAll.setState({ updating: false })
  }
}

/** A check of the catalog under way, from Browse or the palette: its button spins, and one runs at a time. */
export const useCatalogCheck = create<{ checking: boolean }>(() => ({ checking: false }))

/** Check the catalog now, and say what came of it. */
export async function checkCatalog() {
  if (useCatalogCheck.getState().checking) return
  useCatalogCheck.setState({ checking: true })
  try {
    const view = await commands.catalogRefresh()
    queryClient.setQueryData(qk.catalog, view)
    void queryClient.invalidateQueries({ queryKey: qk.plugins })
    if (!view.status.ready) {
      const why = view.status.needsNewerApp ? "This catalog needs a newer AutoScene." : "This AutoScene has no plugin catalog."
      toast.error("Couldn't check the plugin catalog", { description: why })
      return
    }
    if (view.status.error != null) {
      toast.error("Couldn't check the plugin catalog", { description: view.status.error })
      return
    }
    const n = view.plugins.filter((p) => p.update && p.runsHere && !p.needsNewerApp).length
    toast(n === 0 ? "Plugins are up to date" : n === 1 ? "1 plugin update" : `${n} plugin updates`)
  } catch (e) {
    toast.error("Couldn't check the plugin catalog", { description: errorMessage(e) })
  } finally {
    useCatalogCheck.setState({ checking: false })
  }
}

/** Whether AutoScene checks the catalog once a day. */
export async function setDailyChecks(on: boolean) {
  const settings = queryClient.getQueryData<Awaited<ReturnType<typeof commands.settingsGet>>>(qk.settings)
  if (!settings) return
  try {
    const saved = await commands.settingsUpdate({ ...settings, pluginCatalogChecks: on })
    queryClient.setQueryData(qk.settings, saved)
    void queryClient.invalidateQueries({ queryKey: qk.catalog })
  } catch (e) {
    toast.error("Couldn't save the setting", { description: errorMessage(e) })
  }
}
