// Numbers, sizes and dates in the UI language (lib/i18n.ts `fmt`): dates
// follow the interface's language, not the OS region, so English reads
// "13 Oct, 22:13" instead of mixing languages.
import { fmt } from "@/lib/i18n"

/** 1536 → "1.5 KB"; nothing → "0 B". */
export function formatBytes(bytes: number, digits = 1): string {
  return fmt.bytes(bytes, digits)
}

/** 3725.4 → "1:02:05"; 65 → "1:05". */
export function formatDuration(sec: number): string {
  return fmt.duration(sec)
}

/** Unix ms → "13 Oct, 19:42". */
export function formatDateTime(ms: number): string {
  return fmt.dateTime(ms)
}

/** 14200 → "14,200". */
export function formatCount(n: number): string {
  return fmt.count(n)
}

/**
 * A measured pace (frames a second, times real time) to the digits it holds:
 * a tenth below 10, whole up to 1,000, then three figures, as a pace measured
 * over a few seconds wavers by about a percent. 7.46 → "7.5", 612.4 → "612",
 * 2853 → "2,850".
 */
export function formatPace(v: number): string {
  const tenths = Math.round(v * 10) / 10
  if (tenths < 10) return fmt.decimal(tenths, 1)
  return formatCount(v < 1000 ? v : Number(v.toPrecision(3)))
}

/** 0.4 → "40%". */
export function formatPercent(v: number | null | undefined): string {
  return fmt.percent(v)
}

/** "H.264 · Mobile (CapCut, Alight Motion)" → "H.264 · Mobile": an export format's name without what its brackets add. */
export function shortFormatName(name: string): string {
  return name.replace(/\s*\([^)]*\)\s*$/, "") || name
}

/** Remaining time for progress lines: "about 3 min left". */
export function formatEta(ms: number | null | undefined): string {
  return fmt.eta(ms)
}

/** A job's remaining time and progress, each when known: "about 3 min left · 40%". */
export function formatProgress(progress: number | null | undefined, etaMs: number | null | undefined): string {
  return [formatEta(etaMs), formatPercent(progress)].filter(Boolean).join(" · ")
}
