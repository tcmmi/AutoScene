import { act, renderHook } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { JobInfo, JobProgress } from "@/bindings"
import { ETA_BRIDGE_MS, HOLD_MS, useActiveProgress, useActivity, useProjectIndexProgress } from "./activity"

const job = (o: Partial<JobInfo>): JobInfo => ({
  id: "j1",
  kind: "export",
  title: "Exporting 1 clip",
  subtitle: null,
  projectId: "p",
  mediaId: null,
  exportId: null,
  state: "queued",
  progress: null,
  stage: null,
  etaMs: null,
  error: null,
  createdAt: 1,
  finishedAt: null,
  rev: 1,
  ...o,
})

describe("activity store", () => {
  beforeEach(() => useActivity.setState({ jobs: {}, order: [] }))

  it("ignores an event that arrives after a newer one", () => {
    const s = useActivity.getState()
    s.upsert(job({ state: "done", rev: 3 }))
    s.upsert(job({ state: "running", rev: 2 }))
    expect(useActivity.getState().jobs.j1.state).toBe("done")
  })

  it("ignores stale progress", () => {
    const s = useActivity.getState()
    s.upsert(job({ state: "running", progress: 0.5, rev: 5 }))
    s.progress({ id: "j1", rev: 4, progress: 0.1, stage: null, etaMs: null })
    expect(useActivity.getState().jobs.j1.progress).toBe(0.5)
    s.progress({ id: "j1", rev: 6, progress: 0.7, stage: "Rendering", etaMs: 10 })
    expect(useActivity.getState().jobs.j1.progress).toBe(0.7)
  })

  it("keeps the frames a job reads only while it tells them", () => {
    const s = useActivity.getState()
    s.upsert(job({ kind: "ingest", state: "running", rev: 1 }))
    const frames = { done: 1200, total: 34_000, rate: 23.976, perSec: null }
    s.progress({ id: "j1", rev: 2, progress: 0.03, stage: "Indexing frames", etaMs: null, frames })
    expect(useActivity.getState().jobs.j1.frames).toEqual(frames)
    s.progress({ id: "j1", rev: 3, progress: 0.96, stage: "Creating poster", etaMs: null, frames: null })
    expect(useActivity.getState().jobs.j1.frames).toBeNull()
  })

  it("merges a late snapshot without losing newer events", () => {
    const s = useActivity.getState()
    s.upsert(job({ id: "new", state: "running", createdAt: 5, rev: 9 }))
    s.upsert(job({ id: "j1", state: "done", rev: 8 }))
    s.replaceAll([job({ id: "j1", state: "running", rev: 7 })])
    const st = useActivity.getState()
    expect(st.jobs.j1.state).toBe("done")
    expect(st.order).toEqual(["new", "j1"])
  })
})

describe("a project's indexing", () => {
  beforeEach(() => useActivity.setState({ jobs: {}, order: [] }))

  it("is as far as its running indexing jobs are, on average", () => {
    const { result } = renderHook(() => useProjectIndexProgress("p"))
    expect(result.current).toBeNull()
    act(() => {
      const s = useActivity.getState()
      s.upsert(job({ id: "a", kind: "ingest", state: "running", progress: 0.2 }))
      s.upsert(job({ id: "b", kind: "ingest", state: "running", progress: 0.6 }))
      // Not counted: waiting, no progress yet, not indexing, another project.
      s.upsert(job({ id: "c", kind: "ingest", state: "queued", progress: null }))
      s.upsert(job({ id: "d", kind: "ingest", state: "running", progress: null }))
      s.upsert(job({ id: "e", kind: "analyze", state: "running", progress: 0.9 }))
      s.upsert(job({ id: "f", kind: "ingest", state: "running", progress: 0.9, projectId: "q" }))
    })
    expect(result.current).toBeCloseTo(0.4)
  })
})

