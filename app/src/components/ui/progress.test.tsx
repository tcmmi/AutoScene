import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { Progress, ProgressRing } from "./progress"

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The bar's clip, its fill, and its sweep (null until the bar has swept). */
function parts() {
  const bar = screen.getByRole("progressbar")
  const clip = bar.firstElementChild as HTMLElement
  const [fill, wrap] = [...clip.children] as HTMLElement[]
  return { bar, clip, fill, sweep: wrap.firstElementChild as HTMLElement | null }
}

/** The ring's arc, and its spinner (null until the ring has spun). */
function ringParts() {
  const ring = screen.getByRole("progressbar")
  const [, arc, spinner] = ring.querySelectorAll("circle")
  return { ring, arc, spinner: (spinner ?? null) as SVGCircleElement | null }
}

/** Renders at a clock the test moves, so glide times are exact. */
function clock() {
  let now = 0
  vi.spyOn(performance, "now").mockImplementation(() => now)
  return (ms: number) => {
    now = ms
  }
}

// The reduced-motion rule matches classes (styles/reduced-motion.test.ts). An inline
// animation would beat a class and keep looping, so the motion has to come from one.
describe("Progress under reduced motion", () => {
  it("sweeps by a class the reduced-motion rule replaces, never by an inline animation", () => {
    render(<Progress value={null} />)
    const { sweep } = parts()
    expect(sweep?.classList).toContain("animate-indeterminate")
    expect(sweep?.classList).toContain("motion-reduce:w-auto")
    expect(sweep?.style.animation).toBe("")
    expect(sweep?.style.animationName).toBe("")
  })

  it("spins the ring by a class, and turns it into a faint whole circle under reduced motion", () => {
    render(<ProgressRing value={null} />)
    const { spinner } = ringParts()
    expect(spinner?.classList).toContain("animate-spin")
    expect(spinner?.classList).toContain("motion-reduce:[stroke-dasharray:100_0]")
    // At the bar's tint and unlit: a whole circle in full gold would read as done.
    expect(spinner?.classList).toContain("motion-reduce:[stroke:var(--accent-line)]")
    expect(spinner?.classList).toContain("motion-reduce:drop-shadow-none")
    expect(spinner?.style.animation).toBe("")
  })

  it("shows a value without any loop", () => {
    render(<Progress value={0.5} />)
    expect(parts().bar.innerHTML).not.toMatch(/animate-/)
  })
})

describe("Progress", () => {
  it("keeps one fill and one sweep through value, unknown and value again: it never flips", () => {
    const { rerender } = render(<Progress value={0.4} />)
    const fill = parts().fill
    rerender(<Progress value={null} />)
    const sweep = parts().sweep
    expect(parts().fill).toBe(fill)
    expect(sweep).not.toBeNull()
    rerender(<Progress value={0.6} />)
    expect(parts().fill).toBe(fill)
    expect(parts().sweep).toBe(sweep)
  })

  it("has no sweep until it sweeps", () => {
    const { rerender } = render(<Progress value={0.1} />)
    rerender(<Progress value={0.7} />)
    expect(parts().sweep).toBeNull()
  })

  it("pauses its sweep under a value and runs it while unknown, crossfading the two", () => {
    const { rerender } = render(<Progress value={null} />)
    expect(parts().sweep?.style.animationPlayState).toBe("running")
    expect(parts().fill.style.opacity).toBe("0")
    rerender(<Progress value={0.3} />)
    expect(parts().sweep?.style.animationPlayState).toBe("paused")
    expect(parts().fill.style.opacity).toBe("1")
    expect(parts().sweep?.parentElement?.style.opacity).toBe("0")
  })

  it("slides the fill: a round cap at every value, the whole track at 1", () => {
    const { rerender } = render(<Progress value={0.03} />)
    expect(parts().fill.style.transform).toBe("translateX(-97%)")
    rerender(<Progress value={1} />)
    expect(parts().fill.style.transform).toBe("translateX(0%)")
  })

  it("glides at the pace of its values and jumps to a lower one", () => {
    const at = clock()
    const { rerender } = render(<Progress value={0.1} />)
    for (const [t, v] of [
      [500, 0.2],
      [1000, 0.3],
    ] as const) {
      at(t)
      rerender(<Progress value={v} />)
    }
    expect(parts().fill.style.transitionDuration).toBe("600ms, var(--dur-med)")
    expect(parts().fill.style.transitionTimingFunction).toBe("linear, linear")
    at(1500)
    rerender(<Progress value={0.05} />)
    expect(parts().fill.style.transitionDuration).toBe("0ms, var(--dur-med)")
    expect(parts().fill.style.transform).toBe("translateX(-95%)")
  })

  it("eases a change that stands alone", () => {
    const at = clock()
    const { rerender } = render(<Progress value={0.2} />)
    at(5000)
    rerender(<Progress value={0.4} />)
    expect(parts().fill.style.transitionDuration).toBe("320ms, var(--dur-med)")
    expect(parts().fill.style.transitionTimingFunction).toBe("var(--ease-out), linear")
  })

  it("glows outside its track while gold shows, and promotes the fill only then", () => {
    const { rerender } = render(<Progress value={0} />)
    // An empty bar is empty: no fill (not even its edge), no glow.
    expect(parts().fill.style.opacity).toBe("0")
    expect(parts().clip.className).not.toMatch(/drop-shadow/)
    expect(parts().fill.classList).not.toContain("will-change-transform")
    expect(parts().bar.className).not.toMatch(/overflow-hidden/)
    rerender(<Progress value={0.5} />)
    expect(parts().clip.className).toMatch(/drop-shadow/)
    expect(parts().clip.classList).toContain("overflow-hidden")
    expect(parts().fill.classList).toContain("will-change-transform")
  })

  it("tells assistive tech the rounded value, and nothing while unknown", () => {
    const { rerender } = render(<Progress value={0.426} />)
    expect(parts().bar.getAttribute("aria-valuenow")).toBe("43")
    rerender(<Progress value={null} />)
    expect(parts().bar.hasAttribute("aria-valuenow")).toBe(false)
  })

  it("renders a bad value once, without a re-render loop", () => {
    for (const [value, now] of [
      [undefined, null],
      [Number.NaN, null],
      [Number.POSITIVE_INFINITY, null],
      [1.5, "100"],
    ] as const) {
      const errors = vi.spyOn(console, "error").mockImplementation(() => {})
      const { rerender, unmount } = render(<Progress value={0.2} />)
      rerender(<Progress value={value} />)
      rerender(<Progress value={value} />)
      expect(parts().bar.getAttribute("aria-valuenow")).toBe(now)
      expect(errors).not.toHaveBeenCalled()
      unmount()
      errors.mockRestore()
    }
  })
})

