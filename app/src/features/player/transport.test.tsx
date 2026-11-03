import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { AppMenu } from "@/components/shell/AppMenu"
import { ContextArea } from "@/components/ui/context-area"
import { ContextMenuItem } from "@/components/ui/context-menu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { timecode } from "@/lib/timecode"
import { FrameStore } from "./frame-store"
import { useTimecodeMode } from "./timecode-mode"
import { Transport } from "./Transport"

// The window's menu has entries that navigate; here only the order matters.
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => vi.fn(),
  useCanGoBack: () => false,
}))

const tc = (f: number) => timecode(f, { num: 24, den: 1 }, false)
/** The row's width, which picks its tier. */
let width = 1482

beforeEach(() => {
  width = 1482
  useTimecodeMode.setState({ mode: "elapsed" })
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => width)
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  document.body.style.pointerEvents = ""
})

type Props = React.ComponentProps<typeof Transport>

function props(over: Partial<Props> = {}): Props {
  return {
    store: new FrameStore(),
    last: 585,
    rate: 0,
    tc,
    parse: () => null,
    goto: false,
    onGoto: vi.fn(),
    onSeek: vi.fn(),
    onStep: vi.fn(),
    onToggle: vi.fn(),
    inFrame: null,
    outFrame: null,
    length: 586,
    onMarkIn: vi.fn(),
    onMarkOut: vi.fn(),
    onClear: vi.fn(),
    canExport: false,
    onExport: vi.fn(),
    volume: 1,
    muted: false,
    onVolume: vi.fn(),
    onToggleMute: vi.fn(),
    ...over,
  }
}

/** In 00:00:02:00, out 00:00:06:13: 110 frames. */
const MARKED = { inFrame: 48, outFrame: 157, length: 110, canExport: true }

function show(p: Props) {
  render(
    <TooltipProvider delayDuration={0}>
      <Transport {...p} />
    </TooltipProvider>,
  )
}

/** The tooltip's text, opened by focusing what it is about. */
function tip(el: HTMLElement): string {
  fireEvent.focus(el)
  return screen.getByRole("tooltip").textContent ?? ""
}

