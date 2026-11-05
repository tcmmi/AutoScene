import type { CatalogPlugin, CatalogStatus, Permission, PluginReview } from "@/bindings"
import { downloadSize, firstTime, joinWords, permissionLabel, pluginMatches } from "./plugin-items"

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
    perm === "network" && p.hosts.length > 0 ? `Talks to ${p.hosts.join(", ")}` : permissionLabel(perm as Permission),
  )
  if (p.gpu) parts.push("Uses the graphics card")
  return parts.length > 0 ? parts.join(" · ") : "Names no permissions"
}

/** What a catalog entry's large files take its first run, "Downloads 312 MB at its first run": none when it names none. */
export function catalogDownloads(p: Pick<CatalogPlugin, "resourceBytes" | "runtime">): string | null {
  const bytes = p.resourceBytes ?? 0
  return bytes > 0 ? `Downloads ${downloadSize(bytes)} at its ${firstTime(p.runtime)}` : null
}

/** What an update's card says when its files come from a host the installed version had none from: "Downloads from a new host: huggingface.co". */
export function catalogNewHosts(p: Pick<CatalogPlugin, "addsResourceHosts">): string | null {
  const n = p.addsResourceHosts.length
  return n > 0 ? `Downloads from ${n === 1 ? "a new host" : "new hosts"}: ${joinWords(p.addsResourceHosts)}` : null
}

const DAY_MS = 86_400_000

/** A day as people say it: "3 November 2026". */
export function catalogDay(ms: number): string {
  return new Date(ms).toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" })
}

/** "today", "yesterday" or "on 3 November 2026", by the calendar of `now`. */
function when(ms: number, now: number): string {
  const day = (t: number) => {
    const d = new Date(t)
    return Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()) / DAY_MS
  }
  const ago = day(now) - day(ms)
  if (ago === 0) return "today"
  if (ago === 1) return "yesterday"
  return `on ${catalogDay(ms)}`
}

/**
 * The catalog's line under the title: which catalog is held and when
 * AutoScene last checked it, "Catalog of 3 November 2026. Checked today.",
 * or that the last check failed. None while the catalog isn't open: Browse
 * says why on its own.
 */
export function catalogLine(s: CatalogStatus, now = Date.now()): string | null {
  if (!s.ready) return null
  const of = s.published != null ? `Catalog of ${catalogDay(s.published)}.` : "No catalog yet."
  // `checkedAt` is the last try: "Checked" only when it went through.
  const checked =
    s.checkedAt == null
      ? " Not checked yet."
      : s.error != null
        ? ` Last check failed ${when(s.checkedAt, now)}.`
        : ` Checked ${when(s.checkedAt, now)}.`
  return of + checked
}

/** Since when the catalog couldn't be checked: its last fetch, else its publication, else nothing known. */
export function stuckSince(s: CatalogStatus): string | null {
  const since = s.fetchedAt ?? s.published
  return since != null ? catalogDay(since) : null
}
