import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it } from "vitest"
import { Progress, ProgressRing } from "./progress"

afterEach(cleanup)

// The reduced-motion rule matches classes (styles/reduced-motion.test.ts). An inline
// animation would beat a class and keep looping, so the motion has to come from one.
describe("Progress under reduced motion", () => {
  it("sweeps by a class the reduced-motion rule replaces, never by an inline animation", () => {
    render(<Progress value={null} />)
    const sweep = screen.getByRole("progressbar").firstElementChild as HTMLElement
    expect(sweep.classList).toContain("animate-indeterminate")
    expect(sweep.classList).toContain("motion-reduce:w-auto")
    expect(sweep.style.animation).toBe("")
    expect(sweep.style.animationName).toBe("")
  })

  it("spins the ring by a class, and turns it into a faint whole circle under reduced motion", () => {
    render(<ProgressRing value={null} />)
    const spinner = screen.getByRole("progressbar").querySelectorAll("circle")[1]
    expect(spinner.classList).toContain("animate-spin")
    expect(spinner.classList).toContain("motion-reduce:[stroke-dasharray:100_0]")
    // At the bar's tint and unlit: a whole circle in full gold would read as done.
    expect(spinner.classList).toContain("motion-reduce:[stroke:var(--accent-line)]")
    expect(spinner.classList).toContain("motion-reduce:drop-shadow-none")
    expect(spinner.style.animation).toBe("")
  })

  it("shows a value without any loop", () => {
    render(<Progress value={0.5} />)
    const fill = screen.getByRole("progressbar").firstElementChild as HTMLElement
    expect(fill.className).not.toMatch(/animate-/)
  })
})
