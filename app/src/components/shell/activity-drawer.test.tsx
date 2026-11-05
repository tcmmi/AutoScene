import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { JobInfo } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { useActivity } from "@/state/activity"
import { useUi } from "@/state/ui"
import { ActivityDrawer } from "./ActivityDrawer"
import { AppMenu } from "./AppMenu"
import { inLanguage, plain } from "@/test/i18n"

const { copyText } = vi.hoisted(() => ({ copyText: vi.fn() }))

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => vi.fn(),
  useCanGoBack: () => false,
}))
vi.mock("@/lib/os", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/os")>()), copyText }))

const job = (o: Partial<JobInfo>) =>
  ({
    id: "j1",
    kind: "plugin",
    title: plain("Post to Discord"),
    subtitle: "Discord notify",
    projectId: "p",
    mediaId: null,
    exportId: "e1",
    state: "running",
    progress: null,
    stage: null,
    etaMs: null,
    error: null,
    message: null,
    createdAt: 1,
    finishedAt: null,
    rev: 1,
    ...o,
  }) as JobInfo

function show(...jobs: JobInfo[]) {
  useActivity.setState({ jobs: Object.fromEntries(jobs.map((j) => [j.id, j])), order: jobs.map((j) => j.id) })
  render(
    <AppMenu>
      <div>
        <ActivityDrawer />
      </div>
    </AppMenu>,
    { wrapper: TooltipProvider },
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  useUi.setState({ activityOpen: true })
})
afterEach(() => {
  cleanup()
  useUi.setState({ activityOpen: false })
  useActivity.setState({ jobs: {}, order: [] })
})

describe("the Activity drawer", () => {
  it("stops a plugin's run, and cancels other work", () => {
    show(job({}), job({ id: "j2", kind: "ingest", title: plain("Indexing Pilot"), exportId: null }))
    expect(screen.getByRole("button", { name: "Stop Post to Discord" })).toBeTruthy()
    expect(screen.getByRole("button", { name: "Cancel Indexing Pilot" })).toBeTruthy()
  })

  it("shows what a finished plugin had to say, and copies it", async () => {
    show(job({ state: "done", message: plain("Posted 12 clips."), finishedAt: 2 }))
    expect(screen.getByText("Posted 12 clips.")).toBeTruthy()
    fireEvent.contextMenu(screen.getByText("Post to Discord"), { clientX: 40, clientY: 40 })
    fireEvent.click(await screen.findByRole("menuitem", { name: "Copy the message" }))
    await waitFor(() => expect(copyText).toHaveBeenCalledExactlyOnceWith("Posted 12 clips.", "message"))
  })

  it("shows no message for a job that failed", () => {
    show(job({ state: "failed", error: plain("No webhook is set."), message: plain("Posted 12 clips.") }))
    expect(screen.getByText("No webhook is set.")).toBeTruthy()
    expect(screen.queryByText("Posted 12 clips.")).toBeNull()
  })

  it("leaves a waiting job's bar empty and still, and sweeps only for one that runs", () => {
    show(
      job({ id: "run", kind: "analyze", title: plain("Analysing E01.mkv"), state: "running", stage: plain("Grouping characters"), createdAt: 2 }),
      job({ id: "wait", kind: "analyze", title: plain("Analysing E02.mkv"), state: "queued" }),
    )
    const bar = (title: string) => screen.getByText(title).closest("li")!.querySelector<HTMLElement>("[role=progressbar]")!
    const waiting = bar("Analysing E02.mkv")
    expect(screen.getByText("Waiting")).toBeTruthy()
    expect(waiting.getAttribute("aria-valuenow")).toBe("0")
    expect(waiting.querySelector(".animate-indeterminate")).toBeNull()
    const running = bar("Analysing E01.mkv")
    expect(running.getAttribute("aria-valuenow")).toBeNull()
    expect(running.querySelector(".animate-indeterminate")).not.toBeNull()
  })
})

describe("a job in another language", () => {
  const RO = {
    backend: {
      job: {
        analysing: "Se analizează {{name}}",
        exporting_one: "Se exportă {{count}} clip",
        exporting_few: "Se exportă {{count}} clipuri",
        exporting_other: "Se exportă {{count}} de clipuri",
        clipsFailed_few: "{{count}} clipuri n-au putut fi exportate.",
        stage: { findingShots: "Se caută cadrele", onCpu: "{{stage}} · pe procesor (mai încet)" },
      },
      plugin: { saved: "S-a salvat {{file}}" },
    },
  }
  const onCpu = {
    key: "job.stage.onCpu",
    args: { stage: { key: "job.stage.findingShots", text: "Finding shots" } },
    text: "Finding shots · on the CPU (slower)",
  }
  const analysing = { key: "job.analysing", args: { name: "E01.mkv" }, text: "Analysing E01.mkv" }
  const exporting = { key: "job.exporting", args: { count: 3 }, text: "Exporting 3 clips" }

  it("shows its title, its stage, what went wrong and its message in the language", async () => {
    await inLanguage("ro", RO)
    show(
      job({ id: "a", kind: "analyze", title: analysing, stage: onCpu, createdAt: 4 }),
      job({
        id: "e",
        kind: "export",
        title: exporting,
        state: "failed",
        error: { key: "job.clipsFailed", args: { count: 3 }, text: "3 clips failed to export." },
        createdAt: 3,
      }),
      job({ id: "s", title: plain("Save"), state: "done", message: { key: "plugin.saved", args: { file: "a.xml" }, text: "Saved a.xml" }, createdAt: 2 }),
    )
    expect(screen.getByText("Se analizează E01.mkv")).toBeTruthy()
    expect(screen.getByText("Se caută cadrele · pe procesor (mai încet)")).toBeTruthy()
    expect(screen.getByText("Se exportă 3 clipuri")).toBeTruthy()
    expect(screen.getByText("3 clipuri n-au putut fi exportate.")).toBeTruthy()
    expect(screen.getByText("S-a salvat a.xml")).toBeTruthy()
    expect(document.body.innerHTML).not.toContain("[object Object]")
  })

  it("names the job in the words of the language on its cancel button", async () => {
    await inLanguage("ro", RO)
    show(job({ id: "a", kind: "analyze", title: analysing }))
    expect(screen.getByRole("button", { name: /Se analizează E01\.mkv/ })).toBeTruthy()
  })

  it("copies what went wrong as it reads", async () => {
    await inLanguage("ro", RO)
    show(job({ title: exporting, state: "failed", error: { key: "job.clipsFailed", args: { count: 3 }, text: "3 clips failed to export." } }))
    fireEvent.contextMenu(screen.getByText("Se exportă 3 clipuri"), { clientX: 40, clientY: 40 })
    fireEvent.click(await screen.findByRole("menuitem", { name: "Copy the error" }))
    await waitFor(() => expect(copyText).toHaveBeenCalledExactlyOnceWith("3 clipuri n-au putut fi exportate.", "error"))
  })

  it("copies AutoScene's message as it reads", async () => {
    await inLanguage("ro", RO)
    show(job({ title: plain("Save"), state: "done", message: { key: "plugin.saved", args: { file: "a.xml" }, text: "Saved a.xml" } }))
    fireEvent.contextMenu(screen.getByText("Save"), { clientX: 40, clientY: 40 })
    fireEvent.click(await screen.findByRole("menuitem", { name: "Copy the message" }))
    await waitFor(() => expect(copyText).toHaveBeenCalledExactlyOnceWith("S-a salvat a.xml", "message"))
  })

  it("leaves a plugin's stage as the plugin wrote it", async () => {
    await inLanguage("ro", RO)
    show(job({ stage: plain("Posting clips to Discord") }))
    expect(screen.getByText("Post to Discord")).toBeTruthy()
    expect(screen.getByText("Posting clips to Discord")).toBeTruthy()
  })
})
