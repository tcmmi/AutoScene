import type * as React from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider, createMemoryHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router"
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import type { CharacterShot, MediaItem, ProjectSummary } from "@/bindings"
import { AppMenu } from "@/components/shell/AppMenu"
import { TooltipProvider } from "@/components/ui/tooltip"
import { stackCards } from "@/features/shots/groups"
import { ShotCard } from "@/features/shots/ShotCard"
import { ShotBand, ShotStack } from "@/features/shots/ShotStack"
import { Welcome } from "./LibraryPage"
import { ProjectItems } from "./project-actions"
import { useNewProject } from "@/state/new-project"
import { NewProjectDialog } from "./NewProjectDialog"
import { ProjectDetailsDialog } from "./ProjectDialogs"

const { toast, commands } = vi.hoisted(() => ({
  toast: { error: vi.fn(), success: vi.fn() },
  commands: {
    projectSetContent: vi.fn(),
    projectSetFastScan: vi.fn(),
    projectRename: vi.fn(),
    scanDefaultsGet: vi.fn(),
    importScan: vi.fn(),
    settingsGet: vi.fn(),
  },
}))
vi.mock("sonner", () => ({ toast }))
vi.mock("@/lib/ipc", () => ({
  commands,
  errorMessage: (e: unknown) => String(e),
}))

/** These pin the English the screens say: each sentence of a branch that takes its words from the catalog, as people read it today. */
/** The screen under test, inside the app's own menu and a router, as it runs. */
async function mount(ui: React.ReactElement) {
  const root = createRootRoute({
    component: () => (
      <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
        <TooltipProvider>
          <AppMenu>
            <div>{ui}</div>
          </AppMenu>
        </TooltipProvider>
      </QueryClientProvider>
    ),
  })
  const here = createRoute({
    getParentRoute: () => root,
    path: "/",
    component: () => null,
  })
  const router = createRouter({
    routeTree: root.addChildren([here]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  })
  await router.load()
  return render(<RouterProvider router={router} />)
}

beforeEach(() => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  Object.defineProperty(Element.prototype, "scrollTo", {
    configurable: true,
    value: () => {},
  })
  HTMLImageElement.prototype.decode = () => Promise.resolve()
  commands.scanDefaultsGet.mockResolvedValue({
    computer: { class: "forAi", name: "" },
    choice: "auto",
    footage: [],
    notice: false,
  })
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  useNewProject.setState({ open: false, seed: [] })
})

const media = { id: "e1", label: "E01", fps: { num: 24, den: 1 } } as MediaItem
const shot = (more: Partial<CharacterShot> = {}) =>
  ({
    mediaId: "e1",
    start: 7512,
    end: 7512 + 53,
    sprite: "s.jpg",
    cells: 4,
    trackId: 1,
    face: "f.jpg",
    faceHeight: 0.2,
    ...more,
  }) as CharacterShot

describe("a shot card says", () => {
  const card = (more: Partial<CharacterShot>, name?: string) =>
    mount(<ShotCard shot={shot(more)} media={media} name={name} onOpen={vi.fn()} onBody={vi.fn()} />)

  it("to click to watch, and where its shot is", async () => {
    await card({})
    const button = screen.getByRole("button")
    fireEvent.focus(button)
    expect(screen.getAllByText("Click to watch").length).toBeGreaterThan(0)
    expect(screen.queryByText(/found by their body/)).toBeNull()
    expect(button.getAttribute("aria-label")).toBe("Shot at E01 · 00:05:13:00, 2.2 s")
  })

  it("that a face does not show, where one does not", async () => {
    await card({ faceless: true, bodies: [4] })
    const button = screen.getByRole("button")
    fireEvent.focus(button)
    expect(screen.getAllByText("Their face doesn't show: found by their body · Click to watch").length).toBeGreaterThan(0)
    expect(button.getAttribute("aria-label")).toBe("Shot at E01 · 00:05:13:00, 2.2 s, without their face")
  })

  it("who it is not, by name when it has one", async () => {
    const view = await card({ faceless: true, bodies: [4] }, "Walter White")
    fireEvent.contextMenu(screen.getByRole("button"), {
      clientX: 4,
      clientY: 4,
    })
    expect(await screen.findByRole("menuitem", { name: "Not Walter White" })).toBeTruthy()
    expect(screen.getByRole("menuitem", { name: "Not a person" })).toBeTruthy()
    expect(screen.queryByRole("menuitem", { name: "Not them" })).toBeNull()
    await act(async () => view.unmount())
    await card({ faceless: true, bodies: [4] })
    fireEvent.contextMenu(screen.getByRole("button"), {
      clientX: 4,
      clientY: 4,
    })
    expect(await screen.findByRole("menuitem", { name: "Not them" })).toBeTruthy()
  })
})

