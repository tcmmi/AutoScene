import type * as React from "react"
import { cleanup, fireEvent, render, screen } from "@testing-library/react"
import { toast } from "sonner"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { JobInfo, MediaItem } from "@/bindings"
import type { MenuKit } from "@/components/ui/menu-actions"
import { commands } from "@/lib/ipc"
import { useActivity } from "@/state/activity"
import { EpisodeItems } from "./episode-actions"

vi.mock("@/lib/ipc", () => ({
  commands: {
    mediaRetry: vi.fn(),
    mediaAnalyze: vi.fn(() => Promise.resolve(null)),
    jobCancel: vi.fn(() => Promise.resolve(null)),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))

type ItemProps = { onSelect?: () => void; disabled?: boolean; children: React.ReactNode }

/** The items as plain buttons; a disabled item is a disabled button. */
const kit: MenuKit = {
  Item: ({ onSelect, disabled, children }: ItemProps) => (
    <button disabled={disabled} onClick={onSelect}>
      {children}
    </button>
  ),
  Separator: () => null,
}

/** An unfinished job of the episode "m". */
const job = (o: Partial<JobInfo>): JobInfo => ({
  id: "j",
  kind: "analyze",
  title: "Analysing E01.mkv",
  subtitle: null,
  projectId: "p",
  mediaId: "m",
  exportId: null,
  state: "running",
  progress: 0.4,
  stage: "Finding shots",
  etaMs: null,
  error: null,
  createdAt: 1,
  finishedAt: null,
  rev: 1,
  ...o,
})

const analysed = { id: "m", path: "C:\\Show\\E01.mkv", status: "ready", shots: "ready", faces: "ready" } as MediaItem
/** An indexed episode whose stages stand as given. */
const episode = (o: Partial<MediaItem>) => ({ ...analysed, ...o }) as MediaItem
const items = (m: MediaItem, onRemove = () => {}) =>
  render(<EpisodeItems kit={kit} projectId="p" media={m} onOpen={() => {}} onRemove={onRemove} />)
/** The item named so, or null when it isn't offered. */
const item = (name: RegExp) => screen.queryByRole("button", { name }) as HTMLButtonElement | null
/** How an item is offered. */
const look = (b: HTMLButtonElement | null) => (b == null ? "left out" : b.disabled ? "disabled" : "enabled")

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useActivity.setState({ jobs: {}, order: [] })
})

