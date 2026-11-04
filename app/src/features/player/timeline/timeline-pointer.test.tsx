import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Character, Presence, Segment } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { areasOf } from "@/lib/context-areas"
import { FrameClock } from "@/lib/frame-clock"
import { FrameStore } from "../frame-store"
import { Timeline } from "./Timeline"

const W = 1000

beforeEach(() => {
  // The tracks are W px wide and start at x = 0; nothing is drawn (they have no height).
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(W)
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: W,
    bottom: 24,
    width: W,
    height: 24,
    toJSON: () => ({}),
  } as DOMRect)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const seg = (start: number, end: number): Segment => ({ start, end, kind: "shot", tiny: false })
const character = { id: "c1", name: "Walter", label: "Walter", hidden: false } as unknown as Character
const appearance = (start: number, end: number) => ({ characterId: "c1", trackId: 1, start, end, body: false }) as unknown as Presence

/** An episode of `frames` frames at 24 fps, on tracks W px wide. */
function mount(frames: number, segments: Segment[], spans: Presence[] = [], onSeek: (f: number) => void = () => {}) {
  render(
    <TooltipProvider>
      <Timeline
        clock={new FrameClock({ frames, fps: { num: 24, den: 1 }, offset: 0, step: 1 / 24, times: null })}
        fps={24}
        counting={{ count: (f) => f, frame: (n) => n }}
        store={new FrameStore()}
        segments={segments}
        lanes={spans.length > 0 ? [{ character, spans, frames: 60 }] : []}
        focus={null}
        inFrame={null}
        outFrame={null}
        tc={(f) => String(f)}
        onSeek={onSeek}
        onScrub={() => {}}
        onMarkRange={() => {}}
        onMarkIn={() => {}}
        onMarkOut={() => {}}
        onFocus={() => {}}
        onOpenCharacter={() => {}}
      />
    </TooltipProvider>,
  )
}

const has = (el: Element, cls: string) => (el.getAttribute("class") ?? "").split(/\s+/).includes(cls)
const hand = (el: Element) => has(el, "cursor-pointer")
const grabs = (el: Element) => has(el, "cursor-grab")
const grabbing = (el: Element) => has(el, "cursor-grabbing")
const shots = () => screen.getByRole("group", { name: "Shots" })
/** A main character's track, the last cell of its row. */
const laneTrack = () => screen.getByRole("group", { name: "Characters on screen" }).querySelector(".group")!.lastElementChild as HTMLElement
const overview = () => screen.getByRole("scrollbar", { name: "Part of the episode in view" })
const scrubber = () => screen.getByRole("slider", { name: "Playhead" })
const handle = () => overview().querySelector('[class*="cursor-grab"]') as HTMLElement

/** A ten-second episode: two shots of five seconds, the character on screen from 4.2 s to 5.8 s. */
const short = () => mount(240, [seg(0, 120), seg(120, 240)], [appearance(100, 140)])

