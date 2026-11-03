/**
 * Where the person was before the place they are at, and how an open plugin
 * page is left. A plugin page's frame adds entries of its own to the
 * window's history, which Back and `history.go` step through before the
 * app's: so a plugin page is left by going to the place it was opened from,
 * never by stepping back through that history.
 */
import type { RouterHistory } from "@tanstack/react-router"

type Places = { at: string; before: string | null }

const followed = new WeakMap<RouterHistory, Places>()

/** Keeps, for `history`, the place before the current one as the person moves. */
export function followPlaces(history: RouterHistory) {
  if (followed.has(history)) return
  const places: Places = { at: history.location.href, before: null }
  followed.set(history, places)
  history.subscribe(() => {
    const at = history.location.href
    if (at === places.at) return
    places.before = places.at
    places.at = at
  })
}

/** The place before the current one, while `history` is followed; none at the start. */
export function placeBefore(history: RouterHistory): string | null {
  return followed.get(history)?.before ?? null
}

let leaving: (() => void) | null = null

/** How the open plugin page is left, while it is open: the window menu's Back leaves it so too. */
export function whileOpen(leave: () => void): () => void {
  leaving = leave
  return () => {
    if (leaving === leave) leaving = null
  }
}

/** Leaves the open plugin page, if one is open: whether one was. */
export function leavePage(): boolean {
  if (!leaving) return false
  leaving()
  return true
}

/** Whether a plugin page is open, which Back leaves. */
export function pageOpen(): boolean {
  return leaving != null
}
