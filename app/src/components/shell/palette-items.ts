import type { Cast, MediaItem, PluginInfo } from "@/bindings"
import { pageLink, pagesFor } from "@/features/plugins/page-items"
import { formatDuration } from "@/lib/format"
import { fmt, i18n, t } from "@/lib/i18n"
import { fileStem, mediaTitle } from "@/lib/media"

/** What the palette finds in the open project: its characters and episodes. */
export type ProjectEntry = {
  id: string
  label: string
  hint: string
  /** More words it is found by (episode numbers written other ways, the file). */
  keywords: string[]
  picture: string | null
}

/** Asks for the English of a palette word: another language's search still finds what English words name. */
const EN = { lng: "en" } as const
const speaksEnglish = () => i18n.language === "en"

/** The cast as listed (the most on screen first), hidden characters left out. */
export function characterEntries(cast: Cast | undefined): ProjectEntry[] {
  return (cast?.characters ?? [])
    .filter((c) => !c.hidden)
    .map((c) => ({
      id: c.id,
      // Found by the name it is shown by, and by the English "Character 7" in another language.
      label: c.name ?? t("shell:palette.unnamed", { number: c.number }),
      hint: t("shell:palette.character", { time: fmt.screenTime(c.screenSec ?? 0), count: c.shots }),
      keywords: c.name
        ? []
        : [
            t("shell:palette.unnamedWord"),
            ...(speaksEnglish() ? [] : [t("shell:palette.unnamedWord", EN), t("shell:palette.unnamed", { number: c.number, ...EN })]),
          ],
      picture: c.coverSmall,
    }))
}

/** Episodes in order, found by "S01E03", "s1e3", "1x03", "E03" or their file. */
export function episodeEntries(media: MediaItem[] | undefined): ProjectEntry[] {
  return (media ?? []).map((m) => ({
    id: m.id,
    label: mediaTitle(m),
    hint: m.status === "missing" ? t("shell:palette.fileMissing") : m.durationSec ? formatDuration(m.durationSec) : "",
    keywords: episodeKeywords(m),
    picture: m.posterSmall ?? m.poster,
  }))
}

export function episodeKeywords(m: Pick<MediaItem, "season" | "episode" | "fileName">): string[] {
  const words = [fileStem(m.fileName)]
  const { season: s, episode: e } = m
  if (e != null) {
    const ee = String(e).padStart(2, "0")
    words.push(`E${ee}`, t("shell:palette.episodeWord", { number: e }))
    if (!speaksEnglish()) words.push(t("shell:palette.episodeWord", { number: e, ...EN }))
    if (s != null) {
      const ss = String(s).padStart(2, "0")
      words.push(`S${ss}E${ee}`, `S${s}E${e}`, `${s}x${ee}`)
    }
  }
  return words
}

/** A plugin's command, as the palette lists it. */
export type PluginCommandEntry = {
  id: string
  pluginId: string
  actionId: string
  label: string
  hint: string
  keywords: string[]
  icon: string | null
}

/**
 * The commands of the plugins that may run now, plugin by plugin; one that
 * needs a project only while one is open (`pid`).
 */
export function pluginCommandEntries(plugins: PluginInfo[] | undefined, pid: string | undefined): PluginCommandEntry[] {
  return (plugins ?? [])
    .filter((p) => p.active)
    .flatMap((p) =>
      p.commands
        .filter((c) => !c.needsProject || pid != null)
        .map((c) => ({
          id: `plugin-${p.id}.${c.id}`,
          pluginId: p.id,
          actionId: c.id,
          label: c.title,
          hint: p.name,
          keywords: [p.name],
          icon: p.icon,
        })),
    )
}

/** A plugin's page, as the palette lists it. */
export type PluginPageEntry = {
  id: string
  label: string
  hint: string
  keywords: string[]
  icon: string | null
  /** Where it opens. */
  to: string
}

/**
 * The pages of the plugins that may run now: each global page, and while a
 * project is open (`pid`) each project page, in that project.
 */
export function pluginPageEntries(plugins: PluginInfo[] | undefined, pid: string | undefined): PluginPageEntry[] {
  const pages = [...pagesFor(plugins, "global"), ...(pid != null ? pagesFor(plugins, "project") : [])]
  return pages.map((pg) => ({
    id: `plugin-page-${pg.key}`,
    label: t("shell:palette.openPage", { title: pg.title }),
    hint: pg.pluginName,
    keywords: [pg.pluginName, pg.title],
    icon: pg.icon,
    to: pageLink(pg, pg.scope === "project" ? pid : undefined),
  }))
}
