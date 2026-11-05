import { QueryClientProvider } from "@tanstack/react-query"
import userEvent from "@testing-library/user-event"
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { AudioStream, JobInfo, MediaItem } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { reveal } from "@/lib/os"
import { queryClient } from "@/lib/queries"
import { useActivity } from "@/state/activity"
import { EpisodeRow } from "./EpisodesPage"

vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return {
    ...ipc,
    commands: {
      ...ipc.commands,
      settingsGet: vi.fn(() => Promise.resolve({ findBodies: true })),
      projectGet: vi.fn(() => Promise.resolve({ id: "p", fastScan: false })),
      mediaAnalyze: vi.fn(() => Promise.resolve(null)),
      mediaRetry: vi.fn(() => Promise.resolve(null)),
      mediaSetAudio: vi.fn(() => Promise.resolve(null)),
      jobCancel: vi.fn(() => Promise.resolve(null)),
    },
  }
})
vi.mock("@/lib/os", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/os")>()), reveal: vi.fn() }))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)

afterEach(() => {
  cleanup()
  queryClient.clear()
  vi.clearAllMocks()
  useActivity.setState({ jobs: {}, order: [] })
})

const track = (ordinal: number, language: string): AudioStream =>
  ({
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
  }) as AudioStream

/** An episode of the project "p" with the given audio tracks. */
function episode(o: Partial<MediaItem> = {}, audio: AudioStream[] = []): MediaItem {
  return {
    id: "m",
    path: "C:\\Shows\\Show.S01E01.mkv",
    fileName: "Show.S01E01.mkv",
    label: "S01E01",
    status: "ready",
    sizeBytes: 1_000_000,
    durationSec: 2700,
    info: { container: "matroska", video: { width: 1920, height: 1080, codec: "h264", bitDepth: 8 }, audio, subtitles: [] },
    poster: null,
    posterSmall: null,
    audioOrdinal: null,
    shots: "ready",
    shotCount: 12,
    faces: "ready",
    faceCount: 30,
    bodies: "ready",
    ...o,
  } as MediaItem
}

function show(m: MediaItem) {
  const onOpen = vi.fn()
  render(
    <ul>
      <EpisodeRow media={m} ambiguous={false} projectId="p" onOpen={onOpen} onRemove={() => {}} />
    </ul>,
    { wrapper },
  )
  return { onOpen, row: screen.getByRole("listitem") }
}

const hasHand = (el: HTMLElement) => /(^|\s)cursor-pointer(\s|$)/.test(el.className)

describe("an episode's row", () => {
  it("a ready episode opens from its title and from an empty part of its audio cell", () => {
    const { onOpen, row } = show(episode())
    expect(hasHand(row)).toBe(true)
    fireEvent.click(screen.getByText("Show"))
    expect(onOpen).toHaveBeenCalledTimes(1)
    // "No audio" sits in a cell of its own: the part beside it opens the episode too.
    fireEvent.click(screen.getByText("No audio").parentElement!)
    expect(onOpen).toHaveBeenCalledTimes(2)
    fireEvent.click(row)
    expect(onOpen).toHaveBeenCalledTimes(3)
  })

  it("its audio menu and the track picked in it, its state buttons, ⋯ and a ⋯ entry don't open it", async () => {
    // Finding the shots keeps "Find shots again" off: a disabled entry, which a click still reaches.
    const m = episode({ shots: "running" }, [track(0, "eng"), track(1, "jpn")])
    useActivity.setState({
      jobs: { j: { id: "j", kind: "analyze", projectId: "p", mediaId: "m", state: "running", progress: 0.2, stage: "Finding shots", rev: 1 } as JobInfo },
      order: ["j"],
    })
    const user = userEvent.setup()
    const { onOpen } = show(m)

    // The audio menu and the track picked in it (its options are in a portal).
    await user.click(screen.getByRole("combobox", { name: "Audio track" }))
    await user.click(await screen.findByRole("option", { name: /Japanese/ }))
    expect(commands.mediaSetAudio).toHaveBeenCalledWith("p", "m", 1)

    // A state button.
    fireEvent.click(screen.getAllByRole("button", { name: "Cancel analysis" })[0])
    expect(commands.jobCancel).toHaveBeenCalled()

    // ⋯ and its entries: the working one and the disabled one.
    await user.click(screen.getByRole("button", { name: "Show.S01E01.mkv options" }))
    const disabled = await screen.findByRole("menuitem", { name: /Find shots again/ })
    expect(disabled.hasAttribute("data-disabled")).toBe(true)
    fireEvent.click(disabled)
    await user.click(screen.getByRole("menuitem", { name: /Show in folder/ }))
    expect(reveal).toHaveBeenCalledWith("C:\\Shows\\Show.S01E01.mkv")
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull())

    expect(onOpen).not.toHaveBeenCalled()
    // The row itself still opens it.
    fireEvent.click(screen.getByText("Show"))
    expect(onOpen).toHaveBeenCalledTimes(1)
  })

  it("a press that began on a state button doesn't open the episode, wherever it is released", () => {
    // Finding the shots hasn't run: the state is "Indexed" and a plain "Find shots" button.
    const { onOpen, row } = show(episode({ shots: "pending" }))
    const button = screen.getAllByRole("button", { name: /Find shots/ })[0]
    // Slid off the button and released on the cell around it, or on the row: the browser
    // sends the click to the nearest element that holds both ends of the press.
    fireEvent.pointerDown(button)
    fireEvent.click(button.parentElement!)
    fireEvent.pointerDown(button)
    fireEvent.click(row)
    expect(onOpen).not.toHaveBeenCalled()
    // A press that began on the cell opens it, and a click on the button alone (no stopPropagation
    // of its own: the row is the only guard) doesn't.
    fireEvent.pointerDown(screen.getByText("Show"))
    fireEvent.click(row)
    expect(onOpen).toHaveBeenCalledTimes(1)
    fireEvent.click(button)
    expect(onOpen).toHaveBeenCalledTimes(1)
    expect(commands.mediaAnalyze).toHaveBeenCalledTimes(1)
  })

  it("an episode that isn't ready opens nothing and shows no hand", () => {
    const { onOpen, row } = show(episode({ status: "failed", error: { key: "", args: {}, text: "Unreadable" } }))
    expect(hasHand(row)).toBe(false)
    expect(row.hasAttribute("tabindex")).toBe(false)
    fireEvent.click(screen.getByText("Show"))
    fireEvent.click(screen.getByText("No audio").parentElement!)
    fireEvent.click(row)
    fireEvent.keyDown(row, { key: "Enter" })
    expect(onOpen).not.toHaveBeenCalled()
  })
})