describe("a stack and its band say", () => {
  const cardOf = (n: number) =>
    stackCards([
      {
        shots: Array.from({ length: n }, (_, i) =>
          shot({
            start: 7512 + i * 60,
            end: 7512 + i * 60 + 53,
            trackId: i + 1,
          }),
        ),
      },
    ])[0]

  it("how to show its shots, and how to hide them once open", async () => {
    const view = await mount(<ShotStack card={cardOf(3)} media={media} open={false} onToggle={vi.fn()} onWatch={vi.fn()} />)
    fireEvent.contextMenu(screen.getByRole("button", { expanded: false }), {
      clientX: 4,
      clientY: 4,
    })
    expect(await screen.findByRole("menuitem", { name: /Show its shots/ })).toBeTruthy()
    expect(screen.queryByRole("menuitem", { name: /Hide its shots/ })).toBeNull()
    expect(screen.getByRole("menuitem", { name: /Watch from the first shot/ })).toBeTruthy()
    await act(async () => view.unmount())
    await mount(<ShotStack card={cardOf(3)} media={media} open onToggle={vi.fn()} onWatch={vi.fn()} />)
    fireEvent.contextMenu(screen.getByRole("button", { expanded: true }), {
      clientX: 4,
      clientY: 4,
    })
    expect(await screen.findByRole("menuitem", { name: /Hide its shots/ })).toBeTruthy()
    expect(screen.queryByRole("menuitem", { name: /Show its shots/ })).toBeNull()
  })

  const band = (m?: MediaItem) =>
    mount(<ShotBand card={cardOf(3)} media={m} columns={3} notch={20} onOpen={vi.fn()} onWatch={vi.fn()} onClose={vi.fn()} />)

  it("where the band is and to watch from its clock, when the shots have a clock", async () => {
    await band(media)
    expect(screen.getByRole("region").getAttribute("aria-label")).toMatch(/^3 shots · [\d.]+ s at E01 · 05:13$/)
    expect(screen.getByRole("button", { name: "Watch from 05:13" })).toBeTruthy()
    expect(screen.queryByRole("button", { name: "Watch from the first shot" })).toBeNull()
  })

  it("only how many, and to watch from the first shot, when they have none", async () => {
    await band(undefined)
    expect(screen.getByRole("region").getAttribute("aria-label")).toBe("3 shots")
    expect(screen.getByRole("button", { name: "Watch from the first shot" })).toBeTruthy()
  })
})