describe("the timeline's pointer", () => {
  it("the shots lane shows the hand over a shot and not after the pointer leaves", () => {
    short()
    expect(hand(shots())).toBe(false)
    fireEvent.pointerMove(shots(), { clientX: 100 })
    expect(screen.getByText("Shot 1")).toBeTruthy()
    expect(hand(shots())).toBe(true)
    fireEvent.pointerLeave(shots())
    expect(screen.queryByText("Shot 1")).toBeNull()
    expect(hand(shots())).toBe(false)
  })

  it("a character lane shows the hand over an appearance, not in a gap", () => {
    short()
    // The appearance fills x = 417..583; a gap is more than 6 px away from it.
    expect(hand(laneTrack())).toBe(false)
    fireEvent.pointerMove(laneTrack(), { clientX: 500 })
    expect(hand(laneTrack())).toBe(true)
    expect(screen.getAllByText("Walter").length).toBeGreaterThan(1)
    fireEvent.pointerMove(laneTrack(), { clientX: 800 })
    expect(hand(laneTrack())).toBe(false)
    expect(screen.getAllByText("Walter").length).toBe(1)
    // The reach is 6 px: 588 is 4.7 px from the appearance's end, 591 is 7.7 px.
    fireEvent.pointerMove(laneTrack(), { clientX: 588 })
    expect(hand(laneTrack())).toBe(true)
    fireEvent.pointerMove(laneTrack(), { clientX: 591 })
    expect(hand(laneTrack())).toBe(false)
    fireEvent.pointerLeave(laneTrack())
    expect(hand(laneTrack())).toBe(false)
  })

  it("after a zoom with the pointer at rest, the readout and the hand are about what is under it now", () => {
    short()
    // At rest over the second shot, and over the appearance.
    fireEvent.pointerMove(shots(), { clientX: 600 })
    fireEvent.pointerMove(laneTrack(), { clientX: 500 })
    expect(screen.getByText("Shot 2")).toBeTruthy()
    expect(hand(laneTrack())).toBe(true)

    // The zoom (+) is around the playhead at 0: the same x is 3 s in now, in the first shot,
    // and the appearance has moved to x = 833..1167.
    fireEvent.keyDown(window, { key: "=" })
    expect(overview()).toBeTruthy()
    expect(screen.queryByText("Shot 2")).toBeNull()
    expect(screen.getByText("Shot 1")).toBeTruthy()
    expect(hand(shots())).toBe(true)
    expect(hand(laneTrack())).toBe(false)
    expect(screen.getAllByText("Walter").length).toBe(1)

    // Ctrl + wheel out around the wheel's own x, here the right edge, shows the whole
    // episode again: the appearance (x = 417..583) is under the pointer once more.
    fireEvent.wheel(screen.getByRole("group", { name: "Characters on screen" }), { ctrlKey: true, deltaY: 300, clientX: 1000 })
    expect(screen.queryByRole("scrollbar", { name: "Part of the episode in view" })).toBeNull()
    expect(hand(laneTrack())).toBe(true)
    expect(screen.getAllByText("Walter").length).toBeGreaterThan(1)
  })

  it("the playhead's readout above the lanes follows a zoom with the pointer at rest, and names the frame a press seeks to", () => {
    const seeks: number[] = []
    mount(240, [seg(0, 120), seg(120, 240)], [appearance(100, 140)], (f) => seeks.push(f))
    fireEvent.pointerMove(scrubber(), { clientX: 800 })
    expect(within(scrubber()).getByText("192")).toBeTruthy()

    // The zoom (+) is around the playhead at 0: the same x is an earlier frame now.
    fireEvent.keyDown(window, { key: "=" })
    expect(within(scrubber()).queryByText("192")).toBeNull()
    const shown = Number(scrubber().querySelector(".timecode")!.textContent)
    expect(shown).toBeLessThan(192)
    fireEvent.pointerDown(scrubber(), { clientX: 800, button: 0, pointerId: 1 })
    expect(seeks.at(-1)).toBe(shown)
  })

  it("a click on a shot goes to the shot under the click, not under where the pointer last moved", () => {
    const seeks: number[] = []
    mount(240, [seg(0, 120), seg(120, 240)], [], (f) => seeks.push(f))
    // The pointer rests over the second shot; the click is on the first.
    fireEvent.pointerMove(shots(), { clientX: 600 })
    fireEvent.click(shots(), { clientX: 100 })
    expect(seeks).toEqual([0])
    // After a zoom the second shot's spot holds the first: a click there goes to the first, not the second.
    fireEvent.keyDown(window, { key: "=" })
    fireEvent.click(shots(), { clientX: 600 })
    expect(seeks).toEqual([0, 0])
    fireEvent.click(shots(), { clientX: 990 })
    expect(seeks.at(-1)).toBe(0)
    // Beyond the track's edge the click counts as at the edge. On a long episode zoomed to the
    // second around 1350 s, with the second shot starting at 1349 s (just before the view), 3000 px
    // left of the track is 3 s earlier, in the first shot, if it is not clamped.
    cleanup()
    const long = 24 * 2700
    mount(long, [seg(0, 24 * 1349), seg(24 * 1349, long)], [], (f) => seeks.push(f))
    fireEvent.wheel(shots(), { ctrlKey: true, deltaY: -4000, clientX: 500 })
    expect(overview().getAttribute("aria-valuenow")).toBe("1350")
    fireEvent.click(shots(), { clientX: -3000 })
    expect(seeks.at(-1)).toBe(24 * 1349)
  })

  it("a character's lane goes to the appearance under the click, and its menu is about the appearance or shot under the right-click", () => {
    const seeks: number[] = []
    mount(240, [seg(0, 120), seg(120, 240)], [appearance(100, 140)], (f) => seeks.push(f))
    fireEvent.pointerMove(laneTrack(), { clientX: 800 })
    fireEvent.click(laneTrack(), { clientX: 500 })
    expect(seeks).toEqual([100])
    fireEvent.click(laneTrack(), { clientX: 800 })
    expect(seeks).toEqual([100])

    // What the menu offers: the right-click's own area gives its entries as elements.
    const text = (n: React.ReactNode): string =>
      Array.isArray(n) ? n.map(text).join("") : typeof n === "string" || typeof n === "number" ? String(n) : n && typeof n === "object" && "props" in n ? text((n.props as { children?: React.ReactNode }).children) : ""
    let area: ReturnType<typeof areasOf>[number] | undefined
    const menu = () => {
      const content = area!.get()
      return text(typeof content === "function" ? content() : content)
    }
    const grab = (e: Event) => void (area = areasOf(e)[0])
    document.addEventListener("contextmenu", grab)
    try {
      fireEvent.contextMenu(laneTrack(), { clientX: 500 })
      expect(menu()).toContain("Go to this appearance")
      fireEvent.contextMenu(laneTrack(), { clientX: 800 })
      expect(menu()).not.toContain("Go to this appearance")
      fireEvent.contextMenu(shots(), { clientX: 600 })
      expect(menu()).toContain("Shot 2")
      fireEvent.contextMenu(shots(), { clientX: 100 })
      expect(menu()).toContain("Shot 1")
    } finally {
      document.removeEventListener("contextmenu", grab)
    }
  })

  it("the playhead's line in the overview takes no pointer of its own, so the handle shows its hand under it", () => {
    short()
    fireEvent.keyDown(window, { key: "=" })
    const line = overview().querySelector('[class~="w-px"]')!
    expect(has(line, "pointer-events-none")).toBe(true)
  })

  it("a press on any part of the drawn handle pans, even when the view is narrower than it", () => {
    // 45 minutes: a one-second view is drawn 27 s wide, at x = 495..505.
    mount(24 * 2700, [seg(0, 24 * 2700)])
    fireEvent.wheel(shots(), { ctrlKey: true, deltaY: -4000, clientX: 500 })
    expect(overview().getAttribute("aria-valuenow")).toBe("1350")

    // Three px left of the second in view (1350..1351), still on the handle.
    fireEvent.pointerDown(overview(), { clientX: 497, button: 0, pointerId: 1 })
    expect(grabbing(overview())).toBe(true)
    fireEvent.pointerMove(overview(), { clientX: 597, pointerId: 1 })
    // The view followed the drag by 100 px = 270 s; it did not centre on the press.
    expect(overview().getAttribute("aria-valuenow")).toBe("1620")
    fireEvent.pointerUp(overview(), { pointerId: 1 })

    // Beside the handle a press centres the view there and does not pan.
    fireEvent.pointerDown(overview(), { clientX: 800, button: 0, pointerId: 1 })
    expect(grabbing(overview())).toBe(false)
    expect(overview().getAttribute("aria-valuenow")).toBe("2160")
    fireEvent.pointerMove(overview(), { clientX: 900, pointerId: 1 })
    expect(overview().getAttribute("aria-valuenow")).toBe("2160")
  })

  it("zoomed in, the overview's handle grabs, and grabs harder while it pans (released on pointer up and on lost capture)", () => {
    short()
    fireEvent.keyDown(window, { key: "=" })
    // Outside the handle a click goes there: the hand. On the handle: grab.
    expect(hand(overview())).toBe(true)
    expect(grabs(handle())).toBe(true)

    const releases = [
      () => fireEvent.pointerUp(overview(), { pointerId: 1 }),
      () => fireEvent.pointerCancel(overview(), { pointerId: 1 }),
      () => fireEvent.lostPointerCapture(overview(), { pointerId: 1 }),
    ]
    for (const release of releases) {
      fireEvent.pointerDown(overview(), { clientX: 100, button: 0, pointerId: 1 })
      expect(grabbing(handle())).toBe(true)
      expect(grabbing(overview())).toBe(true)
      expect(hand(overview())).toBe(false)
      release()
      expect(grabs(handle())).toBe(true)
      expect(hand(overview())).toBe(true)
    }

    // A right-click opens the menu and pans nothing.
    fireEvent.pointerDown(overview(), { clientX: 100, button: 2, pointerId: 1 })
    expect(grabs(handle())).toBe(true)
  })
})
