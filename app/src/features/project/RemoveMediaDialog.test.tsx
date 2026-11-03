import { act, cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { JobInfo, MediaItem } from "@/bindings"
import { useActivity } from "@/state/activity"
import { RemoveMediaDialog } from "./RemoveMediaDialog"

vi.mock("@/lib/ipc", () => ({
  commands: { mediaRemove: vi.fn(() => Promise.resolve(null)) },
  errorMessage: (e: unknown) => String(e),
}))

afterEach(() => {
  cleanup()
  useActivity.setState({ jobs: {}, order: [] })
})

/** A job of the project "p": the analysis of the episode "m" unless told otherwise. */
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

/** An episode being analysed: its shots are found, its faces not yet. */
const episode = {
  id: "m",
  fileName: "E01.mkv",
  label: "S01E01",
  status: "ready",
  shots: "ready",
  shotCount: 412,
  faces: "running",
  faceCount: null,
} as MediaItem
const WHAT_GOES =
  "Its shots and faces go, and with them your decisions about those faces. Characters you haven't named who appear only here leave the cast. The video file itself isn't touched."
const NOTHING_YET = "Nothing has been found in it yet. The video file itself isn't touched."

const open = (media = episode) =>
  render(<RemoveMediaDialog projectId="p" media={media} onOpenChange={() => {}} />)
/** The dialog's description, as a screen reader hears it. */
const description = () => {
  const id = screen.getByRole("dialog").getAttribute("aria-describedby")
  return document.getElementById(id ?? "")?.textContent ?? ""
}

describe("RemoveMediaDialog", () => {
  it("says analysing stops first, after what goes, and nothing when idle", () => {
    // Its analysis runs; an export of the project and another episode's
    // indexing don't stop for it, and upkeep is never named.
    useActivity.getState().replaceAll([
      job({ id: "analysis" }),
      job({ id: "export", kind: "export", mediaId: null, exportId: "e", createdAt: 2 }),
      job({ id: "other", kind: "ingest", mediaId: "n", createdAt: 3 }),
      job({ id: "posters", kind: "posters", mediaId: null, createdAt: 4 }),
    ])
    const { unmount } = open()
    expect(description()).toBe(`${WHAT_GOES} Work running on it stops first: analysing.`)
    unmount()

    // Idle: a finished analysis stops nothing, and the dialog says nothing of it.
    useActivity.setState({ jobs: {}, order: [] })
    useActivity.getState().replaceAll([job({ id: "analysis", state: "done", progress: 1, finishedAt: 2 })])
    open({ ...episode, faces: "ready", faceCount: 37 })
    expect(description()).toBe(WHAT_GOES)
  })

  it("lists nothing that goes for a file still being indexed, and what goes once it has shots", () => {
    // The wrong file, added a moment ago: no shots or faces to lose yet.
    useActivity.getState().replaceAll([job({ id: "index", kind: "ingest", stage: "Reading file" })])
    const fresh = {
      ...episode,
      status: "indexing",
      shots: "pending",
      shotCount: null,
      faces: "pending",
      faceCount: null,
    } as MediaItem
    const { unmount } = open(fresh)
    expect(description()).toBe(`${NOTHING_YET} Work running on it stops first: indexing.`)
    unmount()

    // Indexed again after an analysis: queued, but its shots and faces are
    // still stored and go with it.
    useActivity.setState({ jobs: {}, order: [] })
    open({ ...fresh, status: "queued", shotCount: 412, faceCount: 37 })
    expect(description()).toBe(WHAT_GOES)
  })

  it("keeps the sentence when the job ends while it is open", () => {
    useActivity.getState().replaceAll([job({ id: "analysis" }), job({ id: "preview", kind: "proxy", createdAt: 2 })])
    const { rerender } = open()
    const said = `${WHAT_GOES} Work running on it stops first: analysing and building a preview.`
    expect(description()).toBe(said)
    // Both end, and the episode comes back fresh from its refetch: the
    // centred dialog keeps its size under the pointer.
    const ended = { state: "done", progress: 1, finishedAt: 3 } as const
    act(() => {
      useActivity.getState().upsert(job({ id: "analysis", ...ended, rev: 2 }))
      useActivity.getState().upsert(job({ id: "preview", kind: "proxy", createdAt: 2, ...ended, rev: 3 }))
    })
    rerender(<RemoveMediaDialog projectId="p" media={{ ...episode, shots: "ready" }} onOpenChange={() => {}} />)
    expect(description()).toBe(said)
  })
})