describe("an episode's actions", () => {
  it("says so when indexing a failed episode again can't start", async () => {
    vi.mocked(commands.mediaRetry).mockRejectedValue("database is locked")
    const m = { id: "m", path: "C:\\Show\\E01.mkv", status: "failed", shots: "pending" } as MediaItem
    render(<EpisodeItems kit={kit} projectId="p" media={m} onOpen={() => {}} onRemove={() => {}} />)
    fireEvent.click(screen.getByRole("button", { name: /Index again/ }))
    expect(commands.mediaRetry).toHaveBeenCalledWith("p", "m")
    await vi.waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Couldn't index it again", { description: "database is locked" }),
    )
  })

  it("cancels the analysis it names, not the preview built beside it", () => {
    // The preview's job is the newer one: what is cancelled is picked by kind.
    useActivity.getState().replaceAll([
      job({ id: "analysis", kind: "analyze", createdAt: 1 }),
      job({ id: "preview", kind: "proxy", stage: "Preparing preview", createdAt: 2, rev: 2 }),
    ])
    render(<EpisodeItems kit={kit} projectId="p" media={analysed} onOpen={() => {}} onRemove={() => {}} />)
    fireEvent.click(screen.getByRole("button", { name: /Cancel analysis/ }))
    expect(commands.jobCancel).toHaveBeenCalledWith("analysis")
    fireEvent.click(screen.getByRole("button", { name: /Cancel preview/ }))
    expect(commands.jobCancel).toHaveBeenLastCalledWith("preview")
  })

  it("waits to find a stage again while the analysis finds it", () => {
    // The analysis under way, or waiting, finds every stage that isn't ready
    // itself: finding one of those again would only start it over at the
    // back of the queue. A stage that is done can be found again.
    // [shots, characters, the analysis] -> [Find shots again, Find characters again]
    type Look = ReturnType<typeof look>
    const matrix: [MediaItem["shots"], MediaItem["faces"], "running" | "queued" | null, Look, Look][] = [
      ["running", "pending", "running", "disabled", "left out"], // finding shots
      ["pending", "pending", "queued", "disabled", "left out"], // waits to find shots
      ["ready", "running", "running", "enabled", "disabled"], // finding characters: shots can start over
      ["ready", "pending", "queued", "enabled", "disabled"], // waits to find characters
      ["ready", "ready", "running", "enabled", "enabled"], // finding people, or grouping
      ["ready", "ready", null, "enabled", "enabled"], // analysed
      ["failed", "pending", null, "enabled", "left out"], // shots failed
      ["ready", "stopped", null, "enabled", "enabled"], // characters stopped
    ]
    for (const [shots, faces, analysis, wantShots, wantFaces] of matrix) {
      const row = `shots ${shots}, characters ${faces}, analysis ${analysis ?? "none"}`
      useActivity.setState({ jobs: {}, order: [] })
      if (analysis) {
        useActivity.getState().replaceAll([job({ state: analysis, progress: analysis === "running" ? 0.4 : null })])
      }
      const { unmount } = items(episode({ shots, faces }))
      expect([look(item(/Find shots again/)), look(item(/Find characters again/))], row).toEqual([wantShots, wantFaces])
      // What runs is named right above, and removal never waits.
      expect(look(item(/Cancel analysis/)), row).toBe(analysis ? "enabled" : "left out")
      expect(look(item(/Remove from project/)), row).toBe("enabled")
      unmount()
    }
  })

  it("finds the shots again while characters are being found", () => {
    // The shots are done, so they can start over: the analysis begins again
    // from them. The characters it is finding wait.
    useActivity.getState().replaceAll([job({ id: "analysis", stage: "Finding characters" })])
    items(episode({ faces: "running" }))
    fireEvent.click(item(/Find shots again/)!)
    expect(commands.mediaAnalyze).toHaveBeenCalledWith("p", "m", "shots")
    expect(look(item(/Find characters again/))).toBe("disabled")
  })

  it("a preview built beside it never holds a redo back", () => {
    // Only the analysis finds stages: a preview of the episode leaves both
    // redo items as they are.
    useActivity.getState().replaceAll([job({ id: "preview", kind: "proxy", stage: "Preparing preview" })])
    items(episode({ faces: "stopped" }))
    fireEvent.click(item(/Find shots again/)!)
    expect(commands.mediaAnalyze).toHaveBeenCalledWith("p", "m", "shots")
    fireEvent.click(item(/Find characters again/)!)
    expect(commands.mediaAnalyze).toHaveBeenLastCalledWith("p", "m", "faces")
  })

  it("removing stays while it works", () => {
    // The backend stops the episode's work before it removes it.
    useActivity.getState().replaceAll([
      job({ id: "analysis", stage: "Finding shots" }),
      job({ id: "preview", kind: "proxy", stage: "Preparing preview", createdAt: 2, rev: 2 }),
    ])
    const onRemove = vi.fn()
    items(episode({ shots: "running", faces: "pending" }), onRemove)
    fireEvent.click(item(/Remove from project/)!)
    expect(onRemove).toHaveBeenCalled()
  })

  it("cancels a queued indexing as indexing, and offers nothing once its jobs are done", () => {
    useActivity.getState().replaceAll([job({ id: "ingest", kind: "ingest", state: "queued", progress: null })])
    const queued = { id: "m", path: "C:\\Show\\E01.mkv", status: "queued", shots: "pending" } as MediaItem
    const { unmount } = render(<EpisodeItems kit={kit} projectId="p" media={queued} onOpen={() => {}} onRemove={() => {}} />)
    fireEvent.click(screen.getByRole("button", { name: /Cancel indexing/ }))
    expect(commands.jobCancel).toHaveBeenCalledWith("ingest")
    expect(screen.queryByRole("button", { name: /Cancel analysis/ })).toBeNull()
    unmount()
    useActivity.getState().upsert(job({ id: "ingest", kind: "ingest", state: "cancelled", rev: 2 }))
    render(<EpisodeItems kit={kit} projectId="p" media={queued} onOpen={() => {}} onRemove={() => {}} />)
    expect(screen.queryByRole("button", { name: /Cancel/ })).toBeNull()
  })

  it("says so when a cancel can't be sent", async () => {
    vi.mocked(commands.jobCancel).mockRejectedValueOnce("the app is closing")
    useActivity.getState().replaceAll([job({ id: "analysis" })])
    render(<EpisodeItems kit={kit} projectId="p" media={analysed} onOpen={() => {}} onRemove={() => {}} />)
    fireEvent.click(screen.getByRole("button", { name: /Cancel analysis/ }))
    await vi.waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith("Couldn't cancel the job", { description: "the app is closing" }),
    )
  })
})