describe("a momentary gap in progress", () => {
  beforeEach(() => {
    vi.useFakeTimers()
    useActivity.setState({ jobs: {}, order: [] })
  })
  afterEach(() => vi.useRealTimers())

  let rev = 1
  const tell = (progress: number | null, etaMs: number | null = null, stage: string | null = "Downloading AI model") => {
    const p: JobProgress = { id: "j1", rev: ++rev, progress, stage, etaMs }
    useActivity.getState().progress(p)
  }
  const shown = () => {
    const j = useActivity.getState().jobs.j1
    return { progress: j.progress, etaMs: j.etaMs }
  }
  const running = () => useActivity.getState().upsert(job({ state: "running", rev: ++rev }))

  it("a null under 1.5 s keeps the last value and time left", () => {
    running()
    tell(0.98, 4000)
    vi.advanceTimersByTime(500)
    tell(null)
    expect(shown()).toEqual({ progress: 0.98, etaMs: 3500 })
    vi.advanceTimersByTime(HOLD_MS - 1)
    tell(null)
    expect(shown()).toEqual({ progress: 0.98, etaMs: 4000 - 500 - (HOLD_MS - 1) })
    tell(0.02)
    expect(shown().progress).toBe(0.02)
    vi.advanceTimersByTime(HOLD_MS)
    expect(shown().progress).toBe(0.02)
  })

  it("a null that lasts 1.5 s shows as unknown, with no time left", () => {
    running()
    tell(0.5, 3000)
    tell(null)
    vi.advanceTimersByTime(HOLD_MS - 1)
    expect(shown()).toEqual({ progress: 0.5, etaMs: 3000 })
    vi.advanceTimersByTime(1)
    expect(shown()).toEqual({ progress: null, etaMs: null })
    // What comes next isn't held any more: it is unknown until a value comes.
    tell(null)
    expect(shown()).toEqual({ progress: null, etaMs: null })
    expect(vi.getTimerCount()).toBe(0)
  })

  it("nulls in a row don't stretch the hold", () => {
    running()
    tell(0.4)
    tell(null)
    vi.advanceTimersByTime(900)
    tell(null)
    vi.advanceTimersByTime(HOLD_MS - 900 - 1)
    expect(shown().progress).toBe(0.4)
    vi.advanceTimersByTime(1)
    expect(shown().progress).toBeNull()
  })

  it("a job that never told a value has no hold", () => {
    running()
    tell(null, null, "Preparing preview")
    expect(shown().progress).toBeNull()
    expect(vi.getTimerCount()).toBe(0)
  })

  it("a job that ends drops its hold", () => {
    running()
    tell(0.7, 2000)
    tell(null)
    expect(vi.getTimerCount()).toBe(1)
    useActivity.getState().upsert(job({ state: "done", progress: 1, rev: ++rev }))
    expect(vi.getTimerCount()).toBe(0)
    vi.advanceTimersByTime(HOLD_MS * 2)
    expect(useActivity.getState().jobs.j1).toMatchObject({ state: "done", progress: 1 })

    // A snapshot that has a job waiting again drops its hold too.
    useActivity.getState().upsert(job({ id: "j2", state: "running", progress: 0.3, rev: ++rev }))
    useActivity.getState().progress({ id: "j2", rev: ++rev, progress: null, stage: null, etaMs: null })
    expect(vi.getTimerCount()).toBe(1)
    useActivity.getState().replaceAll([job({ id: "j2", state: "queued", progress: null, rev: ++rev })])
    expect(vi.getTimerCount()).toBe(0)
  })

  it("a stale timer can't end a newer hold", () => {
    running()
    tell(0.3)
    tell(null) // hold 1 from 0 ms
    vi.advanceTimersByTime(1000)
    tell(0.35)
    vi.advanceTimersByTime(200)
    tell(null) // hold 2 from 1200 ms
    vi.advanceTimersByTime(HOLD_MS - 1200) // where hold 1 would have ended
    expect(shown().progress).toBe(0.35)
    vi.advanceTimersByTime(1200 - 1)
    expect(shown().progress).toBe(0.35)
    vi.advanceTimersByTime(1)
    expect(shown().progress).toBeNull()

    // The token: an old hold's own end does nothing once a newer hold began.
    tell(0.5)
    tell(null)
    const since = Date.now()
    useActivity.getState().unhold("j1", since - 1)
    expect(shown().progress).toBe(0.5)
  })

  it("the rail's ring keeps counting a held job", () => {
    const { result } = renderHook(() => useActiveProgress())
    act(() => {
      running()
      tell(0.6)
      tell(null)
    })
    expect(result.current).toBeCloseTo(0.6)
    act(() => void vi.advanceTimersByTime(HOLD_MS))
    expect(result.current).toBeNull()
  })

  it("after a failed clip the time left counts down until the backend tells one again", () => {
    running()
    tell(0.4, 6000, "Clip 2 of 3")
    vi.advanceTimersByTime(300)
    tell(null, null, "Clip 2 of 3") // the clip failed: the backend measures its pace anew
    expect(shown()).toEqual({ progress: 0.4, etaMs: 5700 })
    vi.advanceTimersByTime(700)
    tell(0.5, null, "Clip 3 of 3")
    expect(shown()).toEqual({ progress: 0.5, etaMs: 5000 })
    vi.advanceTimersByTime(2000)
    tell(0.6, null, "Clip 3 of 3")
    expect(shown()).toEqual({ progress: 0.6, etaMs: 3000 })
    vi.advanceTimersByTime(ETA_BRIDGE_MS - 3000) // 5 s since it was told: no more borrowing
    tell(0.7, null, "Clip 3 of 3")
    expect(shown()).toEqual({ progress: 0.7, etaMs: null })
    tell(0.72, 900, "Clip 3 of 3")
    expect(shown().etaMs).toBe(900)

    // The backend's own figure wins while one is borrowed.
    vi.advanceTimersByTime(100)
    tell(0.74, null, "Clip 3 of 3")
    expect(shown().etaMs).toBe(800)
    tell(0.75, 1200, "Clip 3 of 3")
    expect(shown().etaMs).toBe(1200)
  })

  it("a null that names a new stage holds the old stage's name with its value", () => {
    const stage = () => useActivity.getState().jobs.j1.stage
    running()
    tell(1, 400, "Finding characters")
    tell(null, null, "Grouping characters")
    expect(stage()).toBe("Finding characters")
    expect(shown()).toEqual({ progress: 1, etaMs: 400 })
    vi.advanceTimersByTime(HOLD_MS - 1)
    expect(stage()).toBe("Finding characters")
    vi.advanceTimersByTime(1)
    // The hold ran out: the new stage shows, and how far it is isn't known.
    expect(stage()).toBe("Grouping characters")
    expect(shown()).toEqual({ progress: null, etaMs: null })

    // A model's download: its name comes with its first value, never beside the old stage's 100 %.
    tell(1, null, "Finding shots")
    tell(null, null, "Downloading AI model")
    expect(stage()).toBe("Finding shots")
    expect(shown().progress).toBe(1)
    vi.advanceTimersByTime(300)
    tell(0.44, null, "Downloading AI model")
    expect(stage()).toBe("Downloading AI model")
    expect(shown().progress).toBe(0.44)
    vi.advanceTimersByTime(HOLD_MS)
    expect(stage()).toBe("Downloading AI model")
    expect(shown().progress).toBe(0.44)

    // A null in the same stage keeps its own name, as before.
    tell(null, null, "Downloading AI model")
    expect(stage()).toBe("Downloading AI model")
    expect(shown().progress).toBe(0.44)
  })

  it("a new stage or a value that goes back borrows no time left", () => {
    running()
    tell(0.9, 2000, "Finding shots")
    tell(0.1, null, "Downloading AI model")
    expect(shown().etaMs).toBeNull()

    tell(0.8, 1000, "Downloading AI model")
    tell(0.05, null, "Downloading AI model") // the next model
    expect(shown().etaMs).toBeNull()
  })
})
