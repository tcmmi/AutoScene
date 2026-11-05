import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { Character, Presence, Segment } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { FrameClock } from "@/lib/frame-clock"
import { i18n } from "@/lib/i18n"
import { timecode } from "@/lib/timecode"
import { inLanguage } from "@/test/i18n"
import { FrameStore } from "./frame-store"
import { ShotHeader } from "./panels/ShotHeader"
import { useTimecodeMode } from "./timecode-mode"
import { Timeline } from "./timeline/Timeline"
import { Transport } from "./Transport"
import { characterName, oneSidedNotice, percentText, segmentLabel, withHole } from "./words"
import readySource from "./ReadyPlayer.tsx?raw"
import transportSource from "./Transport.tsx?raw"

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => vi.fn(),
  useCanGoBack: () => false,
}))

const tc = (f: number) => timecode(f, { num: 24, den: 1 }, false)

beforeEach(() => {
  useTimecodeMode.setState({ mode: "elapsed" })
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(1482)
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: 1000,
    bottom: 24,
    width: 1000,
    height: 24,
    toJSON: () => ({}),
  } as DOMRect)
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  useTimecodeMode.setState({ mode: "elapsed" })
})

function transport(store: FrameStore, over: Partial<React.ComponentProps<typeof Transport>> = {}) {
  render(
    <TooltipProvider delayDuration={0}>
      <Transport
        store={store}
        last={585}
        rate={0}
        tc={tc}
        parse={() => null}
        goto={false}
        onGoto={vi.fn()}
        onSeek={vi.fn()}
        onStep={vi.fn()}
        onToggle={vi.fn()}
        inFrame={null}
        outFrame={null}
        length={586}
        onMarkIn={vi.fn()}
        onMarkOut={vi.fn()}
        onClear={vi.fn()}
        canExport={false}
        onExport={vi.fn()}
        volume={1}
        muted={false}
        onVolume={vi.fn()}
        onToggleMute={vi.fn()}
        {...over}
      />
    </TooltipProvider>,
  )
}

/** Every `t()` call, counted where the translator is, whichever way it was reached. */
const translations = () =>
  vi.spyOn((i18n as unknown as { translator: { translate: (...args: unknown[]) => unknown } }).translator, "translate")

