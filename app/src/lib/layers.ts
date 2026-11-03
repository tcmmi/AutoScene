import { useEffect, useState } from "react"

/** How long `<body>` may stay unclickable with nothing open before it is given back. */
const GRACE_MS = 300

/**
 * Last line of defence against a window that no longer takes clicks.
 *
 * Modal layers (dialogs, menus, selects) set `pointer-events: none` on
 * `<body>` while they are open and restore it when the last one closes. When
 * that bookkeeping goes wrong — two copies of a Radix primitive once did (see
 * scripts/check-deps.mjs) — nothing on screen answers the mouse again until a
 * restart. This gives the window back once nothing is open any more.
 */
export function useLayerSafetyNet() {
  useEffect(() => {
    let timer: number | undefined
    const check = () => {
      window.clearTimeout(timer)
      if (document.body.style.pointerEvents !== "none") return
      timer = window.setTimeout(() => {
        if (document.body.style.pointerEvents === "none" && !layerOpen()) {
          document.body.style.pointerEvents = ""
          console.warn("A closed layer left the window unclickable; it takes clicks again.")
        }
      }, GRACE_MS)
    }
    const observer = new MutationObserver(check)
    observer.observe(document.body, { attributes: true, attributeFilter: ["style"] })
    check()
    return () => {
      observer.disconnect()
      window.clearTimeout(timer)
    }
  }, [])
}

/**
 * A dialog, sheet, menu or list of choices is open: it owns the keyboard and
 * the pointer until it closes. (Tooltips don't count, nor a list that is
 * always part of the page.)
 */
export function layerOpen(): boolean {
  return document.querySelector(LAYERS) != null
}

const LAYERS = ["dialog", "alertdialog", "menu", "listbox"].map((r) => `[role="${r}"][data-state="open"]`).join(", ")

/**
 * `layerOpen()` as state, while `watch` is true: it follows layers opening
 * and closing (their `data-state`), so something waiting for the window to be
 * free sees the moment it is. Nothing is observed while it isn't watching.
 */
export function useLayerOpen(watch: boolean): boolean {
  const [open, setOpen] = useState(false)
  useEffect(() => {
    if (!watch) return
    const check = () => setOpen(layerOpen())
    const observer = new MutationObserver(check)
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["data-state"] })
    check()
    return () => observer.disconnect()
  }, [watch])
  return watch && open
}
