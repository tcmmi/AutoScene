import type { Permission, PluginInfo, PluginReview, PluginUpdate, Trust } from "@/bindings"
import { formatBytes } from "@/lib/format"

/**
 * How far a plugin is trusted, as its badge says it. A plugin AutoScene
 * withdrew says that before anything: whatever else it is, it doesn't run.
 * A loaded folder says that next: it is never reviewed. A signature proves
 * no one's name (the key may come in the package), so a signed plugin isn't
 * marked as vouched for.
 */
export function trustLabel(p: Pick<PluginInfo, "trust" | "source"> & Partial<Pick<PluginInfo, "withdrawn">>): {
  label: string
  tone: "neutral" | "danger" | "outline"
} {
  if (p.withdrawn != null) return { label: "Withdrawn", tone: "danger" }
  if (p.source === "folder") return { label: "Loaded from a folder", tone: "outline" }
  return TRUST[p.trust]
}

const TRUST: Record<Trust, { label: string; tone: "neutral" | "danger" | "outline" }> = {
  reviewed: { label: "Reviewed by AutoScene", tone: "neutral" },
  pinned: { label: "Signed, not reviewed", tone: "outline" },
  unreviewed: { label: "Not reviewed", tone: "danger" },
}

/**
 * What a withdrawn plugin's banner says under its title: the catalog's
 * `reason` (AutoScene's words, as a sentence), then what that means.
 */
export function withdrawnLine(reason: string): string {
  const said = reason.trim()
  return `${said}${/[.!?]$/.test(said) ? "" : "."} It is turned off.`
}

/**
 * What a plugin's trust means, in one or two sentences for its review. A
 * signed update of a signed plugin used its pinned key (another is
 * refused); a first signature only brings a key along.
 */
export function trustLine(p: Pick<PluginInfo, "trust" | "source">, update?: Pick<PluginUpdate, "trustBefore" | "sourceBefore"> | null): string {
  if (p.source === "folder") return "It runs from its folder, as the files there are now. AutoScene has not reviewed it."
  switch (p.trust) {
    case "reviewed":
      return "AutoScene reviewed this version."
    case "pinned":
      return update?.trustBefore === "pinned" && update.sourceBefore === "package"
        ? "Signed with the same key as the installed version. AutoScene has not reviewed it."
        : "Signed with a key that came in the package: nothing shows who holds it. AutoScene has not reviewed it. Its updates must be signed with the same key."
    case "unreviewed":
      return "Nobody AutoScene knows signed it, and AutoScene has not reviewed it."
  }
}

