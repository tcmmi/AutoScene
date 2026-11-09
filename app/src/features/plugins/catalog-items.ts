import type { CatalogPlugin, CatalogStatus, Permission, PluginReview } from "@/bindings"
import { fmt, t } from "@/lib/i18n"
import { downloadSize, joinWords, permissionLabel, pluginMatches } from "./plugin-items"

/**
 * Whether a catalog update may install without its review: AutoScene
 * reviewed it, it replaces a reviewed package installed (not a developer's
 * folder, nor one AutoScene didn't review), and it changes nothing the
 * review would mark New: no permission, host or page, no host its large
 * files come from, no program where none ran, none kept running, none
 * outside the sandbox it ran in, no higher limits in it. One that would need
 * uv first shows too.
 * The backend doesn't take this page's word for it: an update that may do
 * more takes a yes in a dialog of the system's own (`plugins::install`).
 */
export function oneClick(r: PluginReview): boolean {
  const u = r.update
  return (
    u != null &&
    r.plugin.trust === "reviewed" &&
    !r.needsUnreviewed &&
    !r.uvMissing &&
    u.trustBefore === "reviewed" &&
    u.sourceBefore === "package" &&
    u.addedPermissions.length === 0 &&
    u.addedHosts.length === 0 &&
    u.addedResourceHosts.length === 0 &&
    u.addedPages.length === 0 &&
    u.addedShotFilters.length === 0 &&
    u.addedPackActions.length === 0 &&
    u.addedSources.length === 0 &&
    u.addedLinkHosts.length === 0 &&
    !u.startsProgram &&
    !u.becomesService &&
    !u.leavesSandbox &&
    !u.raisedLimits
  )
}

/** Whether `p` is a catalog entry `query` looks for: as an installed plugin is found, by its summary for a description. */
export function catalogMatches(p: Pick<CatalogPlugin, "name" | "summary" | "author" | "id">, query: string): boolean {
  return pluginMatches({ name: p.name, description: p.summary, author: p.author, id: p.id }, query)
}

/** The newer versions the catalog offers of installed plugins that run here, in its order. */
export function updatesOf(catalog: CatalogPlugin[] | undefined): CatalogPlugin[] {
  return (catalog ?? []).filter((p) => p.update && p.runsHere && !p.needsNewerApp && p.installed != null)
}

/**
 * Whether an update asks for nothing new, so Update all takes it: its review
 * still says so before it installs. A new host its large files come from is
 * something new; a larger download from known hosts is not (the files'
 * hashes are what was reviewed, and the review still states their size). So
 * is running in another way than the person agreed to: a program where none
 * ran, one kept running, one outside the sandbox.
 */
export const asksNothingNew = (
  p: Pick<CatalogPlugin, "addsPermissions" | "addsHosts" | "addsResourceHosts" | "startsProgram" | "becomesService" | "leavesSandbox">,
) =>
  p.addsPermissions.length === 0 &&
  p.addsHosts.length === 0 &&
  p.addsResourceHosts.length === 0 &&
  !p.startsProgram &&
  !p.becomesService &&
  !p.leavesSandbox

/**
 * What a catalog entry says it may do, in one line: its permissions in
 * plain words, the network as the hosts it talks to, and the graphics card.
 */
export function catalogAsks(p: Pick<CatalogPlugin, "permissions" | "hosts" | "gpu">): string {
  const parts = p.permissions.map((perm) =>
    perm === "network" && p.hosts.length > 0 ? t("plugins:catalog.talksTo", { hosts: p.hosts.join(", ") }) : permissionLabel(perm as Permission),
  )
  if (p.gpu) parts.push(t("plugins:catalog.gpu"))
  return parts.length > 0 ? parts.join(" · ") : t("plugins:catalog.nothing")
}

/** What a catalog entry's large files take its first run, "Downloads 312 MB at its first run": none when it names none. */
export function catalogDownloads(p: Pick<CatalogPlugin, "resourceBytes" | "runtime">): string | null {
  const bytes = p.resourceBytes ?? 0
  if (bytes <= 0) return null
  const size = downloadSize(bytes)
  return p.runtime === "service" ? t("plugins:catalog.downloadsStart", { size }) : t("plugins:catalog.downloadsRun", { size })
}

/** What an update's card says when its files come from a host the installed version had none from: "Downloads from a new host: huggingface.co". */
export function catalogNewHosts(p: Pick<CatalogPlugin, "addsResourceHosts">): string | null {
  const n = p.addsResourceHosts.length
  return n > 0 ? t("plugins:catalog.newHosts", { count: n, hosts: joinWords(p.addsResourceHosts) }) : null
}

const DAY_MS = 86_400_000

/** A day as people say it: "3 November 2026". */
export function catalogDay(ms: number): string {
  return fmt.longDate(ms)
}

/** How many calendar days before `now` `ms` was: 0 is today, 1 yesterday. */
function daysAgo(ms: number, now: number): number {
  const day = (t: number) => {
    const d = new Date(t)
    return Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / DAY_MS
  }
  return day(now) - day(ms)
}

/**
 * The catalog's line under the title: which catalog is held and when
 * AutoScene last checked it, "Catalog of 3 November 2026. Checked today.",
 * or that the last check failed. None while the catalog isn't open: Browse
 * says why on its own.
 */
export function catalogLine(s: CatalogStatus, now = Date.now()): string | null {
  if (!s.ready) return null
  const of = s.published != null ? t("plugins:catalog.of", { date: catalogDay(s.published) }) : t("plugins:catalog.noneYet")
  // `checkedAt` is the last try: "Checked" only when it went through.
  return `${of} ${checkedSentence(s, now)}`
}

/** Since when the catalog couldn't be checked: its last fetch, else its publication, else nothing known. */
export function stuckSince(s: CatalogStatus): string | null {
  const since = s.fetchedAt ?? s.published
  return since != null ? catalogDay(since) : null
}

/** One sentence on the last try: when it was, and whether it went through. */
function checkedSentence(s: CatalogStatus, now: number): string {
  if (s.checkedAt == null) return t("plugins:catalog.notChecked")
  const ago = daysAgo(s.checkedAt, now)
  const date = catalogDay(s.checkedAt)
  if (s.error != null) {
    if (ago === 0) return t("plugins:catalog.failedToday")
    return ago === 1 ? t("plugins:catalog.failedYesterday") : t("plugins:catalog.failedOn", { date })
  }
  if (ago === 0) return t("plugins:catalog.checkedToday")
  return ago === 1 ? t("plugins:catalog.checkedYesterday") : t("plugins:catalog.checkedOn", { date })
}
