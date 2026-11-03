import { describe, expect, it } from "vitest"
import type { JobInfo } from "@/bindings"
import { HOLD_MS } from "@/state/activity"
import { ANALYSIS, analysisSteps, analysisTick } from "./backend"

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
