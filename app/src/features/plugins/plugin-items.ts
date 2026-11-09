import type { Permission, PluginInfo, PluginReview, PluginSandbox, PluginUpdate, SettingSpec, SourceSpec, Trust } from "@/bindings"
import { fmt, t } from "@/lib/i18n"

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
  if (p.withdrawn != null) return { label: t("plugins:trust.withdrawn"), tone: "danger" }
  if (p.source === "folder") return { label: t("plugins:trust.folder"), tone: "outline" }
  return trustOf(p.trust)
}

function trustOf(trust: Trust): { label: string; tone: "neutral" | "danger" | "outline" } {
  switch (trust) {
    case "reviewed":
      return { label: t("plugins:trust.reviewed"), tone: "neutral" }
    case "pinned":
      return { label: t("plugins:trust.pinned"), tone: "outline" }
    case "unreviewed":
      return { label: t("plugins:trust.unreviewed"), tone: "danger" }
  }
}

/**
 * What a withdrawn plugin's banner says under its title: the catalog's
 * `reason` (AutoScene's words, as a sentence), then what that means.
 */
export function withdrawnLine(reason: string): string {
  const said = reason.trim()
  return t("plugins:withdrawn.line", { reason: `${said}${/[.!?]$/.test(said) ? "" : "."}` })
}

/**
 * What a plugin's trust means, in one or two sentences for its review. A
 * signed update of a signed plugin used its pinned key (another is
 * refused); a first signature only brings a key along.
 */
export function trustLine(p: Pick<PluginInfo, "trust" | "source">, update?: Pick<PluginUpdate, "trustBefore" | "sourceBefore"> | null): string {
  if (p.source === "folder") return t("plugins:trustLine.folder")
  switch (p.trust) {
    case "reviewed":
      return t("plugins:trustLine.reviewed")
    case "pinned":
      return update?.trustBefore === "pinned" && update.sourceBefore === "package"
        ? t("plugins:trustLine.pinnedSame")
        : t("plugins:trustLine.pinnedNew")
    case "unreviewed":
      return t("plugins:trustLine.unreviewed")
  }
}

/** "a", "a and b", "a, b and c" (in the UI's language). */
export function joinWords(items: string[]): string {
  return fmt.list(items)
}

/**
 * A size as a download review says it: whole units below a gigabyte ("312
 * MB"), one digit from there ("1.5 GB", "3 GB"), so a big download is never
 * rounded down by half a unit.
 */
export function downloadSize(bytes: number): string {
  if (bytes < 1024 ** 3) return fmt.bytes(bytes, 0)
  const unit = bytes < 1024 ** 4 ? ("gb" as const) : ("tb" as const)
  return t(`common:format.bytes.${unit}`, { value: fmt.decimal(bytes / 1024 ** (unit === "gb" ? 3 : 4), 1, true) })
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
  const hosts = joinWords(r.resourceHosts)
  const from = r.resourceHosts.length > 0
  const service = runtime === "service"
  if (bytes > 0) {
    const v = { size: downloadSize(bytes), hosts }
    if (update) {
      if (service) return from ? t("plugins:resource.nextStartFrom", v) : t("plugins:resource.nextStart", v)
      return from ? t("plugins:resource.nextRunFrom", v) : t("plugins:resource.nextRun", v)
    }
    if (service) return from ? t("plugins:resource.firstStartFrom", v) : t("plugins:resource.firstStart", v)
    return from ? t("plugins:resource.firstRunFrom", v) : t("plugins:resource.firstRun", v)
  }
  if (from) return t("plugins:resource.kept", { hosts })
  return null
}

/** Whether AutoScene stands behind what a plugin says of itself, its author's name too: only its review does. */
export function reviewedPackage(p: Pick<PluginInfo, "trust" | "source">): boolean {
  return p.source === "package" && p.trust === "reviewed"
}

/** Who a plugin is by, as its card and its screen say it: "By" for a reviewed package, else what it says of itself. None without an author. */
export function authorLine(p: Pick<PluginInfo, "trust" | "source" | "author">): string | null {
  if (!p.author) return null
  return reviewedPackage(p) ? t("plugins:by", { author: p.author }) : t("plugins:saysBy", { author: p.author })
}

/**
 * The menu entry that opens a plugin's source, none when its plugin.json
 * names none. Only a reviewed package's is its source: AutoScene's review
 * signed that plugin.json. Of any other, nothing shows that its code came
 * from there, as "Says it is by" says of its author.
 */
