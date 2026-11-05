import type * as React from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { act, cleanup, render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { GraphicsClass, ImportCandidate, ScanDefaults } from "@/bindings"
import { TooltipProvider } from "@/components/ui/tooltip"
import { commands } from "@/lib/ipc"
import { useNewProject } from "@/state/new-project"
import { NewProjectDialog } from "./NewProjectDialog"

const episode = (n: number, refused: string | null = null): ImportCandidate => ({
  path: `D:\\Show\\Show - 0${n}.mkv`,
  fileName: `Show - 0${n}.mkv`,
  season: null,
  episode: n,
  label: `E0${n}`,
  sizeBytes: 1e9,
  duplicate: false,
  refused,
})

const { state } = vi.hoisted(() => ({
  state: {
    cls: "integrated" as GraphicsClass,
    findBodies: false,
    /** How `scan_defaults_get` answers: with the defaults, not yet, or with an error. */
    defaults: "answer" as "answer" | "wait" | "fail",
  },
}))

/** What the library answers on Automatic, like `as_project::scan::defaults`. */
function defaults(): ScanDefaults {
  const here = state.cls === "notForAi" || state.cls === "integrated" || state.cls === "processor"
  return {
    computer: { class: state.cls, name: "Intel Iris Xe Graphics" },
    choice: "auto",
    footage: (["live", "mixed", "animation", "anime"] as const).map((content) => ({
      content,
      fast: here && content !== "animation" && (content === "anime" || !state.findBodies),
    })),
    notice: false,
  }
}

vi.mock("@tanstack/react-router", () => ({ useNavigate: () => vi.fn() }))
vi.mock("@/lib/ipc", () => ({
  commands: {
    importScan: vi.fn(() => Promise.resolve([episode(1), episode(2, "Its name isn't valid Unicode. Rename it to import it.")])),
    projectCreate: vi.fn(() => Promise.resolve({ id: "p" })),
    // `fastScan` is what live action starts with here.
    settingsGet: vi.fn(() => Promise.resolve({ findBodies: state.findBodies, fastScan: defaults().footage[0].fast })),
    scanDefaultsGet: vi.fn(() =>
      state.defaults === "answer"
        ? Promise.resolve(defaults())
        : state.defaults === "fail"
          ? Promise.reject(new Error("no library"))
          : new Promise(() => {}),
    ),
  },
  errorMessage: (e: unknown) => String(e),
}))

let queryClient: QueryClient
const wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={queryClient}>
    <TooltipProvider>{children}</TooltipProvider>
  </QueryClientProvider>
)
const show = (seed: string[] = ["D:\\Show"]) => {
  useNewProject.getState().show(seed)
  render(<NewProjectDialog />, { wrapper })
}
const fastScan = () => screen.getByRole("switch", { name: "Fast Scan" })
const isOn = () => fastScan().getAttribute("aria-checked") === "true"
/** The Fast Scan field's line; `undefined` while it has none. */
const line = () => screen.getByText("Fast Scan", { selector: "label" }).parentElement?.querySelector("p")?.textContent
/** Creates the project; gives the scan it was sent with. */
async function create(user: ReturnType<typeof userEvent.setup>) {
  await screen.findByText("1 video", { exact: false })
  await user.click(screen.getByRole("button", { name: "Create project" }))
  await waitFor(() => expect(commands.projectCreate).toHaveBeenCalled())
  return vi.mocked(commands.projectCreate).mock.calls[0][1]
}

beforeEach(() => {
  Object.assign(state, { cls: "integrated", findBodies: false, defaults: "answer" })
  queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } })
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useNewProject.getState().setDragging(false)
  useNewProject.getState().setOpen(false)
})

