import type * as React from "react"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import { DropdownMenuItem } from "@/components/ui/dropdown-menu"
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
    // Busy, not disabled: the button itself keeps the keyboard, so its tooltip needs no holder around it.
    expect((exportButton as HTMLButtonElement).disabled).toBe(false)
    expect(exportButton.getAttribute("aria-busy")).toBe("true")
    fireEvent.click(exportButton)
    expect(onExport).not.toHaveBeenCalled()
    expect(exportButton.className).not.toMatch(/aria-busy:pointer-events-none/)
    fireEvent.focus(exportButton)
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

  const fcpxml = {
    key: "dev.example.fcpxml.fcpxml",
    pluginId: "dev.example.fcpxml",
    // Not the title's words: what stands beside an entry is its plugin's name.
    pluginName: "Cutroom Bridge",
    icon: null,
    actionId: "fcpxml",
    title: "Save as FCPXML…",
  }
  const withActions = (o: { canSave?: boolean; onSave?: () => void; canExport?: boolean } = {}) =>
    render(
      <PackFooter
        totals={totals}
        bytes={null}
        format={null}
        picks={0}
        canExport={o.canExport ?? true}
        actions={[fcpxml]}
        canSave={o.canSave ?? true}
        onSave={o.onSave}
        onClearPicks={() => {}}
        onExport={() => {}}
        onShowExport={() => {}}
      />,
      { wrapper },
    )

  it("pack actions sit in ⋯ with their plugin mark", async () => {
    const onSave = vi.fn()
    withActions({ onSave })
    // Nothing shows until the menu opens, and the menu is not Export's.
    expect(screen.queryByText("Save as FCPXML…")).toBeNull()
    fireEvent.pointerDown(screen.getByRole("button", { name: "Pack options" }), { button: 0, ctrlKey: false })
    const item = screen.getByRole("menuitem", { name: /Save as FCPXML…/ })
    // Its plugin's name stands beside it, and the puzzle piece stands for an icon it hasn't.
    expect(item.textContent).toContain("Cutroom Bridge")
    expect(item.querySelector("svg")).not.toBeNull()
    expect(item.querySelector("img")).toBeNull()
    fireEvent.click(item)
    // The menu hands the entry on once it has closed.
    await waitFor(() => expect(onSave).toHaveBeenCalledExactlyOnceWith(fcpxml))
  })

  it("a plugin's own icon stands for it, as a picture", () => {
    const icon = "data:image/png;base64,iVBORw0KGgo="
    render(
      <PackFooter
        totals={totals}
        bytes={null}
        format={null}
        picks={0}
        canExport
        actions={[{ ...fcpxml, icon }]}
        canSave
        onClearPicks={() => {}}
        onExport={() => {}}
        onShowExport={() => {}}
      />,
      { wrapper },
    )
    fireEvent.pointerDown(screen.getByRole("button", { name: "Pack options" }), { button: 0, ctrlKey: false })
    const item = screen.getByRole("menuitem", { name: /Save as FCPXML…/ })
    expect(item.querySelector("img")?.getAttribute("src")).toBe(icon)
    expect(item.querySelector("svg")).toBeNull()
    // The menu's heading doesn't start the sentence the entry's title ends.
    expect(screen.getByText("Save with a plugin")).toBeTruthy()
  })

  it("no ⋯ with nothing in it", () => {
    footer(null, null)
    expect(screen.queryByRole("button", { name: "Pack options" })).toBeNull()
    cleanup()
    render(
      <PackFooter totals={totals} bytes={null} format={null} picks={0} canExport actions={[]} onClearPicks={() => {}} onExport={() => {}} onShowExport={() => {}} />,
      { wrapper },
    )
    expect(screen.queryByRole("button", { name: "Pack options" })).toBeNull()
  })

  it("what can be done to every shot comes first, and a plugin's group after it", () => {
    render(
      <PackFooter
        totals={totals}
        bytes={null}
        format={null}
        picks={0}
        canExport
        items={<DropdownMenuItem>Take every shot</DropdownMenuItem>}
        actions={[fcpxml]}
        canSave
        onClearPicks={() => {}}
        onExport={() => {}}
        onShowExport={() => {}}
      />,
      { wrapper },
    )
    fireEvent.pointerDown(screen.getByRole("button", { name: "Pack options" }), { button: 0, ctrlKey: false })
    const entries = screen.getAllByRole("menuitem").map((item) => item.textContent)
    expect(entries[0]).toBe("Take every shot")
    expect(entries[1]).toContain("Save as FCPXML…")
    expect(screen.getByRole("separator")).toBeTruthy()
  })

  const opened = () => {
    const dots = screen.getByRole("button", { name: "Pack options" }) as HTMLButtonElement
    expect(dots.disabled).toBe(false)
    fireEvent.pointerDown(dots, { button: 0, ctrlKey: false })
    return screen.getByRole("menuitem", { name: /Save as FCPXML…/ })
  }

  it("says in its menu what saving waits for", () => {
    render(
      <PackFooter
        totals={totals}
        bytes={null}
        format={null}
        picks={0}
        canExport={false}
        check={{ kind: "checking", checked: 3, total: 10 }}
        actions={[fcpxml]}
        canSave={false}
        saveHeading="Save with a plugin waits for the check"
        onClearPicks={() => {}}
        onExport={() => {}}
        onShowExport={() => {}}
      />,
      { wrapper },
    )
    expect(opened().getAttribute("aria-disabled")).toBe("true")
    expect(screen.getByText("Save with a plugin waits for the check")).toBeTruthy()
  })

  it("saving waits while the pack can't be saved, and goes on while it exports", () => {
    withActions({ canSave: false })
    expect(opened().getAttribute("aria-disabled")).toBe("true")
    cleanup()
    // An export under way doesn't hold a save back: the footer says it can.
    render(
      <PackFooter
        totals={totals}
        bytes={null}
        format={null}
        picks={0}
        canExport={false}
        exporting={{ state: "running", progress: 0.4 }}
        actions={[fcpxml]}
        canSave
        onClearPicks={() => {}}
        onExport={() => {}}
        onShowExport={() => {}}
      />,
      { wrapper },
    )
    expect(opened().getAttribute("aria-disabled")).toBeNull()
  })
})
