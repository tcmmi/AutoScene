import type { Character, Segment } from "@/bindings"
import { fmt, i18n, t } from "@/lib/i18n"
import type { TimecodeDisplayMode } from "./timecode-mode"

// The player's words that are made outside a component's own `t`: the
// character's name, the stretches' names, and the words of what draws again
// with every video frame, which are said once per language, never per frame
// (docs/DESIGN.md, Copy: no `t()` in per-frame paths).

const caches = new Set<() => void>()
const clearCaches = () => {
  for (const clear of caches) clear()
}
// Also when words are added or removed for a language (a bundle that loads
// after the switch, a hot reload of a catalog), so no stale word stays.
i18n.on("languageChanged", clearCaches)
i18n.store.on("added", clearCaches)
i18n.store.on("removed", clearCaches)

/**
 * `make`'s words, made on first use and again after the language changes.
 * What a component reads while the episode plays comes from here, so playing
 * calls no `t()`.
 */
export function perLanguage<T>(make: () => T): () => T {
  let made: { value: T } | null = null
  caches.add(() => (made = null))
  return () => (made ??= { value: make() }).value
}

const HOLE = "\u0000"

/**
 * A message with one value that changes with every frame: said once with a
 * hole in its place, filled by string joining after that, so the language's
 * word order stays and `t()` isn't called per frame.
 */
export function withHole(say: (hole: string) => string): (value: string) => string {
  const parts = say(HOLE).split(HOLE)
  // A message that holds the value twice or not at all can't be filled by
  // joining: fail where a catalog is tested, keep going otherwise.
  if (parts.length !== 2 && import.meta.env.DEV) throw new Error(`a per-frame message needs {{value}} exactly once: ${JSON.stringify(parts)}`)
  const [before = "", after = ""] = parts
  return (value) => before + value + after
}

/**
 * The notice in the export dialog when only one mark is set: the clip then
 * runs to the episode's edge. `keyFor` gives the key that marks an end
 * (slot 0 = in, 1 = out), or null when it has none.
 */
export function oneSidedNotice(inFrame: number | null, outFrame: number | null, keyFor: (slot: number) => string | null): string | undefined {
  if (inFrame != null && outFrame == null) {
    const keys = keyFor(1)
    return keys != null ? t("player:clip.onlyIn", { keys }) : t("player:clip.onlyInNoKey")
  }
  if (outFrame != null && inFrame == null) {
    const keys = keyFor(0)
    return keys != null ? t("player:clip.onlyOut", { keys }) : t("player:clip.onlyOutNoKey")
  }
  return undefined
}

/** The character's name, or "Character 12" in the UI's language (the app sends the number, not the words). */
export function characterName(c: Pick<Character, "name" | "number">): string {
  return c.name ?? t("player:character.unnamed", { number: c.number })
}

/** The name of a kind of stretch on the timeline. */
export function segmentLabel(kind: Segment["kind"]): string {
  switch (kind) {
    case "shot":
      return t("player:segment.shot")
    case "dissolve":
      return t("player:segment.dissolve")
    case "fade_out":
      return t("player:segment.fadeOut")
    case "fade_in":
      return t("player:segment.fadeIn")
    case "black":
      return t("player:segment.black")
    case "flash":
      return t("player:segment.flash")
  }
}

const percentWord = perLanguage(() => withHole((value) => t("player:percent", { value })))

/** How far into the episode, 0 to 100: "12.3 %". */
export function percentText(p: number): string {
  return percentWord()(fmt.decimal(p, 1))
}

/** The words of the timecode readout, which draws on every frame. */
export const readoutWords = perLanguage(() => {
  const menu: Record<TimecodeDisplayMode, string> = {
    elapsed: t("player:readout.elapsed"),
    remaining: t("player:readout.remaining"),
    frames: t("player:readout.frameNumber"),
    percent: t("player:readout.percentage"),
  }
  const named: Record<TimecodeDisplayMode, string> = { ...menu, frames: t("player:readout.currentFrame") }
  const aria = (mode: TimecodeDisplayMode) => withHole((value) => t("player:readout.aria", { label: named[mode], value }))
  return {
    /** The four modes, as the menu names them. */
    menu,
    /** The readout's name for a screen reader, given what it shows. */
    aria: { elapsed: aria("elapsed"), remaining: aria("remaining"), frames: aria("frames"), percent: aria("percent") },
    goTo: t("player:readout.goTo"),
    goToMenu: t("player:readout.goToMenu"),
    goToHint: t("player:readout.goToHint"),
    counterTip: t("player:readout.counterTip"),
    frameUnit: t("player:readout.frameUnit"),
  }
})

/** The words of the timeline's playhead and overview, which draw on every frame. */
export const timelineWords = perLanguage(() => ({
  playhead: t("player:timeline.playhead"),
  inView: t("player:timeline.inView"),
  goHere: t("player:timeline.goHere"),
  markInHere: t("player:timeline.markInHere"),
  markOutHere: t("player:timeline.markOutHere"),
  copyTimecode: t("player:timeline.copyTimecode"),
}))