/** "a", "a and b", "a, b and c". */
export function joinWords(items: string[]): string {
  if (items.length < 2) return items.join("")
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`
}

/** When a program first needs what it downloads: its first run, or a service's first start. */
export const firstTime = (runtime: string) => (runtime === "service" ? "first start" : "first run")

/** When a program next needs what it downloads: its next run, or a service's next start (an update's files). */
export const nextTime = (runtime: string) => (runtime === "service" ? "next start" : "next run")

/**
 * A size as a download review says it: whole units below a gigabyte ("312
 * MB"), one digit from there ("1.5 GB", "3 GB"), so a big download is never
 * rounded down by half a unit.
 */
export function downloadSize(bytes: number): string {
  if (bytes < 1024 ** 3) return formatBytes(bytes, 0)
  return formatBytes(bytes, 1).replace(".0 ", " ")
}

/**
 * What a review says of the large files a plugin names, none when it names
 * none: how much its first run still downloads and from where, and that
 * AutoScene checks each file by its hash. When AutoScene keeps them all
 * already (an update that names the same files), only where they come from.
 * An update's plugin has run before: it downloads at its next run.
 */
export function resourceLine(
  r: Pick<PluginReview, "resourceBytes" | "resourceHosts">,
  runtime: string,
  update = false,
): string | null {
  const bytes = r.resourceBytes ?? 0
  const from = r.resourceHosts.length > 0 ? ` from ${joinWords(r.resourceHosts)}` : ""
  if (bytes > 0) return `It downloads ${downloadSize(bytes)}${from} at its ${update ? nextTime(runtime) : firstTime(runtime)}. AutoScene keeps the files in its cache and checks each one by its hash.`
  if (r.resourceHosts.length > 0) return `The large files it names come${from}. AutoScene already has them, and checks each one by its hash.`
  return null
}

/** Whether AutoScene stands behind what a plugin says of itself, its author's name too: only its review does. */
export function reviewedPackage(p: Pick<PluginInfo, "trust" | "source">): boolean {
  return p.source === "package" && p.trust === "reviewed"
}

/**
 * The menu entry that opens a plugin's source, none when its plugin.json
 * names none. Only a reviewed package's is its source: AutoScene's review
 * signed that plugin.json. Of any other, nothing shows that its code came
 * from there, as "Says it is by" says of its author.
 */
export function sourceLabel(p: Pick<PluginInfo, "trust" | "source" | "repository">): string | null {
  if (!p.repository) return null
  return reviewedPackage(p) ? "Show its source" : "Show the source it names"
}

/**
 * What a plugin's picture may be: its icon (PNG or SVG) or its banner (PNG,
 * JPEG or WebP) as a base64 `data:` URL. The backend reads and checks the file.
 */
const PICTURE_URL = /^data:image\/(?:png|jpeg|webp|svg\+xml);base64,[A-Za-z0-9+/]+={0,2}$/

/** `src` while it is a plugin's picture in hand, never a link: else null. */
export function pluginPicture(src: string | null | undefined): string | null {
  return src != null && PICTURE_URL.test(src) ? src : null
}

/** A plugin package by its name: `.asp`, in any case. */
export const isPluginPackage = (path: string) => path.toLowerCase().endsWith(".asp")

/** From how many installed plugins Plugins offers to find one, as Cast does from six characters. */
export const FIND_FROM = 6

/**
 * Whether `p` is a plugin `query` looks for: each of its words is in its
 * name, what it says it does, its author or its id, whatever the case. An
 * empty query finds every plugin. A catalog entry is found the same way.
 */
export function pluginMatches(p: Pick<PluginInfo, "name" | "description" | "author" | "id">, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  if (words.length === 0) return true
  const text = [p.name, p.description, p.author, p.id].join("\n").toLowerCase()
  return words.every((w) => text.includes(w))
}

const PERMISSIONS = new Map<Permission, string>([
  ["exports:read", "Reads the exported clips"],
  ["exports:write", "Changes or adds files beside the exported clips"],
  ["project:read", "Reads the project: its episodes and characters"],
  ["network", "Talks to the internet"],
])

/** A permission a plugin declares, in plain words. */
export function permissionLabel(p: Permission): string {
  return PERMISSIONS.get(p) ?? p
}

/**
 * When AutoScene holds a plugin to its permissions: each time its pages
 * ask, and each time its program asks AutoScene for something (protocol 2).
 * None for a program that does neither: they are only what it says it does.
 */
export function checkedWhen(p: Pick<PluginInfo, "runtime" | "pages" | "protocol">): string | null {
  if (p.runtime === "none") return "AutoScene checks these each time its pages ask."
  const pages = p.pages.length > 0
  const calls = p.protocol >= 2
  if (pages && calls) return "AutoScene checks these each time its pages or its program ask it for something."
  if (pages) return "AutoScene checks these each time its pages ask."
  if (calls) return "AutoScene checks these each time its program asks it for something."
  return null
}

/** An entry a plugin adds to each export's menu. */
export type ExportActionItem = {
  key: string
  pluginId: string
  pluginName: string
  icon: string | null
  actionId: string
  title: string
}

/** The export actions of the plugins that may run now, plugin by plugin, in the order each lists them. */
export function exportActionItems(plugins: PluginInfo[] | undefined): ExportActionItem[] {
  return (plugins ?? [])
    .filter((p) => p.active)
    .flatMap((p) =>
      p.exportActions.map((a) => ({
        key: `${p.id}.${a.id}`,
        pluginId: p.id,
        pluginName: p.name,
        icon: p.icon,
        actionId: a.id,
        title: a.title,
      })),
    )
}

/** Whether the rail's Plugins tile is lit at `path`: a plugin's settings are under Plugins. */
export const pluginsLit = (path: string) => path === "/plugins" || path.startsWith("/plugins/")
