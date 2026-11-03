import type * as React from "react"
import { useState } from "react"
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { AudioStream, Character, MediaInfo, MediaItem, Segment } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import type { Lane } from "@/lib/presence"
import { timecode } from "@/lib/timecode"
import { useUi } from "@/state/ui"
import { FrameStore } from "../frame-store"
import { DetailsPanel } from "./DetailsPanel"
import { PlayerKeysDialog } from "./PlayerKeysDialog"

// The router's link as a plain one that keeps where it goes; the backend and
// the app's actions as stand-ins.
vi.mock("@tanstack/react-router", () => ({
  Link: ({ to, params: _params, ...props }: React.ComponentProps<"a"> & { to: string; params?: unknown }) => <a href={to} {...props} />,
  useNavigate: () => () => {},
}))
vi.mock("@/lib/ipc", () => ({
  commands: {
    mediaAnalyze: vi.fn(() => Promise.resolve(null)),
    mediaSetAudio: vi.fn(() => Promise.resolve(null)),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("@/lib/os", () => ({ copyText: vi.fn(), reveal: vi.fn() }))
vi.mock("@/lib/actions", () => ({ useAppActions: () => ({ goSettings: vi.fn() }) }))

const FPS = { num: 24, den: 1 }
const tc = (f: number) => timecode(f, FPS, false)

/** Three shots and a dissolve; the playhead starts in the second shot. */
const SEGMENTS: Segment[] = [
  { start: 0, end: 100, kind: "shot", tiny: false },
  { start: 100, end: 110, kind: "dissolve", tiny: false },
  { start: 110, end: 300, kind: "shot", tiny: false },
  { start: 300, end: 586, kind: "shot", tiny: false },
]

const audio = (ordinal: number, language: string): AudioStream => ({
  index: ordinal + 1,
  ordinal,
  codec: "aac",
  profile: null,
  channels: 2,
  channelLayout: "stereo",
  sampleRate: 48000,
  language,
  title: null,
  default: ordinal === 0,
  commentary: false,
})

const info = (tracks: AudioStream[]) =>
  ({
    container: "mov,mp4,m4a,3gp,3g2,mj2",
    video: { width: 640, height: 360, codec: "h264", bitDepth: 8, hdr: "none", sar: { num: 1, den: 1 }, rotation: 0 },
    audio: tracks,
  }) as unknown as MediaInfo

const episode = (o: Partial<MediaItem> = {}) =>
  ({
    id: "m",
    path: "C:\\Show\\Show.S01E01.1080p.mkv",
    fileName: "Show.S01E01.1080p.mkv",
    label: "S01E01",
    shots: "ready",
    shotsError: null,
    faces: "ready",
    facesError: null,
    vfr: false,
    interlaced: false,
    audioOrdinal: null,
    info: info([audio(0, "jpn")]),
    ...o,
  }) as MediaItem

const character = (id: string): Character => ({
  id,
  name: id,
  label: id,
  hidden: false,
  tracks: 1,
  shots: 1,
  media: 1,
  screenSec: 10,
  cover: null,
  coverSmall: null,
  unsure: 0,
  waiting: 0,
})

/** Ten characters, most on screen first; C3 and C5 are in the second shot. */
const LANES: Lane[] = Array.from({ length: 10 }, (_, k) => {
  const id = `C${k + 1}`
  const inShot = id === "C3" || id === "C5"
  const span = inShot ? { start: 120, end: 130 } : { start: 10 + k, end: 20 + k }
  return { character: character(id), spans: [{ characterId: id, trackId: k, ...span }], frames: 100 - k }
})

type Props = React.ComponentProps<typeof DetailsPanel>

function props(over: Partial<Props> = {}): Props {
  const store = new FrameStore()
  store.set(150)
  return {
    open: true,
    pid: "p",
    media: episode(),
    fps: FPS,
    frames: 586,
    lanes: LANES,
    segments: SEGMENTS,
    store,
    tc,
    focus: null,
    onFocus: vi.fn(),
    onMarkShot: vi.fn(),
    onSeek: vi.fn(),
    onGo: vi.fn(),
    onKeys: vi.fn(),
    ...over,
  }
}

const show = (p: Props) => render(<DetailsPanel {...p} />, { wrapper: TooltipProvider })
const panel = () => screen.getByRole("complementary", { name: "Details" })

beforeEach(() => useUi.setState({ playerTab: "cast" }))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  localStorage.clear()
})

describe("the details panel's tab", () => {
  it("opens on the tab it remembers, and remembers the one clicked", () => {
    useUi.setState({ playerTab: "file" })
    show(props())
    expect(screen.getByText("Show.S01E01.1080p.mkv")).toBeTruthy()
    expect(screen.queryByRole("region", { name: "Cast" })).toBeNull()

    fireEvent.click(screen.getByRole("radio", { name: "Cast" }))
    expect(useUi.getState().playerTab).toBe("cast")
    expect(screen.getByRole("region", { name: "Cast" })).toBeTruthy()
    fireEvent.click(screen.getByRole("radio", { name: "File" }))
    expect(useUi.getState().playerTab).toBe("file")
    expect(screen.getByRole("region", { name: "File" })).toBeTruthy()
  })

  it("is kept with the panel's other layout choices across restarts", () => {
    const keep = useUi.persist.getOptions().partialize!(useUi.getState()) as Record<string, unknown>
    expect(keep.playerTab).toBe("cast")
    expect(keep.inspectorOpen).toBeDefined()
  })

  it("opens Cast for a stored value it doesn't know", () => {
    useUi.setState({ playerTab: "clip" as never })
    show(props())
    expect(screen.getByRole("region", { name: "Cast" })).toBeTruthy()
    expect(screen.getByRole("radio", { name: "Cast" }).getAttribute("aria-checked")).toBe("true")
  })
})

describe("the details panel's names", () => {
  it("is a panel named Details, with a radio group of its sections and a section named by its tab", () => {
    show(props())
    expect(panel().id).toBe("player-details")
    const group = screen.getByRole("radiogroup", { name: "Details sections" })
    expect(within(group).getAllByRole("radio").map((r) => r.textContent)).toEqual(["Cast", "File"])
    expect(within(panel()).getByRole("region", { name: "Cast" })).toBeTruthy()
  })

  it("opens the Player keys dialog from its header", () => {
    function Player() {
      const [keys, setKeys] = useState(false)
      return (
        <>
          <DetailsPanel {...props({ onKeys: () => setKeys(true) })} />
          <PlayerKeysDialog open={keys} onOpenChange={setKeys} />
        </>
      )
    }
    render(<Player />, { wrapper: TooltipProvider })
    expect(screen.queryByRole("dialog")).toBeNull()
    fireEvent.click(within(panel()).getByRole("button", { name: "Player keys" }))
    expect(screen.getByRole("dialog", { name: "Player keys" })).toBeTruthy()
  })
})

describe("the Cast tab", () => {
  it("starts with the shot under the playhead: its number of all, range and frames", () => {
    const p = props()
    show(p)
    const cast = screen.getByRole("region", { name: "Cast" })
    expect(within(cast).getByText("Shot 2 of 3")).toBeTruthy()
    expect(within(cast).getByText("190 frames")).toBeTruthy()
    expect(within(cast).getByText(`${tc(110)} – ${tc(299)}`)).toBeTruthy()

    fireEvent.click(within(cast).getByRole("button", { name: /Mark shot/ }))
    expect(p.onMarkShot).toHaveBeenCalledOnce()
    fireEvent.click(within(cast).getByRole("button", { name: "Previous shot" }))
    expect(p.onSeek).toHaveBeenLastCalledWith(110)
    fireEvent.click(within(cast).getByRole("button", { name: "Next shot" }))
    expect(p.onSeek).toHaveBeenLastCalledWith(300)
  })

  it("names a transition by its kind, with no Mark shot", () => {
    const p = props()
    p.store.set(105)
    show(p)
    const cast = screen.getByRole("region", { name: "Cast" })
    expect(within(cast).getByText("Dissolve")).toBeTruthy()
    expect(within(cast).queryByRole("button", { name: /Mark shot/ })).toBeNull()
  })

  it("lists who is in the shot first, then the episode's main characters, the rest on request", () => {
    show(props())
    const cast = screen.getByRole("region", { name: "Cast" })
    const rows = () => within(cast).getAllByRole("listitem").filter((li) => !/more in this episode|Show fewer/.test(li.textContent ?? ""))
    expect(rows().map((li) => li.textContent)).toEqual(["C3in shot", "C5in shot", "C1", "C2", "C4", "C6", "C7", "C8"])

    fireEvent.click(within(cast).getByRole("button", { name: "2 more in this episode" }))
    expect(rows()).toHaveLength(10)
    fireEvent.click(within(cast).getByRole("button", { name: "Show fewer" }))
    expect(rows()).toHaveLength(8)
  })

  it("links to the cast exactly once", () => {
    show(props())
    const links = within(panel()).getAllByRole("link")
    expect(links.map((a) => [a.textContent?.trim(), a.getAttribute("href")])).toEqual([["Open the cast", "/p/$pid/cast"]])
  })

  it("says so when nobody the cast knows appears", () => {
    show(props({ lanes: [] }))
    expect(screen.getByText("Nobody the cast knows appears here.")).toBeTruthy()
    expect(within(panel()).getAllByRole("link")).toHaveLength(1)
  })

  it("offers to find the shots once, where the shot would be", async () => {
    const { commands } = await import("@/lib/ipc")
    show(props({ media: episode({ shots: "pending", faces: "pending" }), segments: [] }))
    const cast = screen.getByRole("region", { name: "Cast" })
    expect(within(cast).getByText("Shots haven't been detected yet.")).toBeTruthy()
    // One place says where the analysis stands, not one per stage.
    expect(within(cast).queryByText("Characters haven't been found in this episode yet.")).toBeNull()
    fireEvent.click(within(cast).getByRole("button", { name: "Find shots" }))
    expect(commands.mediaAnalyze).toHaveBeenCalledExactlyOnceWith("p", "m", null)
  })
})

describe("the File tab", () => {
  beforeEach(() => useUi.setState({ playerTab: "file" }))

  it("tells the episode's length and frame count", () => {
    show(props())
    const file = screen.getByRole("region", { name: "File" })
    expect(within(file).getByText("Length 00:00:24:10 · 586 frames")).toBeTruthy()
    expect(within(file).getByText("640×360 · 24 fps · H.264")).toBeTruthy()
  })

  it("offers the audio track only when there is more than one", () => {
    show(props())
    expect(screen.queryByRole("combobox", { name: "Audio track" })).toBeNull()
    cleanup()
    show(props({ media: episode({ info: info([audio(0, "jpn"), audio(1, "eng")]) }) }))
    expect(screen.getByRole("combobox", { name: "Audio track" })).toBeTruthy()
  })
})

describe("the details panel", () => {
  it("has no In, Out or Export of its own: they are the transport's", () => {
    for (const tab of ["cast", "file"] as const) {
      useUi.setState({ playerTab: tab })
      show(props())
      for (const name of [/^Mark in/, /^Mark out/, /^Export/]) expect(within(panel()).queryByRole("button", { name })).toBeNull()
      cleanup()
    }
  })

  it("fades in when it is opened, not when the player mounts with it open", () => {
    const p = props()
    const { rerender } = show(p)
    expect(panel().className).not.toContain("animate-fade-in")
    rerender(<DetailsPanel {...p} open={false} />)
    expect(screen.queryByRole("complementary")).toBeNull()
    rerender(<DetailsPanel {...p} open />)
    expect(panel().className).toContain("motion-safe:animate-fade-in")
  })

  it("fades in when the player mounted with it closed", () => {
    const p = props({ open: false })
    const { rerender } = show(p)
    expect(screen.queryByRole("complementary")).toBeNull()
    rerender(<DetailsPanel {...p} open />)
    expect(panel().className).toContain("motion-safe:animate-fade-in")
  })
})
