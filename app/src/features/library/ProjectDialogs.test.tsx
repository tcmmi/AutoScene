import { cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { JobInfo, ProjectSummary } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { useActivity } from "@/state/activity"
import { DeleteProjectDialog, ProjectDetailsDialog } from "./ProjectDialogs"

vi.mock("@/lib/ipc", () => ({
  commands: {
    projectSetContent: vi.fn((_id: string, content: string) => Promise.resolve({ content })),
    projectSetFastScan: vi.fn((_id: string, fastScan: boolean) => Promise.resolve({ fastScan })),
    projectRename: vi.fn(() => Promise.resolve({})),
    projectDelete: vi.fn(() => Promise.resolve(null)),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => vi.fn(),
  useRouterState: () => false,
}))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useActivity.setState({ jobs: {}, order: [] })
})

const film = {
  id: "p",
  title: "Film",
  kind: "movie",
  content: "live",
  fastScan: false,
} as ProjectSummary

function open(project: ProjectSummary, onOpenChange = vi.fn()) {
  render(
    <TooltipProvider>
      <ProjectDetailsDialog project={project} onOpenChange={onOpenChange} />
    </TooltipProvider>,
  )
  return onOpenChange
}

describe("ProjectDetailsDialog", () => {
  it("switches a project to Fast Scan and says its faces are found again", async () => {
    const user = userEvent.setup()
    const onOpenChange = open(film)
    expect(screen.getByText(/found in full/)).toBeTruthy()
    await user.click(screen.getByRole("switch", { name: "Fast Scan" }))
    expect(screen.getByText(/found again this way/)).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
    expect(commands.projectSetFastScan).toHaveBeenCalledWith("p", true)
    expect(commands.projectSetContent).not.toHaveBeenCalled()
    expect(commands.projectRename).not.toHaveBeenCalled()
  })

  it("leaves the scan alone when only the title changes", async () => {
    const user = userEvent.setup()
    open({ ...film, fastScan: true })
    expect(screen.getByText(/the way the Scener app does/)).toBeTruthy()
    const title = screen.getByLabelText("Title")
    await user.clear(title)
    await user.type(title, "Film 2")
    await user.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(commands.projectRename).toHaveBeenCalledWith("p", "Film 2"))
    expect(commands.projectSetFastScan).not.toHaveBeenCalled()
  })

  it("switches anime to Fast Scan too, which keeps its character models", async () => {
    const user = userEvent.setup()
    const onOpenChange = open({ ...film, content: "anime" })
    expect(screen.getByText(/with the anime models every few frames/)).toBeTruthy()
    await user.click(screen.getByRole("switch", { name: "Fast Scan" }))
    expect(screen.getByText(/Characters are found again this way/)).toBeTruthy()
    await user.click(screen.getByRole("button", { name: "Save" }))
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
    expect(commands.projectSetFastScan).toHaveBeenCalledWith("p", true)
  })
})

/** A running job of the project "p". */
const job = (o: Partial<JobInfo>): JobInfo => ({
  id: "j",
  kind: "export",
  title: "Exporting 12 clips",
  subtitle: "Film",
  projectId: "p",
  mediaId: null,
  exportId: "e",
  state: "running",
  progress: 0.3,
  stage: "Rendering",
  etaMs: null,
  error: null,
  createdAt: 1,
  finishedAt: null,
  rev: 1,
  ...o,
})

/** The dialog's description, as a screen reader hears it. */
const description = () => {
  const id = screen.getByRole("dialog").getAttribute("aria-describedby")
  return document.getElementById(id ?? "")?.textContent ?? ""
}

describe("DeleteProjectDialog", () => {
  it("says a running export of the project stops first", () => {
    // Another project's plugin run, upkeep and a finished analysis don't count.
    useActivity.getState().replaceAll([
      job({ id: "export" }),
      job({ id: "elsewhere", kind: "plugin", projectId: "q", exportId: null, createdAt: 2 }),
      job({ id: "posters", kind: "posters", exportId: null, createdAt: 3 }),
      job({ id: "done", kind: "analyze", mediaId: "m", exportId: null, state: "done", createdAt: 4 }),
    ])
    const { unmount } = render(<DeleteProjectDialog project={film} onOpenChange={vi.fn()} />)
    expect(description()).toBe(
      "Your source videos and exported clips are never touched. Work running on it stops first: an export.",
    )
    unmount()
    // Without work running, the description stays as it was.
    useActivity.setState({ jobs: {}, order: [] })
    render(<DeleteProjectDialog project={film} onOpenChange={vi.fn()} />)
    expect(description()).toBe("Your source videos and exported clips are never touched.")
  })
})
