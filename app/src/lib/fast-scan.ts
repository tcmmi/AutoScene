import type { ContentKind, GraphicsClass, Platform, ScanDefaults, ThisComputer } from "@/bindings"

/*
 * Every sentence about Fast Scan's default for new projects (design-i52 §5.5),
 * so Settings, the New Project dialog and Edit details say the same thing. No
 * engine names, no numbers, no "Recommended".
 */

/** Whether a new project of this footage starts with Fast Scan here. */
export const startsFast = (d: ScanDefaults, content: ContentKind) => d.footage.find((f) => f.content === content)?.fast ?? false

/** Whether a new live-action project starts with Fast Scan. */
const liveFast = (d: ScanDefaults) => startsFast(d, "live")

/**
 * Settings → General → Fast Scan: what the chosen default means on this
 * computer. It names no device, so it stays true whichever card is chosen.
 */
export function settingsLine(d: ScanDefaults, platform: Platform): string {
  if (d.choice === "fast") return "Every new project starts with Fast Scan, 3D animation too."
  if (d.choice === "full") return "Every new project starts with the full scan."
  const here = platform === "macos" ? "this Mac" : "this computer"
  // On Automatic, live action keeps the full scan only while people without a visible face are found.
  const keeps = liveFast(d)
    ? " 3D animation keeps the full scan."
    : " 3D animation keeps the full scan, and so does live action while Find characters without a visible face is on."
  switch (d.computer.class) {
    case "integrated":
      return `Here, Automatic means Fast Scan: ${here} analyses video on its built-in graphics.${keeps}`
    case "notForAi":
      return `Here, Automatic means Fast Scan: this computer's graphics card has no hardware made for AI work.${keeps}`
    case "processor":
      return `Here, Automatic means Fast Scan: AutoScene analyses video on ${here}'s processor.${keeps}`
    case "forAi":
      return "Here, Automatic means the full scan: this computer's graphics card is built for AI work."
    case "unknown":
      return "Here, Automatic means the full scan: AutoScene couldn't recognise this computer's graphics."
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
      return "Much faster, finds fewer characters."
    case "animation":
      return findBodies
        ? "Much faster, but misses many animated faces and anyone without a visible face."
        : "Much faster, but misses many animated faces."
    default:
      return findBodies ? "Much faster, finds fewer faces and no one without a visible face." : "Much faster, finds fewer faces."
  }
}

/** What Fast Scan suits, for the classes that get it on Automatic. */
const SUITED: Partial<Record<GraphicsClass, string>> = {
  integrated: "this computer's built-in graphics",
  notForAi: "this computer's graphics card",
  processor: "analysis on the processor",
}

/** Why Fast Scan is on for a new project: what it suits. Empty where Automatic keeps the full scan. */
export function suits(c: GraphicsClass): string {
  const suited = SUITED[c]
  return suited ? `Suits ${suited}.` : ""
}

/**
 * Whether this footage starts with Fast Scan because of this computer's
 * graphics: on Automatic, where the person chose nothing.
 */
export function showsSuits(content: ContentKind, d: ScanDefaults): boolean {
  return d.choice === "auto" && startsFast(d, content) && SUITED[d.computer.class] != null
}

/**
 * The New Project dialog's Fast Scan line: what it costs and, where it is on
 * because of this computer's graphics, why. Without the defaults (they
 * failed), only what it costs.
 */
export function newProjectHint(content: ContentKind, findBodies: boolean, d: ScanDefaults | undefined): string {
  const cost = what(content, findBodies)
  return d && showsSuits(content, d) ? `${cost} ${suits(d.computer.class)}` : cost
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
    if (rescanned) return "Characters are found again this way, then regrouped. Your names and decisions stay."
    if (fast) return "Characters are found with the anime models in only a few frames of each shot: much faster, less sure who is who."
    return withSuits("Characters are found with the anime models every few frames.", content, d)
  }
  if (rescanned) return "Faces are found again this way, then the characters regrouped. Your names and decisions stay."
  if (fast) return "Faces are found the way the Scener app does: much faster, less sure who is who."
  return withSuits("Faces are found in full: every few frames, with the larger face models (for non-commercial use only).", content, d)
}

const withSuits = (full: string, content: ContentKind, d: ScanDefaults | undefined) =>
  d && showsSuits(content, d) ? `${full} Fast Scan suits ${SUITED[d.computer.class]}.` : full

// -- the first-start notice ----------------------------------------------------

/** The graphics the analysis runs on, as the notice names them. */
export function deviceLabel(c: ThisComputer, platform: Platform): string {
  if (c.name) return c.name
  if (c.class === "processor") return "Processor"
  return platform === "macos" ? "This Mac" : "Built-in graphics"
}

const HEAVY = "so new projects start with Fast Scan."

/** Why new projects start with Fast Scan here, said of what the analysis runs on. Empty where they don't. */
export function noticeLead(c: ThisComputer, platform: Platform): string {
  const here = platform === "macos" ? "This Mac" : "This computer"
  switch (c.class) {
    case "integrated":
      return `${here} analyses video on its built-in graphics. A full scan is heavy work for them, ${HEAVY}`
    case "notForAi":
      return `This graphics card has no hardware made for AI work. A full scan is heavy work for it, ${HEAVY}`
    case "processor":
      if (platform === "linux") return `On Linux, AutoScene analyses video on the processor. A full scan is heavy work for a processor, ${HEAVY}`
      if (platform === "macos") return `This Mac analyses video on its processor. A full scan is heavy work for a processor, ${HEAVY}`
      return `This computer has no graphics card AutoScene can use. A full scan is heavy work for a processor, ${HEAVY}`
    default:
      return ""
  }
}

/** What Fast Scan gives, and what it costs. */
export const NOTICE_FACTS = [
  { kind: "faster", title: "Much faster", body: "It looks at only a few frames of each shot." },
  { kind: "faces", title: "Less sure who is who", body: "It finds fewer faces, so characters get mixed up more often." },
] as const

/** The notice's foot: what keeps the full scan, and where the default changes. */
export function noticeFoot(d: ScanDefaults): string {
  const keeps = liveFast(d)
    ? "3D animation keeps the full scan."
    : "3D animation keeps the full scan, and so does live action while characters without a visible face are found."
  return `${keeps} You can change the default any time in Settings.`
}
