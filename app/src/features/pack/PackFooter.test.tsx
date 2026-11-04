import type * as React from "react"
import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { TooltipProvider } from "@/components/ui/tooltip"
import { PackFooter } from "./PackFooter"

const totals = { clips: 129, frames: 13834, seconds: 577 }
const wrapper = ({ children }: { children: React.ReactNode }) => <TooltipProvider>{children}</TooltipProvider>
const footer = (bytes: number | null, format: string | null) =>
  render(
    <PackFooter
      totals={totals}
      bytes={bytes}
      format={format}
      picks={0}
      canExport
      onClearPicks={() => {}}
      onExport={() => {}}
      onShowExport={() => {}}
    />,
    { wrapper },
  )

afterEach(cleanup)

describe("PackFooter", () => {
  it("names the format its size is about", () => {
    footer(13.2e9, "ProRes 422 HQ")
    expect(screen.getByText("9:37 · about 12.3 GB (ProRes 422 HQ)")).toBeTruthy()
  })

  it("shows the size alone while the formats load, and no size without one", () => {
    footer(13.2e9, null)
    expect(screen.getByText("9:37 · about 12.3 GB")).toBeTruthy()
    cleanup()
    footer(null, "ProRes 422 HQ")
    expect(screen.getByText("9:37")).toBeTruthy()
  })

  it("shows how far the pack's export is, and opens it rather than exporting again", () => {
    const [onExport, onShowExport] = [vi.fn(), vi.fn()]
    const exporting = (state: "queued" | "running", progress: number | null) =>
      render(
        <PackFooter
          totals={totals}
          bytes={null}
          format={null}
          picks={0}
          canExport={false}
          exporting={{ state, progress }}
          onClearPicks={() => {}}
          onExport={onExport}
          onShowExport={onShowExport}
        />,
        { wrapper },
      )
    exporting("running", 0.12)
    fireEvent.click(screen.getByRole("button", { name: "Exporting… 12%" }))
    expect(onShowExport).toHaveBeenCalledOnce()
    expect(onExport).not.toHaveBeenCalled()
    cleanup()
    exporting("queued", null)
    expect(screen.getByRole("button", { name: "Export queued" })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /^Export$/ })).toBeNull()
  })

  it("shows a waiting export's ring empty and still: no arc, no spinner", () => {
    const ring = (state: "queued" | "running", progress: number | null) => {
      cleanup()
      render(
        <PackFooter
          totals={totals}
          bytes={null}
          format={null}
          picks={0}
          canExport={false}
          exporting={{ state, progress }}
          onClearPicks={() => {}}
          onExport={() => {}}
          onShowExport={() => {}}
        />,
        { wrapper },
      )
      const svg = screen.getByRole("button", { name: /Export/ }).querySelector("svg[role=progressbar]")!
      const [, arc, spinner] = svg.querySelectorAll("circle")
      return { arc: arc as SVGCircleElement, spinner }
    }
    const queued = ring("queued", null)
    expect(queued.arc.style.opacity).toBe("0")
    expect(queued.spinner).toBeUndefined()
    // Gold only while it runs.
    expect(screen.getByRole("button", { name: "Export queued" }).className).not.toContain("text-accent")
    const running = ring("running", 0.12)
    expect(running.arc.style.opacity).toBe("1")
    expect(running.spinner).toBeUndefined()
    expect(screen.getByRole("button", { name: /Exporting/ }).className).toContain("text-accent")
    // A running export that has just started, at 0, keeps its start arc.
    expect(ring("running", 0).arc.style.opacity).toBe("1")
  })

  it("export waits while a plugin checks shots", () => {
    const onExport = vi.fn()
    render(
      <PackFooter
        totals={totals}
        bytes={null}
        format={null}
        picks={0}
        canExport={false}
        check={{ kind: "checking", checked: 120, total: 312 }}
        onClearPicks={() => {}}
        onExport={onExport}
        onShowExport={() => {}}
      />,
      { wrapper },
    )
    expect(screen.getByText("Checking 120 of 312 shots")).toBeTruthy()
    const exportButton = screen.getByRole("button", { name: "Export, waits for the check" })
    expect((exportButton as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(exportButton)
    expect(onExport).not.toHaveBeenCalled()
    // What holds it says why, to the keyboard too (a disabled button takes no focus).
    fireEvent.focus(exportButton.parentElement!)
    expect(screen.getAllByText("Waits for the check").length).toBeGreaterThan(0)
  })

  it("a failed check offers Try again and lets Export go", () => {
    const [onExport, onTryAgain] = [vi.fn(), vi.fn()]
    render(
      <PackFooter
        totals={totals}
        bytes={null}
        format={null}
        picks={0}
        canExport
        check={{ kind: "failed", unchecked: 120 }}
        onClearPicks={() => {}}
        onTryAgain={onTryAgain}
        onExport={onExport}
        onShowExport={() => {}}
      />,
      { wrapper },
    )
    const status = screen.getByText(/Couldn.t check 120 shots/)
    fireEvent.click(screen.getByRole("button", { name: "Try again" }))
    expect(onTryAgain).toHaveBeenCalledOnce()
    // The shots it did not judge keep its word from before, or are in: as the grid shows them.
    fireEvent.focus(status)
    expect(screen.getAllByText(/keep what the filter said with its settings before, or stay in/).length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole("button", { name: /^Export$/ }))
    expect(onExport).toHaveBeenCalledOnce()
  })

  it("says nothing of checks when there are none", () => {
    footer(null, null)
    expect(screen.queryByText(/Checking|check/)).toBeNull()
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull()
  })
})
