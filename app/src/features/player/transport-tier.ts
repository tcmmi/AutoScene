import { type RefObject, useLayoutEffect, useState } from "react"
import { fmt } from "@/lib/i18n"
import type { TimecodeDisplayMode } from "./timecode-mode"
import { percentText, readoutWords } from "./words"

/**
 * How much the transport row under the picture shows, from 1 (everything)
 * to 4 (the least):
 *
 * | tier | volume        | readout | frame counter | In / Out      | length and ✕ | Export   |
 * |------|---------------|---------|---------------|---------------|--------------|----------|
 * | 1    | mute + slider | large   | ✓             | with timecode | ✓            | labelled |
 * | 2    | mute + slider | large   | –             | label         | ✓            | labelled |
 * | 3    | mute          | large   | –             | label         | ✓            | icon     |
 * | 4    | –             | small   | –             | label         | –            | icon     |
 *
 * What a tier hides stays reachable: the chips' tooltips and names, the
 * readout's Frame number mode, the Export tooltip, the keys and the menus.
 */
export type Tier = 1 | 2 | 3 | 4

/**
 * The narrowest row (px) each tier fits in. Measured from the widest strings
 * a 2-hour film shows (a remaining timecode, "−4×", both marks set), with
 * 16 to 19 px to spare on each side of Play.
 */
export const TIERS = { 1: 1292, 2: 960, 3: 808 } as const

/** The tier a row `width` px wide holds. */
export function tierFor(width: number): Tier {
  if (width >= TIERS[1]) return 1
  if (width >= TIERS[2]) return 2
  if (width >= TIERS[3]) return 3
  return 4
}

/** One tier less. */
export function coarser(tier: Tier): Tier {
  return tier >= 4 ? 4 : ((tier + 1) as Tier)
}

/**
 * The most characters the readout can show in `mode` for an episode whose
 * last frame is `last`: its width stays the same while the episode plays.
 */
export function readoutChars(mode: TimecodeDisplayMode, last: number, tc: (f: number) => string): number {
  switch (mode) {
    case "elapsed":
      return tc(last).length
    case "remaining":
      return tc(last).length + 1
    case "frames":
      return fmt.count(last).length
    case "percent":
      return percentText(100).length
  }
}

/** The most characters of the frame counter ("this frame / the last frame"; the frame unit after it in percent mode: " f"). */
export function counterChars(mode: TimecodeDisplayMode, last: number): number {
  return `${fmt.count(last)} / ${fmt.count(last)}`.length + (mode === "percent" ? 1 + readoutWords().frameUnit.length : 0)
}

/**
 * The tier for the row `rowRef` points at, from its width. If a side of the
 * row (`[data-side]`) still doesn't fit its content (a longer translation, a
 * fallback font), the tier goes one coarser after each layout, before it is
 * painted, down to 4. It starts again from the width only when the width or
 * one of `deps` changes, so it can't go back and forth.
 */
export function useTransportTier(rowRef: RefObject<HTMLElement | null>, deps: readonly unknown[]): Tier {
  const [width, setWidth] = useState<number | null>(null)
  const key = JSON.stringify([width, ...deps])
  const [guard, setGuard] = useState({ key: "", steps: 0 })
  const steps = guard.key === key ? guard.steps : 0
  const tier = width == null ? 1 : (Math.min(4, tierFor(width) + steps) as Tier)

  useLayoutEffect(() => {
    const row = rowRef.current
    if (!row) return
    const measure = () => setWidth(row.clientWidth)
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(row)
    return () => ro.disconnect()
  }, [rowRef])

  useLayoutEffect(() => {
    const row = rowRef.current
    if (!row || width == null || tier === 4) return
    const sides = row.querySelectorAll<HTMLElement>("[data-side]")
    if (Array.from(sides).some((side) => side.scrollWidth > side.clientWidth)) setGuard({ key, steps: steps + 1 })
  }, [rowRef, width, key, steps, tier])

  return tier
}