export function sourceLabel(p: Pick<PluginInfo, "trust" | "source" | "repository">): string | null {
  if (!p.repository) return null
  return reviewedPackage(p) ? t("plugins:menu.showSource") : t("plugins:menu.showNamedSource")
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

/** What a program says it does with a permission. */
function says(p: Permission): string | undefined {
  switch (p) {
    case "exports:read":
      return t("plugins:permission.says.exportsRead")
    case "exports:write":
      return t("plugins:permission.says.exportsWrite")
    case "project:read":
      return t("plugins:permission.says.projectRead")
    case "network":
      return t("plugins:permission.says.network")
  }
}

/** What a permission lets a plugin in AutoScene's sandbox do: a wall, not a promise. */
function walls(p: Permission): string | undefined {
  switch (p) {
    case "exports:read":
      return t("plugins:permission.walled.exportsRead")
    case "exports:write":
      return t("plugins:permission.walled.exportsWrite")
    case "project:read":
      return t("plugins:permission.walled.projectRead")
    case "network":
      return t("plugins:permission.walled.network")
  }
}

/**
 * A permission a plugin declares, in plain words: what it says it does, or,
 * for a plugin that runs in AutoScene's sandbox (`sandbox`, with the sites
 * it may reach), what the sandbox lets it do. The network of a plugin in the
 * sandbox that names no site may reach none.
 */
export function permissionLabel(p: Permission, sandbox?: { hosts: readonly string[] }): string {
  if (!sandbox) return says(p) ?? p
  if (p === "network" && sandbox.hosts.length === 0) return t("plugins:permission.walled.networkNone")
  return walls(p) ?? p
}

/**
 * The limits of a plugin in AutoScene's sandbox, in one line: "It may use up
 * to 512 MB of memory. A run stops after 30 minutes." Whole hours are said
 * as hours.
 */
export function limitsLine(s: PluginSandbox): string {
  const memory = s.memoryMb
  if (s.minutes % 60 === 0) {
    const n = s.minutes / 60
    return t("plugins:limits.hours", { count: n, n, memory })
  }
  return t("plugins:limits.minutes", { count: s.minutes, n: s.minutes, memory })
}

/**
 * When AutoScene holds a plugin to its permissions: always for one in its
 * sandbox, each time the pages of another ask, and each time its program
 * asks AutoScene for something (protocol 2). None for a program that does
 * neither: they are only what it says it does.
 */
export function checkedWhen(p: Pick<PluginInfo, "runtime" | "pages" | "protocol">): string | null {
  if (p.runtime === "wasm") return t("plugins:checked.sandbox")
  if (p.runtime === "none") return t("plugins:checked.pages")
  const pages = p.pages.length > 0
  const calls = p.protocol >= 2
  if (pages && calls) return t("plugins:checked.pagesAndProgram")
  if (pages) return t("plugins:checked.pages")
  if (calls) return t("plugins:checked.program")
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

/** An entry a plugin adds to a scenepack's ⋯ menu: the same shape as an export's. */
export type PackActionItem = ExportActionItem

/** The pack actions of the plugins that may run now, plugin by plugin, in the order each lists them. */
export function packActionItems(plugins: PluginInfo[] | undefined): PackActionItem[] {
  return (plugins ?? [])
    .filter((p) => p.active)
    .flatMap((p) =>
      p.packActions.map((a) => ({
        key: `${p.id}.${a.id}`,
        pluginId: p.id,
        pluginName: p.name,
        icon: p.icon,
        actionId: a.id,
        title: a.title,
      })),
    )
}

/** A source a plugin adds: links of its hosts become downloads into a project. */
export type SourceItem = {
  key: string
  pluginId: string
  pluginName: string
  icon: string | null
  source: SourceSpec
  /** The settings it shows beside the links, in its order. */
  settings: SettingSpec[]
}

/** The sources of the plugins that may run now, plugin by plugin, in the order each lists them. */
export function sourceItems(plugins: PluginInfo[] | undefined): SourceItem[] {
  return (plugins ?? [])
    .filter((p) => p.active)
    .flatMap((p) =>
      p.sources.map((s) => ({
        key: `${p.id}.${s.id}`,
        pluginId: p.id,
        pluginName: p.name,
        icon: p.icon,
        source: s,
        settings: s.settings.flatMap((k) => p.settings.filter((x) => x.key === k)),
      })),
    )
}

/** Whether the rail's Plugins tile is lit at `path`: a plugin's settings are under Plugins. */
export const pluginsLit = (path: string) => path === "/plugins" || path.startsWith("/plugins/")
