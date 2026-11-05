import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { FrameCount, JobInfo, MediaItem } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { useActivity } from "@/state/activity"
import { inLanguage } from "@/test/i18n"
import { FrameStore } from "./frame-store"
import { NotReady } from "./NotReady"
import { CastTab } from "./panels/CastTab"
import { FileTab } from "./panels/FileTab"

vi.mock("@/lib/ipc", async (original) => ({
  ...(await original<typeof import("@/lib/ipc")>()),
  commands: { jobCancel: vi.fn(() => Promise.resolve(null)), mediaIndexingStill: vi.fn(() => Promise.resolve(null)) },
}))
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => vi.fn(),
  Link: ({ children, ...rest }: { children: React.ReactNode }) => <a {...(rest as object)}>{children}</a>,
}))

afterEach(() => {
  cleanup()
  useActivity.setState({ jobs: {}, order: [] })
})

const job = (o: Partial<JobInfo>): JobInfo => ({
  id: "ingest",
  kind: "ingest",
  title: "Indexing E01.mkv",
  subtitle: null,
  projectId: "p",
  mediaId: "m",
  exportId: null,
  state: "running",
  progress: 0.4,
  stage: "Indexing frames",
  etaMs: null,
  error: null,
  createdAt: 1,
  finishedAt: null,
  rev: 1,
  ...o,
})
const episode = (o: Partial<MediaItem>) => ({ id: "m", path: "C:\\Show\\E01.mkv", poster: null, ...o }) as MediaItem
const notReady = (media: MediaItem) =>
  render(<NotReady pid="p" media={media} error={null} gone={false} listError={null} onRetryList={() => {}} />)

const RO = {
  common: { state: { waiting: "Se așteaptă…" }, action: { tryAgain: "Încearcă din nou" } },
  player: {
    notReady: {
      indexingTitle: "Se indexează",
      missingBody: "{{path}} nu mai este acolo.",
      framesOfAbout_one: "<x>{{done}}</x> din aproximativ <mono>{{count}}</mono> cadru",
      framesOfAbout_few: "<x>{{done}}</x> din aproximativ <mono>{{count}}</mono> cadre",
      framesOfAbout_other: "<x>{{done}}</x> din aproximativ <mono>{{count}}</mono> de cadre",
      framesRead_other: "<x>{{count}}</x> de cadre citite",
      measuring: "Se măsoară viteza…",
      pace: "<mono>{{pace}}</mono> cadre/s",
      paceReal: "<mono>{{pace}}</mono> cadre/s · <mono>{{speed}}×</mono> viteza reală",
    },
  },
}

describe("the player in another language", () => {
  it("counts the frames read with the file's count in the plural form and the digits of the language, numbers in the timecode font", async () => {
    await inLanguage("ro", RO)
    const frames = (o: Partial<FrameCount>): FrameCount => ({ done: 14_200, total: 88_500, rate: 24, perSec: null, ...o })
    useActivity.getState().replaceAll([job({ frames: frames({}) })])
    const { container } = notReady(episode({ status: "indexing" }))
    expect(screen.getByText("Se indexează")).toBeTruthy()
    expect(container.textContent).toContain("14.200 din aproximativ 88.500 de cadre")
    expect(screen.getByText("14.200").className).toContain("timecode")
    expect(screen.getByText("Se măsoară viteza…")).toBeTruthy()
    cleanup()
    useActivity.getState().replaceAll([job({ frames: frames({ total: 3, done: 2 }) })])
    expect(notReady(episode({ status: "indexing" })).container.textContent).toContain("2 din aproximativ 3 cadre")
    cleanup()
    useActivity.getState().replaceAll([job({ frames: frames({ done: 90_100, perSec: 600 }) })])
    const read = notReady(episode({ status: "indexing" })).container
    expect(read.textContent).toContain("90.100 de cadre citite")
    expect(read.textContent).toContain("600 cadre/s · 25× viteza reală")
  })

  it("says a missing file's path inside the sentence, as the person has it", async () => {
    await inLanguage("ro", RO)
    notReady(episode({ status: "missing" }))
    expect(screen.getByText("C:\\Show\\E01.mkv nu mai este acolo.")).toBeTruthy()
  })

  it("says Waiting in the language while the job waits its turn", async () => {
    await inLanguage("ro", RO)
    useActivity.getState().replaceAll([job({ state: "queued" })])
    notReady(episode({ status: "indexing" }))
    expect(screen.getByText("Se așteaptă…")).toBeTruthy()
  })

  it("names where the analysis stands and what to press, in whole sentences", async () => {
    await inLanguage("ro", {
      ...RO,
      player: {
        details: { cast: "Distribuția" },
        castTab: { shotsFailed: "Detectarea cadrelor a eșuat.", shotsAgain: "Caută din nou cadrele", findCharacters: "Caută personajele" },
      },
    })
    const store = new FrameStore()
    const view = (m: Partial<MediaItem>) =>
      render(
        <TooltipProvider>
          <CastTab
            pid="p"
            media={episode({ status: "ready", ...m })}
            lanes={[]}
            segments={[]}
            store={store}
            tc={String}
            focus={null}
            onFocus={() => {}}
            onMarkShot={() => {}}
            onSeek={() => {}}
            onGo={() => {}}
          />
        </TooltipProvider>,
      )
    view({ shots: "failed", shotsError: { key: "", args: {}, text: "Out of memory" }, faces: "pending" })
    expect(screen.getByRole("region", { name: "Distribuția" })).toBeTruthy()
    expect(screen.getByText("Detectarea cadrelor a eșuat.")).toBeTruthy()
    // The reason the app gave stays as it was said.
    expect(screen.getByText("Out of memory")).toBeTruthy()
    expect(screen.getByRole("button", { name: "Încearcă din nou" })).toBeTruthy()
    cleanup()
    view({ shots: "ready", faces: "pending" })
    expect(screen.getByRole("button", { name: "Caută personajele" })).toBeTruthy()
  })

  it("writes the file's facts in the language: the length with its plural, the picture as a whole line", async () => {
    await inLanguage("ro", {
      player: {
        file: {
          length_one: "Durată {{time}} · {{count}} cadru",
          length_few: "Durată {{time}} · {{count}} cadre",
          length_other: "Durată {{time}} · {{count}} de cadre",
          video: "{{width}}×{{height}} · {{fps}} cadre/s · {{codec}}",
        },
      },
    })
    const info = { video: { width: 1920, height: 1080, codec: "h264" }, audio: [] } as unknown as MediaItem["info"]
    render(
      <FileTab pid="p" media={episode({ status: "ready", fileName: "E01.mkv", info })} fps={{ num: 24000, den: 1001 }} frames={34_000} tc={() => "00:23:37:16"} />,
    )
    expect(screen.getByText("Durată 00:23:37:16 · 34.000 de cadre")).toBeTruthy()
    expect(screen.getByText(/^1920×1080 · .* cadre\/s · /)).toBeTruthy()
  })
})