describe("NewProjectDialog", () => {
  it("lists a file that can't be imported with why, and leaves it out", async () => {
    const user = userEvent.setup()
    show()
    expect(await screen.findByText(/isn't valid Unicode/)).toBeTruthy()
    expect(screen.getByText("1 video", { exact: false })).toBeTruthy()
    expect(screen.queryByRole("button", { name: "Skip Show - 02.mkv" })).toBeNull()
    await user.click(screen.getByRole("button", { name: "Create project" }))
    await waitFor(() => expect(commands.projectCreate).toHaveBeenCalled())
    expect(vi.mocked(commands.projectCreate).mock.calls[0][0].files).toEqual(["D:\\Show\\Show - 01.mkv"])
  })

  it("Create project is busy while it runs: a second click does nothing", async () => {
    const user = userEvent.setup()
    let finish = () => {}
    vi.mocked(commands.projectCreate).mockImplementationOnce(
      () => new Promise((done) => (finish = () => done({ id: "p" } as never))),
    )
    show()
    await screen.findByText("1 video", { exact: false })
    const button = screen.getByRole("button", { name: "Create project" })
    await user.click(button)
    await waitFor(() => expect(button.getAttribute("aria-busy")).toBe("true"))
    await user.click(button)
    expect(commands.projectCreate).toHaveBeenCalledOnce()
    finish()
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull())
    expect(commands.projectCreate).toHaveBeenCalledOnce()
  })

  it("shows its file list as where dragged files go", () => {
    show([])
    expect(screen.queryByText("Drop to add to this project")).toBeNull()
    act(() => useNewProject.getState().setDragging(true))
    expect(screen.getByText("Drop to add to this project")).toBeTruthy()
    act(() => useNewProject.getState().setDragging(false))
    expect(screen.queryByText("Drop to add to this project")).toBeNull()
  })

  describe("Fast Scan", () => {
    it("starts with Fast Scan on graphics not built for AI work and says why", async () => {
      const user = userEvent.setup()
      show()
      await waitFor(() => expect(isOn()).toBe(true))
      expect(line()).toBe("Much faster, finds fewer faces. Suits this computer's built-in graphics.")
      expect(await create(user)).toBe(true)
    })

    it("switched off, sends false and keeps the line", async () => {
      const user = userEvent.setup()
      show()
      await waitFor(() => expect(isOn()).toBe(true))
      await user.click(fastScan())
      expect(isOn()).toBe(false)
      expect(line()).toBe("Much faster, finds fewer faces. Suits this computer's built-in graphics.")
      expect(await create(user)).toBe(false)
    })

    it("on graphics built for AI work it starts with the full scan and only says what Fast Scan costs", async () => {
      state.cls = "forAi"
      const user = userEvent.setup()
      show()
      await waitFor(() => expect(line()).toBe("Much faster, finds fewer faces."))
      expect(isOn()).toBe(false)
      expect(await create(user)).toBe(false)
    })

    it("picking Animation turns the untouched switch off and says why", async () => {
      const user = userEvent.setup()
      show()
      await waitFor(() => expect(isOn()).toBe(true))
      await user.click(screen.getByRole("radio", { name: "Animation" }))
      expect(isOn()).toBe(false)
      expect(line()).toBe("Much faster, but misses many animated faces.")
      // Anime gets it back, still untouched.
      await user.click(screen.getByRole("radio", { name: "Anime" }))
      expect(isOn()).toBe(true)
      expect(line()).toBe("Much faster, finds fewer characters. Suits this computer's built-in graphics.")
      // Flipped by hand, it stays as the person left it, whatever the footage.
      await user.click(fastScan())
      await user.click(screen.getByRole("radio", { name: "Live action" }))
      expect(isOn()).toBe(false)
      expect(await create(user)).toBe(false)
    })

    it("with faceless people on, live action starts with the full scan", async () => {
      state.findBodies = true
      const user = userEvent.setup()
      show()
      await waitFor(() => expect(line()).toBe("Much faster, finds fewer faces and no one without a visible face."))
      expect(isOn()).toBe(false)
      expect(await create(user)).toBe(false)
    })

    it("while the defaults load, the switch waits and Create sends null", async () => {
      state.defaults = "wait"
      const user = userEvent.setup()
      show()
      expect(fastScan().hasAttribute("disabled")).toBe(true)
      expect(line()).toBeUndefined()
      expect(await create(user)).toBeNull()
    })

    it("if the defaults fail, Create sends what the switch shows", async () => {
      state.defaults = "fail"
      const user = userEvent.setup()
      show()
      // The settings say live action starts with Fast Scan here.
      await waitFor(() => expect(isOn()).toBe(true))
      expect(fastScan().hasAttribute("disabled")).toBe(false)
      // Only what it costs: without the defaults there is no why.
      expect(line()).toBe("Much faster, finds fewer faces.")
      await user.click(screen.getByRole("radio", { name: "Animation" }))
      expect(isOn()).toBe(false)
      expect(await create(user)).toBe(false)
    })
  })
})
