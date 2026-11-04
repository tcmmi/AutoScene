// English, synchronously, before any test renders (lib/i18n.ts); a test that
// switches language gets English back afterwards (test/i18n.ts).
import { afterEach } from "vitest"
import "@/lib/i18n"
import { resetHolds } from "@/state/activity"
import { resetLanguage } from "./i18n"

// The activity store keeps its progress holds outside React state: no test
// leaves one running into the next.
afterEach(resetHolds)
afterEach(resetLanguage)

// jsdom lacks a few browser APIs that Radix primitives call while they open
// (pointer capture, scrolling an option into view, measuring their trigger).

if (!Element.prototype.hasPointerCapture) {
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
}

if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {}
}

if (!("ResizeObserver" in globalThis)) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
}
