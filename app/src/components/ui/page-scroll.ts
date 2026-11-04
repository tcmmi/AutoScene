import { createContext, useContext, useLayoutEffect } from "react"

export const PageScrolled = createContext(false)

/** Whether the page under the top bar has scrolled (the bar turns to glass). */
export function usePageScrolled(): boolean {
  return useContext(PageScrolled)
}

export const PageScrollElement = createContext<HTMLElement | null>(null)

/** The element the current page scrolls in (for lists that render only what is on screen). */
export function usePageScrollElement(): HTMLElement | null {
  return useContext(PageScrollElement)
}

/** The history entry on screen: what a page keeps for Back goes under it. */
export const PageEntry = createContext("")

export function usePageEntry(): string {
  return useContext(PageEntry)
}

/**
 * Where each visited page was scrolled to, by history entry (for Back), the
 * latest visited last. Every visit is a new entry: only the latest are kept.
 */
const positions = new Map<string, number>()
/** More entries than the webview's history goes back. */
export const REMEMBERED_PAGES = 100
/** How long a page coming back keeps trying to reach its place while its lists load. */
export const SETTLE_MS = 1000

/** Keep what's known of `entry` as the latest, forgetting the oldest beyond the limit. */
export function keepLatest<T>(memory: Map<string, T>, entry: string, value: T) {
  memory.delete(entry)
  memory.set(entry, value)
  if (memory.size > REMEMBERED_PAGES) memory.delete(memory.keys().next().value!)
}

/**
 * A page scrolls back to where it was when it is visited again (Back,
 * Forward); a new visit starts at the top. Lists that render what is on
 * screen grow as their data arrives, so the place is tried again for a
 * moment — until it is reached or the person scrolls.
 */
export function useScrollMemory(el: HTMLElement | null, entry: string) {
  useLayoutEffect(() => {
    if (!el) return
    const remembered = positions.get(entry)
    // A page visited again is as recent as a new one.
    if (remembered != null) keepLatest(positions, entry, remembered)
    const target = remembered ?? 0
    el.scrollTo({ top: target })
    let frame = 0
    const until = performance.now() + SETTLE_MS
    const settle = () => {
      el.scrollTo({ top: target })
      if (Math.abs(el.scrollTop - target) > 1 && performance.now() < until) frame = requestAnimationFrame(settle)
    }
    if (Math.abs(el.scrollTop - target) > 1) frame = requestAnimationFrame(settle)
    const stop = () => cancelAnimationFrame(frame)
    const remember = () => keepLatest(positions, entry, el.scrollTop)
    el.addEventListener("scroll", remember, { passive: true })
    for (const e of ["wheel", "touchstart", "pointerdown", "keydown"]) el.addEventListener(e, stop, { passive: true })
    return () => {
      stop()
      el.removeEventListener("scroll", remember)
      for (const e of ["wheel", "touchstart", "pointerdown", "keydown"]) el.removeEventListener(e, stop)
    }
  }, [el, entry])
}
