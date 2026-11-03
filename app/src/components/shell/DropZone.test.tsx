import { RouterProvider, createMemoryHistory, createRootRoute, createRouter } from "@tanstack/react-router"
import { act, cleanup, render, screen } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { useNewProject } from "@/state/new-project"
import { DropZone } from "./DropZone"

type Drag = { type: "enter" | "over" | "drop" | "leave"; paths: string[] }

const { dropped, mediaAdd } = vi.hoisted(() => ({
  dropped: { handler: null as null | ((e: { payload: Drag }) => void) },
  mediaAdd: vi.fn(async () => 2),
}))

vi.mock("@/lib/platform", async (importOriginal) => ({ ...(await importOriginal<typeof import("@/lib/platform")>()), isTauri: true }))
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (e: { payload: Drag }) => void) => {
      dropped.handler = handler
      return Promise.resolve(() => {})
    },
  }),
}))
vi.mock("@/lib/ipc", async (importOriginal) => {
  const ipc = await importOriginal<typeof import("@/lib/ipc")>()
  return { ...ipc, commands: { ...ipc.commands, mediaAdd } }
})
vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), { error: vi.fn() }) }))

/** The window at `path`, with its drop zone. */
async function at(path: string) {
  const root = createRootRoute({ component: DropZone })
  const router = createRouter({ routeTree: root, history: createMemoryHistory({ initialEntries: [path] }) })
  render(<RouterProvider router={router} />)
  await act(() => router.load())
}

const files = [String.raw`D:\Video\Show S01E04.mkv`]
const drag = (type: Drag["type"]) => act(() => dropped.handler!({ payload: { type, paths: files } }))

beforeEach(() => {
  vi.spyOn(window, "scrollTo").mockImplementation(() => {})
  useNewProject.setState({ open: false, seed: [], dragging: false })
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  dropped.handler = null
})

describe("dropping files on the window", () => {
  it("a drop on a plugin page adds nothing", async () => {
    for (const path of ["/x/dev.a.page/main", "/p/p1/x/dev.a.page/clips/3"]) {
      await at(path)
      drag("enter")
      // Nothing says it would take them…
      expect(screen.queryByText("Add to this project")).toBeNull()
      expect(screen.queryByText("Create a project")).toBeNull()
      drag("drop")
      // …and nothing takes them: no project, no new one.
      expect(mediaAdd).not.toHaveBeenCalled()
      expect(useNewProject.getState().open).toBe(false)
      cleanup()
    }

    // Elsewhere in a project a drop adds to it, as always.
    await at("/p/p1/cast")
    drag("enter")
    expect(screen.getByText("Add to this project")).toBeTruthy()
    drag("drop")
    expect(mediaAdd).toHaveBeenCalledWith("p1", files)
  })
})
