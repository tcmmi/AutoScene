import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { cleanup, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, describe, expect, it, vi } from "vitest"
import type { GraphicsClass, JobInfo, ProjectSummary, ScanDefaults } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { useActivity } from "@/state/activity"
import { DeleteProjectDialog, ProjectDetailsDialog } from "./ProjectDialogs"
import { plain } from "@/test/i18n"

const { machine, nav, route, toasts } = vi.hoisted(() => ({
  machine: { cls: "forAi" as GraphicsClass },
  nav: vi.fn(),
  route: { inProject: false },
  toasts: Object.assign(vi.fn(), { error: vi.fn() }),
}))

/** What the library answers on Automatic, like `as_project::scan::defaults`. */
function defaults(): ScanDefaults {
  const here = machine.cls === "notForAi" || machine.cls === "integrated" || machine.cls === "processor"
  return {
    computer: { class: machine.cls, name: "Intel Iris Xe Graphics" },
    choice: "auto",
    footage: (["live", "mixed", "animation", "anime"] as const).map((content) => ({ content, fast: here && content !== "animation" })),
    notice: false,
  }
}

vi.mock("@/lib/ipc", () => ({
  commands: {
    scanDefaultsGet: vi.fn(() => Promise.resolve(defaults())),
    projectSetContent: vi.fn((_id: string, content: string) => Promise.resolve({ content })),
    projectSetFastScan: vi.fn((_id: string, fastScan: boolean) => Promise.resolve({ fastScan })),
    projectRename: vi.fn(() => Promise.resolve({})),
    projectDelete: vi.fn(() => Promise.resolve(null)),
  },
  errorMessage: (e: unknown) => String(e),
}))
vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tanstack/react-router")>()),
  useNavigate: () => nav,
  useRouterState: () => route.inProject,
}))
vi.mock("sonner", () => ({ toast: toasts }))

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useActivity.setState({ jobs: {}, order: [] })
  machine.cls = "forAi"
  route.inProject = false
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
    <QueryClientProvider client={new QueryClient()}>
      <TooltipProvider>
        <ProjectDetailsDialog project={project} onOpenChange={onOpenChange} />
      </TooltipProvider>
    </QueryClientProvider>,
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

  it("a full-scan project on built-in graphics says Fast Scan suits them", async () => {
    machine.cls = "integrated"
    const user = userEvent.setup()
    open(film)
    expect(await screen.findByText(/found in full.* Fast Scan suits this computer's built-in graphics\.$/)).toBeTruthy()
    // Switched, it says what happens instead; so does a project on Fast Scan.
    await user.click(screen.getByRole("switch", { name: "Fast Scan" }))
    expect(screen.queryByText(/suits/)).toBeNull()
    cleanup()
    open({ ...film, fastScan: true })
    await screen.findByText(/the way the Scener app does/)
    expect(screen.queryByText(/suits/)).toBeNull()
    // 3D animation keeps the full scan on Automatic: Fast Scan isn't said to suit it.
    cleanup()
    open({ ...film, content: "animation" })
    await user.click(screen.getByRole("radio", { name: "Live action" }))
    expect(await screen.findByText(/Fast Scan suits/)).toBeTruthy()
    await user.click(screen.getByRole("radio", { name: "Animation" }))
    expect(screen.getByText(/found in full/).textContent).not.toMatch(/suits/)
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
  title: plain("Exporting 12 clips"),
  subtitle: "Film",
  projectId: "p",
  mediaId: null,
  exportId: "e",
  state: "running",
  progress: 0.3,
  stage: plain("Rendering"),
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

  it("is asked with the analysis switch in its box, and a running removal can't be dismissed", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(commands.projectDelete).mockImplementationOnce(() => new Promise((done) => (finish = () => done(null))))
    const onOpenChange = vi.fn()
    render(<DeleteProjectDialog project={film} onOpenChange={onOpenChange} />)
    const dialog = screen.getByRole("dialog")
    const keep = within(dialog).getByRole("switch", { name: "Delete its analysis too" })
    await user.click(keep)
    await user.click(within(dialog).getByRole("button", { name: "Remove from library" }))
    expect(commands.projectDelete).toHaveBeenCalledWith("p", false)
    await user.keyboard("{Escape}")
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }))
    expect(onOpenChange).not.toHaveBeenCalled()
    expect(within(dialog).getByRole("button", { name: "Remove from library" }).getAttribute("aria-busy")).toBe("true")
    finish()
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
  })

  it("leaves the project it was in and says it is gone", async () => {
    const user = userEvent.setup()
    route.inProject = true
    const onOpenChange = vi.fn()
    render(<DeleteProjectDialog project={film} onOpenChange={onOpenChange} />)
    await user.click(screen.getByRole("button", { name: "Delete project" }))
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
    expect(commands.projectDelete).toHaveBeenCalledWith("p", true)
    expect(nav).toHaveBeenCalledWith({ to: "/" })
    expect(toasts).toHaveBeenCalledWith("Removed “Film”")
  })

  it("stays on the library page when it was deleted from there", async () => {
    const user = userEvent.setup()
    const onOpenChange = vi.fn()
    render(<DeleteProjectDialog project={film} onOpenChange={onOpenChange} />)
    await user.click(screen.getByRole("button", { name: "Delete project" }))
    await waitFor(() => expect(toasts).toHaveBeenCalledWith("Removed “Film”"))
    expect(nav).not.toHaveBeenCalled()
  })

  it("a removal that fails stays open, says why, and doesn't navigate", async () => {
    const user = userEvent.setup()
    route.inProject = true
    vi.mocked(commands.projectDelete).mockRejectedValueOnce("The folder is in use.")
    const onOpenChange = vi.fn()
    render(<DeleteProjectDialog project={film} onOpenChange={onOpenChange} />)
    await user.click(screen.getByRole("button", { name: "Delete project" }))
    await waitFor(() =>
      expect(toasts.error).toHaveBeenCalledWith("Couldn't remove the project", { description: "The folder is in use." }),
    )
    expect(screen.getByRole("dialog")).toBeTruthy()
    expect(onOpenChange).not.toHaveBeenCalled()
    expect(nav).not.toHaveBeenCalled()
    expect(toasts).not.toHaveBeenCalled()
  })
})

describe("ProjectDetailsDialog's Save while it runs", () => {
  it("does nothing on a second click or Enter, and says it is busy", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(commands.projectRename).mockImplementationOnce(() => new Promise((done) => (finish = () => done({} as never))))
    const onOpenChange = open(film)
    const title = screen.getByLabelText("Title")
    await user.clear(title)
    await user.type(title, "Film 2")
    const save = screen.getByRole("button", { name: "Save" })
    await user.click(save)
    await waitFor(() => expect(save.getAttribute("aria-busy")).toBe("true"))
    await user.click(save)
    await user.type(title, "{Enter}")
    expect(commands.projectRename).toHaveBeenCalledOnce()
    expect(onOpenChange).not.toHaveBeenCalled()
    finish()
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false))
    expect(commands.projectRename).toHaveBeenCalledOnce()
  })
})
