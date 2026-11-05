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

/** How many entries the window's session history holds: read when a page opens, which is then its last. */
export function entryCount(): number {
  return window.history.length
}

/** How long a step back over the frame's entries may take before the replace goes on without its end. */
const STEP_MS = 800

/**
 * Goes to the last of `before`, else to `fallback`, in place of the entry the
 * page was opened at (`openedWith`, from `entryCount`); the places before
 * that one are again the ones before it. So a place left so is never one to
 * go back to, and two pages never send the person to each other.
 *
 * The entries the page's frame added since share the page's address, and a
 * replace would write the new place into all of them: they are stepped back
 * over first, so Back from there goes on to where the person was. Nothing
 * tells the window when that step ends, but the frame loads again where it
 * lands. If the person went Back inside the frame meanwhile, the step goes
 * that many entries too far.
 */
export async function goBackTo(history: RouterHistory, before: string[], fallback: string, openedWith: number | null = null) {
  const added = openedWith == null ? 0 : window.history.length - openedWith
  if (added > 0) {
    await new Promise<void>((done) => {
      const end = () => {
        window.removeEventListener("load", end, true)
        clearTimeout(timer)
        done()
      }
      const timer = setTimeout(end, STEP_MS)
      window.addEventListener("load", end, true)
      window.history.go(-added)
    })
  }
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
