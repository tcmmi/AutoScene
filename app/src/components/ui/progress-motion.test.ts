import { describe, expect, it } from "vitest"
import { DUR_SLOW, follow, type Motion, normalize, start } from "./progress-motion"

/** Feeds `values` to a bar that starts at `first`, `gap` ms (or the gaps listed) apart. */
function stream(first: number | null, values: (number | null)[], gap: number | number[]): Motion {
  let now = 0
  let m = start(first, now)
  values.forEach((v, i) => {
    now += Array.isArray(gap) ? gap[i] : gap
    m = follow(m, v, now)
  })
  return m
}

describe("progress motion", () => {
  it("shows the first value at once", () => {
    const m = start(0.4, 0)
    expect(m).toMatchObject({ value: 0.4, pos: 0.4, ms: 0, swept: false })
  })

  it("shows a value after a sweep at once, and keeps the fill where it stood under the sweep", () => {
    const swept = follow(start(0.3, 0), null, 300)
    expect(swept).toMatchObject({ value: null, pos: 0.3, ms: 0, swept: true })
    const back = follow(swept, 0.5, 600)
    expect(back).toMatchObject({ value: 0.5, pos: 0.5, ms: 0, swept: true })
    // Once it swept, its sweep exists for good (paused under a value), so it never restarts.
    expect(follow(back, 0.6, 900).swept).toBe(true)
  })

  it("a bar that starts unknown has swept, one that starts with a value hasn't", () => {
    expect(start(null, 0).swept).toBe(true)
    expect(start(undefined, 0).swept).toBe(true)
    expect(start(0, 0).swept).toBe(false)
  })

  it("glides linearly at the pace of its values: 360 ms at 300 ms gaps, 600 ms at 500 ms", () => {
    expect(stream(0, [0.1, 0.2, 0.3], 300)).toMatchObject({ ms: 360, ease: "linear" })
    expect(stream(0, [0.1, 0.2, 0.3], 500)).toMatchObject({ ms: 600, ease: "linear" })
  })

  it("never glides shorter than 300 ms, just over the backend's 250 ms", () => {
    expect(stream(0, [0.1, 0.2, 0.3], 100).ms).toBe(300)
  })

  it("one late value doesn't stretch the next glide", () => {
    expect(stream(0, [0.1, 0.2, 0.3], [500, 500, 1500]).ms).toBe(600)
    // Nor the one after it: the middle of the last three gaps.
    expect(stream(0, [0.1, 0.2, 0.3, 0.4], [500, 500, 1500, 500]).ms).toBe(600)
  })

  it("jumps to a lower value (a new stage) and times the next glide from there", () => {
    const down = stream(0, [0.5, 0.9, 0.1], 500)
    expect(down).toMatchObject({ value: 0.1, pos: 0.1, ms: 0, gaps: [] })
    // The gap before the jump doesn't count: the first glide after it is timed from its own gap.
    expect(follow(down, 0.2, 1500 + 300)).toMatchObject({ ms: 360, ease: "linear", gaps: [300] })
  })

  it("eases a value that stands alone, more than 2 s after the last", () => {
    const m = follow(start(0.2, 0), 0.4, 2001)
    expect(m).toMatchObject({ value: 0.4, ms: DUR_SLOW, ease: "out", gaps: [] })
    // And the next one starts a new stream.
    expect(follow(m, 0.5, 2001 + 500)).toMatchObject({ ms: 600, ease: "linear" })
  })

  it("returns the same object for the same value, so nothing re-renders or re-times", () => {
    const m = stream(0, [0.1, 0.2], 300)
    expect(follow(m, 0.2, 5000)).toBe(m)
    const unknown = start(null, 0)
    expect(follow(unknown, undefined, 100)).toBe(unknown)
    expect(follow(unknown, Number.NaN, 100)).toBe(unknown)
    // A value past the end is the end: 1.5 after 1 changes nothing.
    const full = start(1, 0)
    expect(follow(full, 1.5, 100)).toBe(full)
  })

  it("normalize: anything not finite is unknown, the rest is clamped to 0..1", () => {
    expect(normalize(undefined)).toBeNull()
    expect(normalize(null)).toBeNull()
    expect(normalize(Number.NaN)).toBeNull()
    expect(normalize(Number.POSITIVE_INFINITY)).toBeNull()
    expect(normalize(Number.NEGATIVE_INFINITY)).toBeNull()
    expect(normalize(-0.2)).toBe(0)
    expect(normalize(1.5)).toBe(1)
    expect(normalize(0.42)).toBe(0.42)
  })
})
