/**
 * How a progress bar follows the values it is given (pure; `progress.tsx`
 * keeps one `Motion` per bar). A stream of values glides linearly at the
 * pace the values come; a lower value or the first one is shown at once; a
 * value that stands alone eases in.
 */

/** Changes further apart than this stand alone (eased); closer, they are a stream (glided). */
export const STREAM_GAP = 2000
/** The shortest glide: just over the 250 ms the backend sends progress at. */
export const MIN_GLIDE = 300
/** `--dur-slow`: how long a change that stands alone eases. */
export const DUR_SLOW = 320
/** A glide lasts this much longer than the gap it is timed from, so it doesn't end before the next value. */
const MARGIN = 1.2
/** The gaps a glide is timed from: the middle one of the last three, so one late value doesn't slow the next few. */
const KEEP = 3

export type Motion = {
  /** 0..1 shown; `null`: how far isn't known (the sweep). */
  value: number | null
  /** Where the fill stands: the last value, kept under a sweep so the fill fades out where it was. */
  pos: number
  /** When `value` last changed (`performance.now()`). */
  at: number
  /** The last gaps of the stream, newest last; empty when not in one. */
  gaps: number[]
  /** How long the move to `value` takes; 0: at once. */
  ms: number
  ease: "linear" | "out"
  /** Has the bar shown the sweep? Its animation exists from then on (paused under a value), never before. */
  swept: boolean
}

/** A finite value clamped to 0..1; anything else (undefined, null, NaN, ±Infinity) is unknown. */
export function normalize(raw: number | null | undefined): number | null {
  return raw == null || !Number.isFinite(raw) ? null : Math.min(1, Math.max(0, raw))
}

/** The motion of a bar that starts at `raw`. */
export function start(raw: number | null | undefined, now: number): Motion {
  const value = normalize(raw)
  return { value, pos: value ?? 0, at: now, gaps: [], ms: 0, ease: "linear", swept: value == null }
}

function middle(gaps: number[]): number {
  const s = [...gaps].sort((a, b) => a - b)
  const n = s.length
  return n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2
}

/** The motion after the bar is given `raw` at `now`. The same value returns `m` itself. */
export function follow(m: Motion, raw: number | null | undefined, now: number): Motion {
  const next = normalize(raw)
  if (next === m.value) return m
  // Unknown: the fill fades out where it stands.
  if (next == null) return { ...m, value: null, gaps: [], ms: 0, swept: true }
  // The first value, one after a sweep, or a lower one (a new stage): in place, at once.
  if (m.value == null || next < m.value) return { ...m, value: next, pos: next, at: now, gaps: [], ms: 0, ease: "linear" }
  const since = now - m.at
  if (since > STREAM_GAP) return { ...m, value: next, pos: next, at: now, gaps: [], ms: DUR_SLOW, ease: "out" }
  const gaps = [...m.gaps, since].slice(-KEEP)
  const ms = Math.min(STREAM_GAP, Math.max(MIN_GLIDE, Math.round(middle(gaps) * MARGIN)))
  return { ...m, value: next, pos: next, at: now, gaps, ms, ease: "linear" }
}