describe("ProgressRing", () => {
  it("keeps one arc and one spinner through value, unknown and value again", () => {
    const { rerender } = render(<ProgressRing value={0.4} />)
    const { arc } = ringParts()
    expect(ringParts().spinner).toBeNull()
    rerender(<ProgressRing value={null} />)
    const { spinner } = ringParts()
    expect(spinner).not.toBeNull()
    expect(spinner?.style.animationPlayState).toBe("running")
    expect(arc.style.opacity).toBe("0")
    rerender(<ProgressRing value={0.6} />)
    expect(ringParts().arc).toBe(arc)
    expect(ringParts().spinner).toBe(spinner)
    expect(spinner?.style.animationPlayState).toBe("paused")
    expect(arc.style.opacity).toBe("1")
    expect(arc.getAttribute("stroke-dasharray")).toBe("60 100")
  })

  it("steps with a stream of values and eases a change that stands alone", () => {
    const at = clock()
    const { rerender } = render(<ProgressRing value={0.1} />)
    at(300)
    rerender(<ProgressRing value={0.2} />)
    expect(ringParts().arc.style.transitionDuration).toBe("0ms, var(--dur-med)")
    at(5000)
    rerender(<ProgressRing value={0.4} />)
    expect(ringParts().arc.style.transitionDuration).toBe("var(--dur-slow), var(--dur-med)")
  })

  it("draws no arc while it waits, not even its round cap's dot, whatever its value", () => {
    const { rerender } = render(<ProgressRing waiting value={0} />)
    expect(ringParts().arc.style.opacity).toBe("0")
    expect(ringParts().spinner).toBeNull()
    expect(ringParts().ring.getAttribute("aria-valuenow")).toBe("0")
    // Waiting never spins, even with no value told.
    rerender(<ProgressRing waiting value={null} />)
    expect(ringParts().arc.style.opacity).toBe("0")
    expect(ringParts().spinner).toBeNull()
    rerender(<ProgressRing value={0.01} />)
    expect(ringParts().arc.style.opacity).toBe("1")
    expect(ringParts().arc.getAttribute("stroke-dasharray")).toBe("3 100")
  })

  it("keeps a start arc for running work at 0, so a new stage doesn't blink it out", () => {
    // The backend tells a stage's start as exactly 0 while the job runs.
    const { rerender } = render(<ProgressRing value={0.99} />)
    rerender(<ProgressRing value={0} />)
    expect(ringParts().arc.style.opacity).toBe("1")
    expect(ringParts().arc.getAttribute("stroke-dasharray")).toBe("3 100")
    rerender(<ProgressRing value={0.004} />)
    expect(ringParts().arc.style.opacity).toBe("1")
  })

  it("tells assistive tech the rounded value", () => {
    const { rerender } = render(<ProgressRing value={0.257} />)
    expect(ringParts().ring.getAttribute("aria-valuenow")).toBe("26")
    rerender(<ProgressRing value={null} />)
    expect(ringParts().ring.hasAttribute("aria-valuenow")).toBe(false)
  })
})
