import { useLayoutEffect, useState } from "react"
import { act, cleanup, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { anchorShift, gridRows } from "@/lib/grid"
import { PageScrollElement } from "./page-scroll"
import { VirtualGrid } from "./virtual-grid"

const items = (n: number) => Array.from({ length: n }, (_, i) => i)
const kinds = (rows: ReturnType<typeof gridRows>) => rows.map((r) => (r.kind === "detail" ? "detail" : `${r.first}-${r.end}`))

describe("gridRows", () => {
  it("makes rows of the column count, the last one short", () => {
    expect(kinds(gridRows(7, 3))).toEqual(["0-3", "3-6", "6-7"])
    expect(gridRows(0, 3)).toEqual([])
  })

  it("puts the detail row after the row of its item, whichever column it is in", () => {
    expect(kinds(gridRows(7, 3, 3))).toEqual(["0-3", "3-6", "detail", "6-7"])
    expect(kinds(gridRows(7, 3, 5))).toEqual(["0-3", "3-6", "detail", "6-7"])
  })

  it("puts it last under the last row", () => {
    expect(kinds(gridRows(7, 3, 6))).toEqual(["0-3", "3-6", "6-7", "detail"])
  })

  it("has none for an item that isn't there", () => {
    expect(kinds(gridRows(7, 3, 9))).toEqual(["0-3", "3-6", "6-7"])
    expect(kinds(gridRows(7, 3, null))).toEqual(["0-3", "3-6", "6-7"])
  })
})

describe("anchorShift", () => {
  it("scrolls by how far the row moved", () => {
    expect(anchorShift(900, 500)).toBe(-400)
    expect(anchorShift(500, 500)).toBe(0)
  })

  it("leaves a row alone that wasn't on screen", () => {
    expect(anchorShift(undefined, 500)).toBe(0)
  })
})

const ROW = 100
const GAP = 16
const BAND = 300
/** How tall a detail row turns out. */
let band = BAND

let view: HTMLElement
let open: (index: number | null) => void
/** The window's width changes: the rows get another height, the detail row stays as tall. */
let rowsOf: (height: number) => void
let list: (next: number[]) => void

const own = (i: number) => i

/** A page of `count` numbered pictures, three to a row; what it hands the grid stays the same, like the app's pages. */
function Grid({ count }: { count: number }) {
  const [shown, setShown] = useState(() => items(count))
  const [row, setRow] = useState(ROW)
  const [scroller, setScroller] = useState<HTMLDivElement | null>(null)
  const [index, setIndex] = useState<number | null>(null)
  useLayoutEffect(() => {
    open = setIndex
    rowsOf = setRow
    list = setShown
    if (scroller) view = scroller
  }, [scroller])
  return (
    <div ref={setScroller}>
      <PageScrollElement.Provider value={scroller}>
        <VirtualGrid
          items={shown}
          minItemWidth={240}
          itemHeight={() => row}
          gap={GAP}
          getKey={own}
          renderItem={(i) => <button>{`item ${i}`}</button>}
          detail={index === null ? undefined : { index, key: index, render: (l) => <p>{`band at ${l.column} of ${l.columns}`}</p> }}
        />
      </PageScrollElement.Provider>
    </div>
  )
}

/** Where the row of an item is. */
const rowY = (i: number) => {
  const row = screen.getByRole("button", { name: `item ${i}` }).parentElement!.parentElement!
  return Number(/translateY\((.+)px\)/.exec(row.style.transform)?.[1])
}
const height = () => parseFloat(view.querySelector<HTMLElement>(".relative")!.style.height)

describe("a grid with a detail row", () => {
  beforeEach(() => {
    // jsdom doesn't lay out: 1000 wide, a page 800 tall, a detail row 300 tall.
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1000)
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(800)
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(1000)
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) {
      return this.hasAttribute("data-index") ? band : 800
    })
    band = BAND
    Element.prototype.scrollBy ??= () => {}
    Element.prototype.scrollTo ??= () => {}
  })
  afterEach(() => {
    cleanup()
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it("lays out without one as before", () => {
    render(<Grid count={30} />)
    expect(rowY(0)).toBe(0)
    expect(rowY(3)).toBe(ROW + GAP)
    expect(height()).toBe(10 * ROW + 9 * GAP)
  })

  it("opens under the row of its item and counts its measured height", () => {
    render(<Grid count={30} />)
    act(() => open(4))
    expect(screen.getByText("band at 1 of 3")).toBeTruthy()
    expect(rowY(3)).toBe(ROW + GAP)
    expect(rowY(6)).toBe(2 * (ROW + GAP) + BAND + GAP)
    expect(height()).toBe(10 * ROW + 9 * GAP + BAND + GAP)
  })

  it("moves the rows between when it switches rows, though the row count stays the same", () => {
    render(<Grid count={30} />)
    act(() => open(1))
    expect(rowY(3)).toBe(ROW + GAP + BAND + GAP)
    act(() => open(4))
    expect(rowY(3)).toBe(ROW + GAP)
    expect(rowY(6)).toBe(2 * (ROW + GAP) + BAND + GAP)
    expect(height()).toBe(10 * ROW + 9 * GAP + BAND + GAP)
  })

  it("moves them too when no measurement says anything changed: the band is as tall as first guessed", () => {
    band = ROW * 1.5
    render(<Grid count={30} />)
    act(() => open(1))
    expect(rowY(3)).toBe(ROW + GAP + band + GAP)
    act(() => open(4))
    expect(rowY(3)).toBe(ROW + GAP)
    expect(rowY(6)).toBe(2 * (ROW + GAP) + band + GAP)
  })

  it("keeps the cards below it the same elements", () => {
    render(<Grid count={30} />)
    const card = screen.getByRole("button", { name: "item 7" })
    act(() => open(1))
    expect(screen.getByRole("button", { name: "item 7" })).toBe(card)
    act(() => open(4))
    expect(screen.getByRole("button", { name: "item 7" })).toBe(card)
    act(() => open(null))
    expect(screen.getByRole("button", { name: "item 7" })).toBe(card)
  })

  it("keeps the height it measured when the rows change theirs", () => {
    render(<Grid count={30} />)
    act(() => open(4))
    act(() => rowsOf(150))
    expect(rowY(3)).toBe(150 + GAP)
    expect(rowY(6)).toBe(2 * (150 + GAP) + BAND + GAP)
    expect(height()).toBe(10 * 150 + 9 * GAP + BAND + GAP)
  })

  it("lays out a new row height without one too", () => {
    render(<Grid count={30} />)
    act(() => rowsOf(150))
    expect(rowY(3)).toBe(150 + GAP)
    expect(height()).toBe(10 * 150 + 9 * GAP)
  })

  it("keeps a card's element and focus when one before it leaves the list", () => {
    render(<Grid count={30} />)
    const card = screen.getByRole("button", { name: "item 7" })
    card.focus()
    act(() => list(items(30).slice(1)))
    expect(screen.getByRole("button", { name: "item 7" })).toBe(card)
    expect(document.activeElement).toBe(card)
  })

  it("leaves the keeping in place to itself when the band lands above the fold: nothing counts it twice", async () => {
    const scrollTo = vi.spyOn(Element.prototype, "scrollTo").mockImplementation(() => {})
    render(<Grid count={60} />)
    act(() => open(1))
    await act(async () => {
      view.scrollTop = 700
      view.dispatchEvent(new Event("scroll"))
    })
    // The virtualizer measures nothing at once until it counts the scroll as over (150 ms).
    await act(() => new Promise((done) => setTimeout(done, 200)))
    const before = rowY(9)
    scrollTo.mockClear()
    act(() => open(9))
    expect(rowY(9) - view.scrollTop).toBe(before - 700)
    expect(scrollTo).not.toHaveBeenCalled()
  })

  it("keeps a card on screen when the band opening under it closes one above it", () => {
    render(<Grid count={30} />)
    act(() => open(1))
    view.scrollTop = 500
    const before = rowY(9)
    act(() => open(9))
    expect(rowY(9) - view.scrollTop).toBe(before - 500)
  })

  it("stays put when it opens above the one it closes", () => {
    render(<Grid count={30} />)
    act(() => open(9))
    view.scrollTop = 200
    act(() => open(1))
    expect(view.scrollTop).toBe(200)
  })

  it("keeps its item and itself rendered however far the page scrolls away", async () => {
    render(<Grid count={600} />)
    act(() => open(1))
    const card = screen.getByRole("button", { name: "item 1" })
    await act(async () => {
      view.scrollTop = 20000
      view.dispatchEvent(new Event("scroll"))
    })
    expect(screen.queryByRole("button", { name: "item 7" })).toBeNull()
    expect(screen.getByRole("button", { name: "item 1" })).toBe(card)
    expect(screen.getByText("band at 1 of 3")).toBeTruthy()
  })

  it("scrolls just enough to show a band that opens below the fold", () => {
    const scrollBy = vi.spyOn(Element.prototype, "scrollBy").mockImplementation(() => {})
    render(<Grid count={30} />)
    act(() => open(1))
    expect(scrollBy).not.toHaveBeenCalled()
    act(() => open(null))
    act(() => open(19))
    // Row 6 is the seventh: the band starts below it and shows a row and a half of itself.
    expect(scrollBy).toHaveBeenCalledWith({ top: 7 * (ROW + GAP) + ROW * 1.5 - 800, behavior: "smooth" })
  })

  it("scrolls to a band once: one already open stays where the person scrolled it", () => {
    const scrollBy = vi.spyOn(Element.prototype, "scrollBy").mockImplementation(() => {})
    render(<Grid count={30} />)
    act(() => open(19))
    expect(scrollBy).toHaveBeenCalledOnce()
    act(() => rowsOf(150))
    expect(scrollBy).toHaveBeenCalledOnce()
  })

  it("jumps instead for someone who asked for less motion", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }))
    const scrollBy = vi.spyOn(Element.prototype, "scrollBy").mockImplementation(() => {})
    render(<Grid count={30} />)
    act(() => open(19))
    expect(scrollBy).toHaveBeenCalledWith(expect.objectContaining({ behavior: "auto" }))
  })
})
