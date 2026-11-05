import { act, cleanup, renderHook } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { timecode } from "@/lib/timecode"
import { inLanguage } from "@/test/i18n"
import { coarser, counterChars, labelExtra, readoutChars, tierFor, useTransportTier } from "./transport-tier"

describe("tierFor", () => {
  it("gives each width its tier, the edges included", () => {
    expect(tierFor(586)).toBe(4)
    expect(tierFor(807)).toBe(4)
    expect(tierFor(808)).toBe(3)
    expect(tierFor(959)).toBe(3)
    expect(tierFor(960)).toBe(2)
    expect(tierFor(1291)).toBe(2)
    expect(tierFor(1292)).toBe(1)
    expect(tierFor(1802)).toBe(1)
  })

  it("moves every edge by what longer words on the row need", () => {
    expect(tierFor(1292, 63)).toBe(2)
    expect(tierFor(1292 + 63, 63)).toBe(1)
    expect(tierFor(960 + 62, 63)).toBe(3)
    expect(tierFor(808 + 63, 63)).toBe(3)
    expect(tierFor(807 + 63, 63)).toBe(4)
  })

  it("goes one tier coarser and stops at 4", () => {
    expect(coarser(1)).toBe(2)
    expect(coarser(3)).toBe(4)
    expect(coarser(4)).toBe(4)
  })
})

describe("labelExtra", () => {
  it("is nothing in English, whose words the tiers were measured for", () => {
    expect(labelExtra()).toBe(0)
  })

  it("is what Intrare, Ieșire and Exportă add to In, Out and Export", async () => {
    await inLanguage("ro")
    // 7 + 6 + 7 letters against 2 + 3 + 6, at 9 px a letter.
    expect(labelExtra()).toBe(9 * 9)
  })

  it("is nothing for a language with shorter words", async () => {
    await inLanguage("ro", { player: { transport: { in: "I", out: "O", export: "E" } } })
    expect(labelExtra()).toBe(0)
  })
})

describe("the readout's and the counter's widths", () => {
  const tc = (f: number) => timecode(f, { num: 24, den: 1 }, false)

  it("is the longest string each mode can show", () => {
    for (const last of [585, 172_799]) {
      expect(readoutChars("elapsed", last, tc)).toBe(11)
      expect(readoutChars("remaining", last, tc)).toBe(12)
      expect(readoutChars("percent", last, tc)).toBe(7)
    }
    expect(readoutChars("frames", 585, tc)).toBe(3)
    expect(readoutChars("frames", 172_799, tc)).toBe("172,799".length)
  })

  it("counts this frame and the last one, and the f in percent mode", () => {
    expect(counterChars("elapsed", 585)).toBe("585 / 585".length)
    expect(counterChars("elapsed", 172_799)).toBe(17)
    expect(counterChars("remaining", 172_799)).toBe(17)
    expect(counterChars("percent", 172_799)).toBe(19)
  })
})

describe("useTransportTier", () => {
  /** What the row's observer is told when it changes size. */
  let resized: (() => void)[] = []
  let width = 0
  /** How many more times a side says it doesn't fit. */
  let overflows = 0
  let row: HTMLDivElement
  let ref: { current: HTMLDivElement }

  beforeEach(() => {
    resized = []
    overflows = 0
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(changed: () => void) {
          resized.push(changed)
        }
        observe() {}
        disconnect() {}
      },
    )
    row = document.createElement("div")
    Object.defineProperty(row, "clientWidth", { configurable: true, get: () => width })
    const side = document.createElement("div")
    side.dataset.side = "left"
    Object.defineProperty(side, "clientWidth", { configurable: true, get: () => 300 })
    Object.defineProperty(side, "scrollWidth", { configurable: true, get: () => (overflows-- > 0 ? 320 : 300) })
    row.append(side)
    ref = { current: row }
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
  })

  const resize = (w: number) =>
    act(() => {
      width = w
      for (const r of resized) r()
    })

  it("starts at the width's tier", () => {
    width = 1002
    const { result } = renderHook(() => useTransportTier(ref, [false]))
    expect(result.current).toBe(2)
    resize(1482)
    expect(result.current).toBe(1)
    resize(586)
    expect(result.current).toBe(4)
  })

  it("goes one tier coarser per layout while a side doesn't fit, then stays", () => {
    width = 1300
    overflows = 2
    const { result } = renderHook(() => useTransportTier(ref, [false]))
    expect(result.current).toBe(3)
  })

  it("stops at tier 4", () => {
    width = 1300
    overflows = 99
    const { result } = renderHook(() => useTransportTier(ref, [false]))
    expect(result.current).toBe(4)
    // Tier 4 has no sides to check.
    expect(overflows).toBe(96)
  })

  it("starts again from the width when the width or what the row shows changes", () => {
    width = 1300
    overflows = 1
    const { result, rerender } = renderHook(({ marks }) => useTransportTier(ref, [marks]), {
      initialProps: { marks: false },
    })
    expect(result.current).toBe(2)
    rerender({ marks: true })
    expect(result.current).toBe(1)
    overflows = 1
    resize(1000)
    expect(result.current).toBe(3)
    resize(1300)
    expect(result.current).toBe(1)
  })
})
