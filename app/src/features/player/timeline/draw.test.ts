import { cleanup, render } from "@testing-library/react"
import { createElement } from "react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { Presence } from "@/bindings"
import { SPAN_BAR, drawSpans } from "./draw"
import { TrackCanvas } from "./Timeline"
import { fit } from "./viewport"

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

/** A 2D context that only records the rounded rectangles drawn into it. */
function recorder() {
  const rects: { x: number; y: number; w: number; h: number }[] = []
  const ctx = {
    fillStyle: "",
    globalAlpha: 1,
    shadowColor: "",
    shadowBlur: 0,
    beginPath: () => {},
    fill: () => {},
    roundRect: (x: number, y: number, w: number, h: number) => rects.push({ x, y, w, h }),
    createPattern: () => null,
    setTransform: () => {},
    clearRect: () => {},
  }
  return { ctx: ctx as unknown as CanvasRenderingContext2D, rects }
}

describe("a character's lane", () => {
  it("draws an 8 px bar centred in a 20 px lane", () => {
    // The palette's hatch tile asks jsdom for a context it doesn't have.
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
    const { ctx, rects } = recorder()
    const spans: Presence[] = [
      { characterId: "c", trackId: 1, start: 10, end: 20 },
      { characterId: "c", trackId: 2, start: 40, end: 60, body: true },
    ]
    drawSpans(ctx, 1000, 20, fit(100), spans, false, (f) => f)
    expect(SPAN_BAR).toBe(8)
    expect(rects).toHaveLength(2)
    for (const r of rects) {
      expect(r.h).toBe(8)
      expect(r.y).toBe(6)
      expect(r.y + r.h / 2).toBe(10)
    }
    // Across the lane where the span is: frames 10..20 of 100 on 1000 px.
    expect(rects.find((r) => r.x === 100)?.w).toBe(100)
  })
})

describe("a track's canvas", () => {
  it("is drawn at its track's real height, in device pixels", () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null)
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(20)
    vi.stubGlobal("devicePixelRatio", 2)
    const draw = vi.fn()
    const { container } = render(createElement(TrackCanvas, { axis: { width: 300 }, draw }))
    const canvas = container.querySelector("canvas")!
    expect(canvas.height).toBe(40)
    expect(canvas.width).toBe(600)
  })

  it("draws what it holds at that height", () => {
    const { ctx } = recorder()
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(ctx as never)
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(16)
    vi.stubGlobal("devicePixelRatio", 1.5)
    const draw = vi.fn()
    const { container } = render(createElement(TrackCanvas, { axis: { width: 300 }, draw }))
    expect(container.querySelector("canvas")!.height).toBe(24)
    expect(draw).toHaveBeenCalledWith(ctx, 300, 16)
  })
})
