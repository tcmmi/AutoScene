import type * as React from "react"
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react"
import { defaultRangeExtractor, type Range, useVirtualizer } from "@tanstack/react-virtual"
import { anchorShift, gridColumns, gridRows } from "@/lib/grid"
import { cn } from "@/lib/utils"
import { usePageScrollElement } from "./page-scroll"

/** The first guess at a detail row's height, in rows of pictures: it is measured before anything paints. */
const DETAIL_ROWS = 1.5

const reducedMotion = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false

/** What a detail row is told about the grid it opens in. */
export type GridLayout = { itemWidth: number; columns: number; gap: number; column: number }

/**
 * A grid of pictures that renders only the rows on screen — a character's
 * shots or everyone else's faces run into the thousands. Columns fill the
 * width like CSS `auto-fill` (each at least `minItemWidth`); every row is as
 * tall as `itemHeight(itemWidth)`. It scrolls with the page it is on.
 *
 * `detail` opens a full-width row under the item at `index`, as tall as it
 * turns out. Opening it elsewhere keeps that item where it was on screen,
 * unless it would open below the fold: the page then scrolls just enough to
 * show it. The item's row and the detail row stay rendered while it is open,
 * so focus inside them survives scrolling away.
 */
export function VirtualGrid<T>({
  items,
  minItemWidth,
  itemHeight,
  gap = 16,
  getKey,
  renderItem,
  detail,
  overscan = 4,
  className,
}: {
  items: T[]
  minItemWidth: number
  /** An item's height for a width (pictures keep their shape). */
  itemHeight: (width: number) => number
  gap?: number
  getKey: (item: T) => React.Key
  renderItem: (item: T, index: number) => React.ReactNode
  detail?: {
    /** The item whose row it follows. */
    index: number
    key: React.Key
    render: (layout: GridLayout) => React.ReactNode
  }
  overscan?: number
  className?: string
}) {
  const scroller = usePageScrollElement()
  const ref = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  // Where the grid starts inside the scrolling page: content above it (a
  // hero, a header that wraps) moves it.
  const [top, setTop] = useState(0)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el || !scroller) return
    const measure = () => {
      setWidth(el.clientWidth)
      setTop(el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop)
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    // Content above the grid changing height (a section that loads late, a
    // header that wraps) moves it without resizing it: the page's content
    // grows with it.
    for (const child of scroller.children) ro.observe(child)
    return () => ro.disconnect()
  }, [scroller])

  const { columns, itemWidth } = gridColumns(width, minItemWidth, gap)
  const rowHeight = itemHeight(itemWidth)
  const detailIndex = detail?.index
  const detailKey = detail?.key
  const rows = useMemo(() => gridRows(items.length, columns, detailIndex), [items.length, columns, detailIndex])
  const detailRow = rows.findIndex((r) => r.kind === "detail")
  // A new function makes the virtualizer lay the rows out anew: it remembers
  // its offsets by the count alone, and neither moving the detail row nor a
  // new row height changes that. (`measure()` would forget the detail row's
  // height too, and nothing measures it again until it changes.) Rows go by
  // their place, so a card leaving the list doesn't rebuild every row after it.
  const rowKey = useCallback(
    (i: number): React.Key => {
      const row = rows[i]
      return row.kind === "detail" ? `detail:${detailKey}` : row.first
    },
    // oxlint-disable-next-line react-hooks/exhaustive-deps -- a new height is why it's a new function.
    [rows, detailKey, rowHeight],
  )
  const rangeExtractor = useCallback(
    (range: Range) => {
      const shown = defaultRangeExtractor(range)
      if (detailRow < 0) return shown
      return [...new Set([...shown, detailRow - 1, detailRow])].sort((a, b) => a - b)
    },
    [detailRow],
  )
  // The virtualizer is read during this render only, never handed to a
  // memoised child (and the app doesn't run the React Compiler).
  // oxlint-disable-next-line react/incompatible-library
  const virtualizer = useVirtualizer({
    count: width > 0 ? rows.length : 0,
    getScrollElement: () => scroller,
    estimateSize: (i) => (rows[i]?.kind === "detail" ? rowHeight * DETAIL_ROWS : rowHeight),
    getItemKey: rowKey,
    rangeExtractor,
    gap,
    overscan,
    scrollMargin: top,
  })
  // This grid keeps its rows in place itself (below), and a detail row
  // measured while that happens would be counted twice.
  useLayoutEffect(() => {
    virtualizer.shouldAdjustScrollPositionOnItemSizeChange = () => false
  }, [virtualizer])

  // Where the rows on screen were when the page last drew.
  const placed = useRef(new Map<React.Key, number>())
  useLayoutEffect(() => {
    if (detailRow < 1 || !scroller) return
    const row = virtualizer.getVirtualItems().find((r) => r.index === detailRow - 1)
    if (!row) return
    const shift = anchorShift(placed.current.get(row.key), row.start)
    if (shift !== 0) scroller.scrollTop += shift
  }, [virtualizer, scroller, detailRow, detailKey])
  useLayoutEffect(() => {
    placed.current = new Map(virtualizer.getVirtualItems().map((row) => [row.key, row.start]))
  })
  // Only when it opens: a band already open stays where the person scrolled it.
  const revealed = useRef(detailKey)
  useLayoutEffect(() => {
    if (revealed.current === detailKey) return
    revealed.current = detailKey
    const band = virtualizer.getVirtualItems().find((r) => r.index === detailRow)
    if (!band || !scroller) return
    // After the anchoring above, on what it left on screen.
    const below = band.start + Math.min(band.size, rowHeight * DETAIL_ROWS) - (scroller.scrollTop + scroller.clientHeight)
    if (below > 0) scroller.scrollBy({ top: below, behavior: reducedMotion() ? "auto" : "smooth" })
  })

  return (
    <div ref={ref} className={cn("relative w-full", className)} style={{ height: virtualizer.getTotalSize() }}>
      {virtualizer.getVirtualItems().map((vrow) => {
        const row = rows[vrow.index]
        const y = { transform: `translateY(${vrow.start - top}px)` }
        if (!row) return null
        if (row.kind === "detail") {
          return (
            <div key={vrow.key} ref={virtualizer.measureElement} data-index={vrow.index} className="absolute inset-x-0 top-0" style={y}>
              {detail?.render({ itemWidth, columns, gap, column: detail.index % columns })}
            </div>
          )
        }
        return (
          <div
            key={vrow.key}
            className="absolute inset-x-0 top-0 grid"
            style={{ ...y, gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`, gap }}
          >
            {items.slice(row.first, row.end).map((item, i) => (
              <div key={getKey(item)} className="min-w-0">
                {renderItem(item, row.first + i)}
              </div>
            ))}
          </div>
        )
      })}
    </div>
  )
}
