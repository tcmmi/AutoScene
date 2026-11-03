import { afterEach } from "vitest"
import { resetHolds } from "@/state/activity"

// The activity store keeps its progress holds outside React state: no test
// leaves one running into the next.
afterEach(resetHolds)

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
