import { cleanup, render, screen } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { JobInfo, MediaItem } from "@/bindings"
import { useActivity } from "@/state/activity"
import { AnalysisStatus } from "./AnalysisStatus"
import { inLanguage, plain } from "@/test/i18n"

vi.mock("@/lib/queries", () => ({
  useMediaList: () => ({ data: [{ id: "m1", label: "S01E01", fileName: "Pilot.mkv" } as MediaItem] }),
}))

const job = (o: Partial<JobInfo>): JobInfo => ({
  id: "a1",
  kind: "analyze",
  title: plain("Analysing S01E01 · Pilot.mkv"),
  subtitle: null,
  projectId: "p",
  mediaId: "m1",
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

afterEach(() => {
  cleanup()
  useActivity.setState({ jobs: {}, order: [] })
})

describe("the project's analysis status", () => {
  it("stands still while the analysis waits: no ping, no gold, an empty bar", () => {
    useActivity.getState().replaceAll([job({})])
    const { container } = render(<AnalysisStatus pid="p" />)
    expect(screen.getByText("Waiting to analyse")).toBeTruthy()
    expect(container.querySelector(".animate-ping")).toBeNull()
    expect(container.querySelector(".text-accent")).toBeNull()
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("0")
    expect(container.querySelector(".animate-indeterminate")).toBeNull()
  })

  it("says its stage in the language", async () => {
    await inLanguage("ro", { backend: { job: { stage: { findingShots: "Se caută cadrele" } } } })
    useActivity.getState().replaceAll([job({ state: "running", stage: { key: "job.stage.findingShots", text: "Finding shots" } })])
    const { container } = render(<AnalysisStatus pid="p" />)
    expect(screen.getByText("Se caută cadrele")).toBeTruthy()
    expect(container.textContent).not.toContain("[object Object]")
  })

  it("pings while it works", () => {
    useActivity.getState().replaceAll([job({ state: "running", progress: 0.3, stage: plain("Finding shots") })])
    const { container } = render(<AnalysisStatus pid="p" />)
    expect(screen.getByText("Finding shots")).toBeTruthy()
    expect(container.querySelector(".animate-ping")).not.toBeNull()
    expect(container.querySelector(".text-accent")).not.toBeNull()
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBe("30")
  })
})
