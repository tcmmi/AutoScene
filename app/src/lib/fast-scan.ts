import type { ContentKind, GraphicsClass, Platform, ScanDefaults, ThisComputer } from "@/bindings"
import { t } from "@/lib/i18n"

/*
 * Every sentence about Fast Scan's default for new projects (design-i52 §5.5),
 * so Settings, the New Project dialog and Edit details say the same thing. No
 * engine names, no numbers, no "Recommended".
 *
 * Each sentence is a whole message of the catalog (`library:fastScan`), one
 * per case: what changes with the computer (a Mac, a graphics card) is a
 * different sentence, never a word put into one. Where a line has two
 * sentences, each is its own message and they follow each other.
 */

/** The sentences of a line, one after the other. */
const sentences = (...parts: string[]) => parts.filter(Boolean).join(" ")

/** Whether a new project of this footage starts with Fast Scan here. */
export const startsFast = (d: ScanDefaults, content: ContentKind) => d.footage.find((f) => f.content === content)?.fast ?? false

/** Whether a new live-action project starts with Fast Scan. */
const liveFast = (d: ScanDefaults) => startsFast(d, "live")

/** The graphics classes that get Fast Scan on Automatic. */
const SUITED: ReadonlySet<GraphicsClass> = new Set<GraphicsClass>(["integrated", "notForAi", "processor"])

/**
 * Settings → General → Fast Scan: what the chosen default means on this
 * computer. It names no device, so it stays true whichever card is chosen.
 */
export function settingsLine(d: ScanDefaults, platform: Platform): string {
  if (d.choice === "fast") return t("library:fastScan.settings.fast")
  if (d.choice === "full") return t("library:fastScan.settings.full")
  const mac = platform === "macos"
  // On Automatic, live action keeps the full scan only while people without a visible face are found.
  const keeps = () => t(liveFast(d) ? "library:fastScan.settings.keeps" : "library:fastScan.settings.keepsWithBodies")
  switch (d.computer.class) {
    case "integrated":
      return sentences(t(mac ? "library:fastScan.settings.integratedMac" : "library:fastScan.settings.integrated"), keeps())
    case "notForAi":
      return sentences(t("library:fastScan.settings.notForAi"), keeps())
    case "processor":
      return sentences(t(mac ? "library:fastScan.settings.processorMac" : "library:fastScan.settings.processor"), keeps())
    case "forAi":
      return t("library:fastScan.settings.forAi")
    case "unknown":
      return t("library:fastScan.settings.unknown")
  }
}

/**
 * The New Project dialog's Fast Scan line: what it costs this footage. While
 * people without a visible face are found, it misses them too (it finds
 * faces only); anime is found by its own character models, faces or not.
 */
export function what(content: ContentKind, findBodies: boolean): string {
  switch (content) {
    case "anime":
      return t("library:fastScan.cost.anime")
    case "animation":
      return t(findBodies ? "library:fastScan.cost.animationBodies" : "library:fastScan.cost.animation")
    default:
      return t(findBodies ? "library:fastScan.cost.liveBodies" : "library:fastScan.cost.live")
  }
}

/** Why Fast Scan is on for a new project: what it suits. Empty where Automatic keeps the full scan. */
export function suits(c: GraphicsClass): string {
  switch (c) {
    case "integrated":
      return t("library:fastScan.suits.integrated")
    case "notForAi":
      return t("library:fastScan.suits.notForAi")
    case "processor":
      return t("library:fastScan.suits.processor")
    default:
      return ""
  }
}

/**
 * Whether this footage starts with Fast Scan because of this computer's
 * graphics: on Automatic, where the person chose nothing.
 */
export function showsSuits(content: ContentKind, d: ScanDefaults): boolean {
  return d.choice === "auto" && startsFast(d, content) && SUITED.has(d.computer.class)
}

/**
 * The New Project dialog's Fast Scan line: what it costs and, where it is on
 * because of this computer's graphics, why. Without the defaults (they
 * failed), only what it costs.
 */
export function newProjectHint(content: ContentKind, findBodies: boolean, d: ScanDefaults | undefined): string {
  const cost = what(content, findBodies)
  return d && showsSuits(content, d) ? sentences(cost, suits(d.computer.class)) : cost
}

/**
 * Edit details: what Fast Scan does to a project's footage (anime keeps its
 * character models, see `Project::faces_plan`). A project on the full scan
 * that a new one of its footage wouldn't be, because of this computer's
 * graphics, hears that Fast Scan suits them; said with its name, since the
 * sentence before it is about the full scan.
 */
export function fastScanHint(
  content: ContentKind,
  rescanned: boolean,
  fast: boolean,
  d: ScanDefaults | undefined,
): string {
  if (content === "anime") {
    if (rescanned) return t("library:fastScan.hint.animeRescanned")
    if (fast) return t("library:fastScan.hint.animeFast")
    return withSuits(t("library:fastScan.hint.animeFull"), content, d)
  }
  if (rescanned) return t("library:fastScan.hint.rescanned")
  if (fast) return t("library:fastScan.hint.fast")
  return withSuits(t("library:fastScan.hint.full"), content, d)
}

const withSuits = (full: string, content: ContentKind, d: ScanDefaults | undefined) => {
  if (!d || !showsSuits(content, d)) return full
  switch (d.computer.class) {
    case "integrated":
      return sentences(full, t("library:fastScan.fastScanSuits.integrated"))
    case "notForAi":
      return sentences(full, t("library:fastScan.fastScanSuits.notForAi"))
    default:
      return sentences(full, t("library:fastScan.fastScanSuits.processor"))
  }
}

// -- the first-start notice ----------------------------------------------------

/** The graphics the analysis runs on, as the notice names them. */
export function deviceLabel(c: ThisComputer, platform: Platform): string {
  if (c.name) return c.name
  if (c.class === "processor") return t("library:fastScan.device.processor")
  return t(platform === "macos" ? "library:fastScan.device.mac" : "library:fastScan.device.integrated")
}

/** Why new projects start with Fast Scan here, said of what the analysis runs on. Empty where they don't. */
export function noticeLead(c: ThisComputer, platform: Platform): string {
  switch (c.class) {
    case "integrated":
      return t(platform === "macos" ? "library:fastScan.lead.integratedMac" : "library:fastScan.lead.integrated")
    case "notForAi":
      return t("library:fastScan.lead.notForAi")
    case "processor":
      if (platform === "linux") return t("library:fastScan.lead.processorLinux")
      if (platform === "macos") return t("library:fastScan.lead.processorMac")
      return t("library:fastScan.lead.processor")
    default:
      return ""
  }
}

/**
 * What Fast Scan gives, and what it costs. The words are read when shown
 * (each `title` and `body` looks itself up), in the language in use.
 */
export const NOTICE_FACTS = [
  {
    kind: "faster",
    get title() {
      return t("library:fastScan.notice.faster.title")
    },
    get body() {
      return t("library:fastScan.notice.faster.body")
    },
  },
  {
    kind: "faces",
    get title() {
      return t("library:fastScan.notice.faces.title")
    },
    get body() {
      return t("library:fastScan.notice.faces.body")
    },
  },
] as const

/** The notice's foot: what keeps the full scan, and where the default changes. */
export function noticeFoot(d: ScanDefaults): string {
  const keeps = t(liveFast(d) ? "library:fastScan.notice.keeps" : "library:fastScan.notice.keepsWithBodies")
  return sentences(keeps, t("library:fastScan.notice.change"))
}
