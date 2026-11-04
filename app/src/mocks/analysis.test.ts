import { invoke } from "@tauri-apps/api/core"
import { clearMocks } from "@tauri-apps/api/mocks"
import { afterAll, beforeAll, describe, expect, it } from "vitest"
import type { JobInfo, MediaItem, ProjectSummary } from "@/bindings"
import { HOLD_MS, isFinished } from "@/state/activity"
import { ANALYSIS, analysisSteps, analysisTick, installMockBackend } from "./backend"

const job = (): JobInfo => ({
  id: "a",
  kind: "analyze",
  title: "Analysing E01.mkv",
  subtitle: null,
  projectId: "p",
  mediaId: "m",
  exportId: null,
  state: "running",
  progress: 0,
  stage: null,
  etaMs: null,
  error: null,
  createdAt: 1,
  finishedAt: null,
  rev: 1,
})

/** What the mock tells `elapsed` ms into a run. */
function at(facesOnly: boolean, elapsed: number) {
  const j = job()
  analysisTick(analysisSteps(facesOnly))(j, elapsed)
  return { stage: j.stage, progress: j.progress, etaMs: j.etaMs }
}

describe("the mock's analysis", () => {
  it("runs as a first analysis does: shots, a model's download, characters, then grouping", () => {
    expect(at(false, 0)).toEqual({ stage: "Finding shots", progress: 0, etaMs: null })
    expect(at(false, 1400)).toEqual({ stage: "Finding shots", progress: 0.8, etaMs: 600 })
    // Its last sample tells it is done, so the value held over the next moment is its 100 %.
    expect(at(false, 1800).progress).toBe(1)
    // The download tells nothing until its response comes, then starts its bar at 0.
    expect(at(false, 2100)).toEqual({ stage: "Downloading AI model", progress: null, etaMs: null })
    expect(at(false, 2300)).toEqual({ stage: "Downloading AI model", progress: 0, etaMs: null })
    expect(at(false, 3000).stage).toBe("Finding characters")
    expect(at(false, 6000)).toEqual({ stage: "Grouping characters", progress: null, etaMs: null })
  })

  it("has one momentary gap the bar holds over and one unknown stage it sweeps for", () => {
    const [gap, grouping] = ANALYSIS.filter((s) => !s.told)
    expect(gap.ms).toBeLessThan(HOLD_MS)
    expect(grouping.ms).toBeGreaterThan(HOLD_MS)
  })

  it("starts a characters-only run at its characters", () => {
    expect(at(true, 0)).toEqual({ stage: "Finding characters", progress: 0, etaMs: null })
    expect(at(true, 2600).stage).toBe("Grouping characters")
  })
})

describe("the mock's analyses, as the backend's one analysis slot runs them", () => {
  beforeAll(() => installMockBackend())
  afterAll(() => clearMocks())

  it("run one at a time, and cancelling one that waits leaves its episode stopped where it would have begun", async () => {
    const projects = await invoke<ProjectSummary[]>("projects_list")
    const projectId = projects.find((p) => p.title === "Sousou no Frieren")!.id
    const episodes = async () => invoke<MediaItem[]>("media_list", { projectId })
    const [a, b, c] = (await episodes()).filter((m) => m.status === "ready")
    for (const m of [a, b, c]) await invoke("media_analyze", { projectId, mediaId: m.id, redo: "shots" })
    const jobOf = async (m: MediaItem) =>
      (await invoke<JobInfo[]>("jobs_list")).find((j) => j.kind === "analyze" && j.mediaId === m.id && !isFinished(j))
    const shots = async (m: MediaItem) => (await episodes()).find((x) => x.id === m.id)!.shots

    const [ja, jb, jc] = [(await jobOf(a))!, (await jobOf(b))!, (await jobOf(c))!]
    expect([ja.state, jb.state, jc.state]).toEqual(["running", "queued", "queued"])
    // As the backend queues a job: nothing told until it runs, and its episode isn't begun.
    expect([jb.progress, jb.stage, jb.etaMs]).toEqual([null, null, null])
    expect([await shots(a), await shots(b)]).toEqual(["running", "pending"])
    // Asking again for one that waits is asking for one under way: nothing new is queued.
    await invoke("media_analyze", { projectId, mediaId: b.id, redo: null })
    expect((await invoke<JobInfo[]>("jobs_list")).filter((j) => j.mediaId === b.id && !isFinished(j))).toHaveLength(1)

    await invoke("job_cancel", { jobId: jb.id })
    expect(await shots(b)).toBe("stopped")
    expect(await jobOf(b)).toBeUndefined()
    expect([(await jobOf(a))?.state, (await jobOf(c))?.state]).toEqual(["running", "queued"])

    // The slot comes free: the one that has waited since goes on.
    await invoke("job_cancel", { jobId: ja.id })
    expect(await shots(a)).toBe("stopped")
    expect((await jobOf(c))?.state).toBe("running")
    expect(await shots(c)).toBe("running")
    await invoke("job_cancel", { jobId: jc.id })
    expect(await shots(c)).toBe("stopped")
  })
})