describe("the transport", () => {
  it("without marks: In and Out mark, Export waits and says why, no length or Clear", () => {
    const p = props()
    show(p)
    expect(document.querySelector("[data-transport]")?.getAttribute("data-tier")).toBe("1")
    expect(screen.getAllByRole("button", { name: "Mark in" })).toHaveLength(1)
    expect(screen.getAllByRole("button", { name: "Mark out" })).toHaveLength(1)
    fireEvent.click(screen.getByRole("button", { name: "Mark in" }))
    expect(p.onMarkIn).toHaveBeenCalled()
    const exp = screen.getByRole("button", { name: "Export clip" })
    expect((exp as HTMLButtonElement).disabled).toBe(true)
    expect(tip(exp.parentElement!)).toContain("Mark an in or out point, or a whole shot (X)")
    expect(screen.queryByRole("button", { name: "Clear in and out" })).toBeNull()
    expect(screen.queryByText(tc(586))).toBeNull()
  })

  it("with marks on the widest row: the chips show their timecodes and go there; length, Clear and Export", () => {
    const p = props(MARKED)
    show(p)
    const inChip = screen.getByRole("button", { name: "Go to the in point, 00:00:02:00" })
    expect(inChip.textContent).toContain("00:00:02:00")
    expect(screen.getByRole("button", { name: "Go to the out point, 00:00:06:13" }).textContent).toContain("00:00:06:13")
    fireEvent.click(inChip)
    expect(p.onSeek).toHaveBeenCalledWith(48)
    // The length reads as a length (its |<->| mark), not as a third position.
    expect(screen.getByText(tc(110)).querySelector("svg")).toBeTruthy()
    fireEvent.click(screen.getByRole("button", { name: "Clear in and out" }))
    expect(p.onClear).toHaveBeenCalled()
    const exp = screen.getByRole("button", { name: "Export clip" })
    expect(exp.textContent).toContain("Export")
    fireEvent.click(exp)
    expect(p.onExport).toHaveBeenCalled()
  })

  it("with marks at 1002 px: In and Out by name only, the timecode in the tooltip; length and a labelled Export", () => {
    width = 1002
    show(props(MARKED))
    expect(document.querySelector("[data-transport]")?.getAttribute("data-tier")).toBe("2")
    const inChip = screen.getByRole("button", { name: "Go to the in point, 00:00:02:00" })
    expect(inChip.textContent).toBe("In")
    expect(screen.getByRole("button", { name: "Go to the out point, 00:00:06:13" }).textContent).toBe("Out")
    expect(tip(inChip)).toContain("00:00:02:00")
    expect(screen.getByText(tc(110)).querySelector("svg")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Export clip" }).textContent).toContain("Export")
    expect(screen.getByRole("slider", { name: "Volume" })).toBeTruthy()
  })

  it("at the narrowest: no volume, no Clear, no length; Export is an icon", () => {
    width = 586
    show(props(MARKED))
    expect(document.querySelector("[data-transport]")?.getAttribute("data-tier")).toBe("4")
    expect(screen.queryByRole("button", { name: "Mute" })).toBeNull()
    expect(screen.queryByRole("slider", { name: "Volume" })).toBeNull()
    expect(screen.queryByRole("button", { name: "Clear in and out" })).toBeNull()
    expect(screen.queryByText(tc(110))).toBeNull()
    const exp = screen.getByRole("button", { name: "Export clip" })
    expect(exp.textContent).toBe("")
    expect(screen.getByRole("button", { name: "Play" })).toBeTruthy()
  })

  it("shows no length while nothing is marked, though it's passed (the whole episode then)", () => {
    for (const w of [1482, 1002]) {
      width = w
      show(props({ length: 586 }))
      expect(screen.queryByText(tc(586))).toBeNull()
      cleanup()
    }
  })

  it("keeps the speed's slot: the speed shows in it while shuttling, hidden otherwise", () => {
    show(props({ rate: 2 }))
    const slot = document.querySelector("[data-speed]")!
    expect(slot.textContent).toBe("2×")
    expect(slot.getAttribute("aria-hidden")).toBeNull()
    expect(slot.className).not.toContain("invisible")
    cleanup()
    show(props({ rate: 1 }))
    const idle = document.querySelector("[data-speed]")!
    expect(idle.getAttribute("aria-hidden")).toBe("true")
    expect(idle.className).toContain("invisible")
  })

  it("gives the readout and the counter one width each for the episode", () => {
    show(props({ last: 172_799 }))
    expect((document.querySelector("[data-readout]") as HTMLElement).style.minWidth).toBe("calc(11ch + 14px)")
    expect((document.querySelector("[data-counter]") as HTMLElement).style.minWidth).toBe("17ch")
  })

  it("with Go to open: the field, and neither the readout nor the counter", () => {
    show(props({ goto: true }))
    expect(screen.getByRole("textbox", { name: "Go to timecode or frame" })).toBeTruthy()
    expect(document.querySelector("[data-readout]")).toBeNull()
    expect(document.querySelector("[data-counter]")).toBeNull()
    expect(document.querySelector("[data-speed]")).toBeNull()
  })

  describe("right-clicked", () => {
    function inPlayer(p: Props) {
      render(
        <TooltipProvider delayDuration={0}>
          <AppMenu>
            <div>
              <ContextArea
                menu={
                  <>
                    <ContextMenuItem>Play</ContextMenuItem>
                    <ContextMenuItem>Mark in</ContextMenuItem>
                    <ContextMenuItem>Mark out</ContextMenuItem>
                  </>
                }
              >
                <div>
                  <Transport {...p} />
                </div>
              </ContextArea>
            </div>
          </AppMenu>
        </TooltipProvider>,
      )
    }

    const entries = () =>
      within(screen.getByRole("menu"))
        .getAllByRole("menuitem")
        .map((item) => {
          const label = item.cloneNode(true) as HTMLElement
          for (const keys of label.querySelectorAll(".timecode")) keys.remove()
          return label.textContent?.trim() ?? ""
        })

    it("a set chip offers going to its point first, then the player's entries once each", async () => {
      inPlayer(props({ ...MARKED, outFrame: null }))
      fireEvent.contextMenu(screen.getByRole("button", { name: "Go to the in point, 00:00:02:00" }), { clientX: 40, clientY: 40 })
      await screen.findByRole("menu")
      const list = entries()
      expect(list[0]).toBe("Go to the in point")
      expect(list[1]).toBe("Play")
      expect(list.filter((e) => e === "Mark in")).toHaveLength(1)
      expect(list.filter((e) => e === "Mark out")).toHaveLength(1)
    })

    it("a chip without a mark adds nothing", async () => {
      inPlayer(props({ ...MARKED, outFrame: null }))
      fireEvent.contextMenu(screen.getByRole("button", { name: "Mark out" }), { clientX: 40, clientY: 40 })
      await screen.findByRole("menu")
      const list = entries()
      expect(list[0]).toBe("Play")
      expect(list.some((e) => e.startsWith("Go to"))).toBe(false)
    })
  })
})
