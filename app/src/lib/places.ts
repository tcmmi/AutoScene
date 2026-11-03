/**
 * Where the person was before the place they are at, and how an open plugin
 * page is left. A plugin page's frame adds entries of its own to the
 * window's history, which Back and `history.go` step through before the
 * app's: so a plugin page is left by going to the place it was opened from,
 * never by stepping back through that history.
 */
import type { RouterHistory } from "@tanstack/react-router"

/**
 * The places the person came through, oldest first, the one they are at
 * last; and, while a page is left, the places before the one it goes back to.
 */
type Places = { trail: string[]; returning: string[] | null }

/** How many places are kept. */
const KEPT = 50

const followed = new WeakMap<RouterHistory, Places>()

/** Keeps, for `history`, the places before the current one as the person moves. */
export function followPlaces(history: RouterHistory) {
  if (followed.has(history)) return
  const places: Places = { trail: [history.location.href], returning: null }
  followed.set(history, places)
  history.subscribe(() => {
    const at = history.location.href
    if (places.returning) places.trail = [...places.returning, at]
    else if (at !== places.trail.at(-1)) places.trail = [...places.trail, at].slice(-KEPT)
  })
}

/** The places before the current one, the one just before it last, while `history` is followed; none at the start. */
export function placesBefore(history: RouterHistory): string[] {
  return followed.get(history)?.trail.slice(0, -1) ?? []
}

/**
 * Goes to the last of `before`, else to `fallback`, in place of the current
 * entry; the places before that one are again the ones before it. So a
 * place left so is never one to go back to, and two pages never send the
 * person to each other.
 */
export function goBackTo(history: RouterHistory, before: string[], fallback: string) {
  const places = followed.get(history)
  if (places) places.returning = before.slice(0, -1)
  // The history tells its subscribers of a replace before it returns, unless a blocker holds it back: AutoScene has none.
  try {
    history.replace(before.at(-1) ?? fallback)
  } finally {
    if (places) places.returning = null
  }
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