describe("the library says", () => {
  it("what the three steps are, each with its own line", async () => {
    await mount(<Welcome onCreate={vi.fn()} />)
    const steps = [
      ["Add episodes", "Drop a season folder or a movie. Episodes are recognised and put in order automatically."],
      ["Meet the cast", "Every character is found across all episodes. Name, merge or correct them in a keystroke."],
      ["Export packs", "Frame-exact clips in ProRes, DNxHR or H.264, named by character and episode, ready for any editor."],
    ]
    for (const [title, body] of steps) {
      expect(screen.getByText(title).parentElement?.textContent, title).toMatch(new RegExp(`^[0-9]+${title}`))
      expect(screen.getByText(body)).toBeTruthy()
    }
  })

  const kit = {
    Item: ({ children, onSelect }: { children: React.ReactNode; onSelect: () => void }) => <button onClick={onSelect}>{children}</button>,
    Separator: () => <hr />,
  }
  const items = (kind: "series" | "movie") =>
    mount(
      <ProjectItems
        kit={kit as never}
        project={{ id: "p", title: "T", kind, dir: "D:\\T" } as ProjectSummary}
        onRename={vi.fn()}
        onDelete={vi.fn()}
      />,
    )

  it("Episodes for a series and Files for a movie", async () => {
    const view = await items("series")
    expect(screen.getByRole("button", { name: /^Episodes/ })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /^Files/ })).toBeNull()
    view.unmount()
    await items("movie")
    expect(screen.getByRole("button", { name: /^Files/ })).toBeTruthy()
    expect(screen.queryByRole("button", { name: /^Episodes/ })).toBeNull()
  })
})

describe("Edit details says", () => {
  const live = {
    id: "p",
    title: "Breaking Bad",
    kind: "series",
    content: "live",
    fastScan: false,
  } as ProjectSummary
  const dialog = (p = live) => mount(<ProjectDetailsDialog project={p} onOpenChange={vi.fn()} />)
  const save = async () => {
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Save" })))
  }

  it("that it could not change the project, when nothing changed", async () => {
    commands.projectRename.mockRejectedValue("no")
    await dialog()
    fireEvent.change(screen.getByLabelText("Title"), {
      target: { value: "Other" },
    })
    await save()
    expect(toast.error).toHaveBeenCalledWith("Couldn't change the project", {
      description: "no",
    })
  })

  it("that some details changed, when one did", async () => {
    commands.projectSetFastScan.mockResolvedValue(undefined)
    commands.projectRename.mockRejectedValue("no")
    await dialog()
    fireEvent.click(screen.getByRole("switch", { name: "Fast Scan" }))
    fireEvent.change(screen.getByLabelText("Title"), {
      target: { value: "Other" },
    })
    await save()
    expect(toast.error).toHaveBeenCalledWith("Some details changed, but not all of them", { description: "no" })
  })

  it("why anime can not be picked in a project that is not anime", async () => {
    const apart = "Anime is analysed differently from the start. Add the videos as a new anime project instead."
    await dialog()
    const anime = screen.getByRole("radio", { name: "Anime" })
    expect(anime.hasAttribute("disabled") || anime.getAttribute("aria-disabled") === "true").toBe(true)
    fireEvent.pointerEnter(anime.parentElement!, { pointerType: "mouse" })
    expect((await screen.findAllByText(apart)).length).toBeGreaterThan(0)
  })
})

describe("New project says", () => {
  it("to skip a file, and once skipped to include it, in the tip and for a screen reader", async () => {
    commands.importScan.mockResolvedValue([
      {
        path: "D:/Show/Show - 01.mkv",
        fileName: "Show - 01.mkv",
        season: null,
        episode: 1,
        label: "E01",
        sizeBytes: 1e9,
        duplicate: false,
        refused: null,
      },
    ])
    commands.settingsGet.mockResolvedValue({
      findBodies: false,
      fastScan: false,
    })
    useNewProject.getState().show(["D:/Show"])
    await mount(<NewProjectDialog />)
    const skip = await screen.findByRole("button", {
      name: "Skip Show - 01.mkv",
    })
    fireEvent.focus(skip)
    expect(screen.getAllByText("Skip it").length).toBeGreaterThan(0)
    expect(screen.queryByText("Include it")).toBeNull()
    await act(async () => fireEvent.click(skip))
    const include = screen.getByRole("button", {
      name: "Include Show - 01.mkv",
    })
    fireEvent.blur(skip)
    fireEvent.focus(include)
    expect(screen.getAllByText("Include it").length).toBeGreaterThan(0)
  })
})
