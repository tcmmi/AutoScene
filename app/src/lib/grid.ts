/** Columns that fit `width` with at least `min` px each, and the width each gets. */
export function gridColumns(width: number, min: number, gap: number): { columns: number; itemWidth: number } {
  const columns = Math.max(1, Math.floor((width + gap) / (min + gap)))
  return { columns, itemWidth: Math.max(0, (width - gap * (columns - 1)) / columns) }
}

/** A grid's rows: pictures, or the detail row under the row that holds its item. */
export type GridRow = { kind: "items"; first: number; end: number } | { kind: "detail" }

/** The rows of `count` items in `columns` columns, with a detail row after the row holding `detailIndex`. */
export function gridRows(count: number, columns: number, detailIndex?: number | null): GridRow[] {
  const rows: GridRow[] = []
  for (let first = 0; first < count; first += columns) {
    const end = Math.min(first + columns, count)
    rows.push({ kind: "items", first, end })
    if (detailIndex != null && detailIndex >= first && detailIndex < end) rows.push({ kind: "detail" })
  }
  return rows
}

/** How far to scroll to keep a row in place that moved from `before` (unknown: it wasn't on screen) to `after`. */
export function anchorShift(before: number | undefined, after: number): number {
  return before === undefined ? 0 : after - before
}

/**
 * Clicking in a grid of selectable pictures, like a file browser: Shift
 * adds the range from the last picture clicked; Ctrl/⌘ — or any click once
 * something is selected — flips one; a plain click starts a selection.
 */
export function clickSelect<K>(
  keys: readonly K[],
  selected: ReadonlySet<K>,
  anchor: K | null,
  index: number,
  { shift, toggle }: { shift: boolean; toggle: boolean },
): { selected: Set<K>; anchor: K | null } {
  const key = keys[index]
  const next = new Set(selected)
  if (shift && anchor != null) {
    const a = keys.indexOf(anchor)
    const [lo, hi] = a < 0 ? [index, index] : [Math.min(a, index), Math.max(a, index)]
    for (let k = lo; k <= hi; k++) next.add(keys[k])
    return { selected: next, anchor }
  }
  if (toggle || selected.size > 0) {
    if (next.has(key)) next.delete(key)
    else next.add(key)
  } else {
    next.add(key)
  }
  return { selected: next, anchor: key }
}