describe("the marks of a clip", () => {
  it("are whole sentences, one for in and one for out, so a language can inflect each", async () => {
    await inLanguage("ro", {
      player: {
        transport: {
          in: "Început",
          out: "Sfârșit",
          markIn: "Marchează începutul",
          markOut: "Marchează sfârșitul",
          goToInAt: "Mergi la începutul clipului, {{time}}",
          goToOutAt: "Mergi la sfârșitul clipului, {{time}}",
        },
      },
    })
    transport(new FrameStore())
    expect(screen.getByRole("button", { name: "Marchează începutul" }).textContent).toBe("Început")
    expect(screen.getByRole("button", { name: "Marchează sfârșitul" }).textContent).toBe("Sfârșit")
    cleanup()
    transport(new FrameStore(), { inFrame: 48, outFrame: 157, length: 110, canExport: true })
    expect(screen.getByRole("button", { name: "Mergi la începutul clipului, 00:00:02:00" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Mergi la sfârșitul clipului, 00:00:06:13" })).toBeTruthy()
  })

  it("never build a sentence out of a lower-cased word", () => {
    for (const source of [transportSource, readySource]) expect(source).not.toMatch(/toLowerCase|toLocaleLowerCase/)
  })
})

describe("what draws with every frame", () => {
  const readout = () => document.querySelector("[data-readout]") as HTMLElement

  for (const mode of ["elapsed", "remaining", "frames", "percent"] as const) {
    it(`says no word while the episode plays in ${mode} mode, and speaks the new language after a switch`, async () => {
      useTimecodeMode.setState({ mode })
      const store = new FrameStore()
      transport(store)
      const spy = translations()
      // A second of playing, a frame at a time, with the counter beside the readout.
      act(() => {
        for (let f = 1; f <= 24; f++) store.set(f)
      })
      expect(spy).not.toHaveBeenCalled()
      const name = { elapsed: "Elapsed Timecode", remaining: "Remaining Timecode", frames: "Current Frame", percent: "Percentage" }[mode]
      expect(readout().getAttribute("aria-label")).toBe(`${name}: ${readout().textContent}`)

      spy.mockRestore()
      await act(() =>
        inLanguage("ro", {
          player: {
            percent: "{{value}} la sută",
            readout: {
              elapsed: "Cod de timp scurs",
              remaining: "Cod de timp rămas",
              currentFrame: "Cadrul curent",
              percentage: "Procent",
              aria: "{{label}} — {{value}}",
            },
          },
        }),
      )
      const ro = translations()
      const label = { elapsed: "Cod de timp scurs", remaining: "Cod de timp rămas", frames: "Cadrul curent", percent: "Procent" }[mode]
      expect(readout().getAttribute("aria-label")).toBe(`${label} — ${readout().textContent}`)
      if (mode === "percent") expect(readout().textContent).toBe("4,1 la sută")
      act(() => {
        for (let f = 25; f <= 48; f++) store.set(f)
      })
      expect(ro).not.toHaveBeenCalled()
      expect(readout().getAttribute("aria-label")).toBe(`${label} — ${readout().textContent}`)
    })
  }

  it("keeps the timeline's playhead and overview words out of the frame loop, too", async () => {
    const store = new FrameStore()
    const character = { id: "c1", name: "Walter", label: "Walter", number: 1, hidden: false } as unknown as Character
    const span = { characterId: "c1", trackId: 1, start: 10, end: 120, body: true } as unknown as Presence
    const mount = () =>
      render(
        <TooltipProvider>
          <Timeline
            clock={new FrameClock({ frames: 240, fps: { num: 24, den: 1 }, offset: 0, step: 1 / 24, times: null })}
            fps={24}
            counting={{ count: (f) => f, frame: (n) => n }}
            store={store}
            segments={[{ start: 0, end: 240, kind: "shot", tiny: false } as Segment]}
            lanes={[{ character, spans: [span], frames: 110 }]}
            focus={null}
            inFrame={null}
            outFrame={null}
            tc={(f) => String(f)}
            onSeek={() => {}}
            onScrub={() => {}}
            onMarkRange={() => {}}
            onMarkIn={() => {}}
            onMarkOut={() => {}}
            onFocus={() => {}}
            onOpenCharacter={() => {}}
          />
        </TooltipProvider>,
      )
    mount()
    // The tips over a shot and over a character's lane are open while it plays.
    for (const track of Array.from(document.querySelectorAll<HTMLElement>("div[class~='h-[20px]']"))) fireEvent.pointerMove(track, { clientX: 100 })
    expect(screen.getByText("no face", { exact: false })).toBeTruthy()
    const spy = translations()
    act(() => {
      for (let f = 1; f <= 60; f++) store.set(f)
    })
    expect(spy).not.toHaveBeenCalled()
    expect(screen.getByRole("slider", { name: "Playhead" })).toBeTruthy()
    spy.mockRestore()
    await act(() => inLanguage("ro", { player: { timeline: { playhead: "Capul de redare" } } }))
    expect(screen.getByRole("slider", { name: "Capul de redare" })).toBeTruthy()
    // Out of the frame loop, but ready when the menu opens.
    fireEvent.contextMenu(screen.getByRole("slider", { name: "Capul de redare" }))
  })

  it("fails on a message that holds the value twice or not at all, and fills one that holds it once", () => {
    expect(() => withHole(() => "no value here")).toThrow(/exactly once/)
    expect(() => withHole((h) => `${h} and ${h}`)).toThrow(/exactly once/)
    expect(withHole((h) => `${h} %`)("5")).toBe("5 %")
  })

  it("holds the value once in the catalog's per-frame messages, and sees words added after the switch", async () => {
    expect(percentText(12.3)).toBe("12.3 %")
    await inLanguage("ro", { player: { percent: "{{value}} la sută" } })
    expect(percentText(12.3)).toBe("12,3 la sută")
    i18n.addResourceBundle("ro", "player", { percent: "{{value}} procente" }, true, true)
    expect(percentText(12.3)).toBe("12,3 procente")
  })

  it("passes the transport's tier the language, so a switch measures the row again", () => {
    expect(transportSource).toContain("useTransportTier(row, [hasMarks, goto, mode, i18n.language], labelExtra())")
  })

  it("fills a message's hole without asking the catalog", () => {
    const say = vi.fn((hole: string) => `Cadrul ${hole} din 585`)
    const fill = withHole(say)
    expect([fill("12"), fill("13"), fill("14")]).toEqual(["Cadrul 12 din 585", "Cadrul 13 din 585", "Cadrul 14 din 585"])
    expect(say).toHaveBeenCalledTimes(1)
  })
})

describe("the export notice for one mark", () => {
  it("names the key of the other end, or says to mark it when it has none", () => {
    const slots: number[] = []
    const key = (slot: number) => {
      slots.push(slot)
      return slot === 1 ? "O" : "I"
    }
    // Only the in point is marked: the person needs the key that marks the out point (slot 1).
    expect(oneSidedNotice(10, null, key)).toContain("press O where it should stop")
    // Only the out point is marked: the key that marks the in point (slot 0).
    expect(oneSidedNotice(null, 20, key)).toContain("press I where it should begin")
    expect(slots).toEqual([1, 0])
    expect(oneSidedNotice(10, null, () => null)).toContain("mark the out point where it should stop")
    expect(oneSidedNotice(null, 20, () => null)).toContain("mark the in point where it should begin")
    expect(oneSidedNotice(10, 20, key)).toBeUndefined()
    expect(oneSidedNotice(null, null, key)).toBeUndefined()
  })
})

describe("words the player makes itself", () => {
  it("name an unnamed character from their number, in the UI's language, and keep a given name as it is", async () => {
    const unnamed = { name: null, number: 12 } as Pick<Character, "name" | "number">
    expect(characterName(unnamed)).toBe("Character 12")
    expect(characterName({ name: "Walter", number: 1 })).toBe("Walter")
    await inLanguage("ro", { player: { character: { unnamed: "Personajul {{number}}" } } })
    expect(characterName(unnamed)).toBe("Personajul 12")
    expect(characterName({ name: "Walter", number: 1 })).toBe("Walter")
  })

  it("name the kinds of stretch the shots come in", async () => {
    expect((["shot", "dissolve", "fade_out", "fade_in", "black", "flash"] as const).map(segmentLabel)).toEqual([
      "Shot",
      "Dissolve",
      "Fade out",
      "Fade in",
      "Black",
      "Flash",
    ])
    await inLanguage("ro", { player: { segment: { fadeOut: "Estompare spre negru" } } })
    expect(segmentLabel("fade_out")).toBe("Estompare spre negru")
  })

  it("count frames with the language's plural forms, and write a shot's length plain", async () => {
    const seg = (start: number, end: number): Segment => ({ start, end, kind: "shot", tiny: false })
    const header = (s: Segment) =>
      render(
        <TooltipProvider>
          <ShotHeader seg={s} number={0} total={3} tc={tc} onMark={() => {}} onPrev={() => {}} onNext={() => {}} />
        </TooltipProvider>,
      )
    await inLanguage("ro", {
      player: { shot: { of: "Cadrul {{number}} din {{total}}", length_one: "{{length}} cadru", length_few: "{{length}} cadre", length_other: "{{length}} de cadre" } },
    })
    const lengths = [] as string[]
    for (const [a, b] of [[0, 1], [0, 3], [0, 20], [0, 1200]]) {
      header(seg(a, b))
      lengths.push(screen.getByText(/cadr/, { selector: "span.timecode" }).textContent ?? "")
      cleanup()
    }
    expect(lengths).toEqual(["1 cadru", "3 cadre", "20 de cadre", "1200 de cadre"])
    header(seg(0, 5))
    expect(screen.getByText("Cadrul 1 din 3")).toBeTruthy()
  })
})
